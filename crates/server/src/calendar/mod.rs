//! The calendar, on the server: the Google Calendar account's events, read-only, and Grêmio's
//! matches as a calendar of their own, with a card on the TVs ten minutes before each event. From
//! the old launcher's `calendar/`.
//!
//! - Sign-in: Google's consent page with read-only Calendar access, through the Desktop app
//!   client Google Photos and YouTube use. The server has no browser: a TV PC opens the page and
//!   hands back the code (`oauth.loopback`), and the server exchanges and keeps the token. The
//!   Cloud project needs the Google Calendar API on.
//! - Calendars: `calendarList.list`. The ones ticked in Google Calendar are shown at first; the
//!   chips on the Calendar screen show and hide them on the TVs (calendar-prefs.json).
//! - Events: `events.list` per shown calendar, repeating ones expanded into their occurrences,
//!   from a week back to seven weeks ahead, every five minutes (kept in calendar-events.json, so
//!   a start without the network still has them); weeks and months further off are read when the
//!   screen pages to them.
//! - Grêmio's matches: the TV PCs' Sports tab reads them and sends them over (`feed.sports`),
//!   kept in gremio-feed.json so they show while every TV PC is off. (Sports moves to the server
//!   in M2.)
//! - Reminders: every timed event, ten minutes before, unless declined or turned off for it.
//!   Grêmio's matches keep the Sports tab's reminders (15 minutes before, off until asked for).
//!   Each TV PC that's on shows it (`calendar.card`), over the page of the app in front too.
//! - Travel time to an event's place, from the home address in Settings: travel.rs.

mod travel;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::future::join_all;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tvl_kit::oauth::{google, TokenStore};
use tvl_kit::store::JsonStore;
use tvl_kit::time::{civil_from_days, now_ms, rfc3339_ms, Day, DAY, HOUR, MIN};
use tvl_kit::{Error, Result};
use tvl_protocol::ServiceLink as Link;

use crate::ctx::Ctx;

pub use travel::Travel;

const TOKEN_FILE: &str = "calendar.json";
const PREFS_FILE: &str = "calendar-prefs.json";
const EVENTS_FILE: &str = "calendar-events.json";
const FEED_FILE: &str = "gremio-feed.json";
const SCOPE: &str = "https://www.googleapis.com/auth/calendar.readonly";
const API: &str = "https://www.googleapis.com/calendar/v3";
/// Only what the screen shows, of each event.
const FIELDS: &str = "nextPageToken,items(id,status,summary,start,end,location,description,hangoutLink,\
                      conferenceData(entryPoints(entryPointType,uri)),colorId,attendees(self,responseStatus),eventType)";

/// The calendar of Grêmio's matches.
const GREMIO: &str = "gremio";
const GREMIO_NAME: &str = "Grêmio · Sports";
const GREMIO_COLOR: &str = "#7986CB";
/// Google is read again this often.
const SYNC_MS: i64 = 5 * MIN;
/// The events kept at hand: from this long ago...
const BEHIND: i64 = 8 * DAY;
/// ...to this far ahead.
const AHEAD: i64 = 49 * DAY;
/// Weeks and months outside those, once read, are kept this long.
const RANGE_MS: i64 = 5 * MIN;
const REMIND_BEFORE: i64 = 10 * MIN;
/// Home's next event is at most this far ahead.
const NEXT_WITHIN: i64 = 7 * DAY;
/// Wherever the screen paged to, it also gets the events from a day back to this far ahead,
/// for its agenda and its Up next.
const SOON: i64 = 8 * DAY;

/// Google Calendar's colours as its web app shows them. The API still gives a calendar's colour
/// in the old palette (`backgroundColor`); a colour picked by hand comes as it is.
const CALENDAR_COLORS: &[(&str, &str)] = &[
    ("#ac725e", "#795548"), // Cocoa
    ("#d06b64", "#E67C73"), // Flamingo
    ("#f83a22", "#D50000"), // Tomato
    ("#fa573c", "#F4511E"), // Tangerine
    ("#ff7537", "#EF6C00"), // Pumpkin
    ("#ffad46", "#F09300"), // Mango
    ("#42d692", "#009688"), // Eucalyptus
    ("#16a765", "#0B8043"), // Basil
    ("#7bd148", "#7CB342"), // Pistachio
    ("#b3dc6c", "#C0CA33"), // Avocado
    ("#fbe983", "#E4C441"), // Citron
    ("#fad165", "#F6BF26"), // Banana
    ("#92e1c0", "#33B679"), // Sage
    ("#9fe1e7", "#039BE5"), // Peacock
    ("#9fc6e7", "#4285F4"), // Cobalt
    ("#4986e7", "#3F51B5"), // Blueberry
    ("#9a9cff", "#7986CB"), // Lavender
    ("#b99aff", "#B39DDB"), // Wisteria
    ("#c2c2c2", "#616161"), // Graphite
    ("#cabdbf", "#A79B8E"), // Birch
    ("#cca6ac", "#AD1457"), // Radicchio
    ("#f691b2", "#D81B60"), // Cherry blossom
    ("#cd74e6", "#8E24AA"), // Grape
    ("#a47ae2", "#9E69AF"), // Amethyst
];

/// An event's own colour, by its `colorId` (1–11).
const EVENT_COLORS: [&str; 11] = [
    "#7986CB", // Lavender
    "#33B679", // Sage
    "#8E24AA", // Grape
    "#E67C73", // Flamingo
    "#F6BF26", // Banana
    "#F4511E", // Tangerine
    "#039BE5", // Peacock
    "#616161", // Graphite
    "#3F51B5", // Blueberry
    "#0B8043", // Basil
    "#D50000", // Tomato
];

/// A calendar, for the screen's chips.
#[derive(Debug, Clone, Serialize)]
pub struct Calendar {
    pub id: String,
    pub name: String,
    pub color: String,
    /// On the TV: its events on the screen, as Home's next event and as reminders.
    pub shown: bool,
}

/// A calendar as Google lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Listed {
    id: String,
    name: String,
    color: String,
    /// Ticked in Google Calendar.
    selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Google's id of this occurrence; "gremio-<ESPN id>" for a match.
    pub id: String,
    pub calendar: String,
    pub title: String,
    /// Unix ms. All day: the UTC midnights of `date` and `end_date`; the screen places those by
    /// the dates.
    pub start: i64,
    pub end: i64,
    /// All day: the first day, "2026-10-27".
    pub date: Option<String>,
    /// All day: the day after the last.
    pub end_date: Option<String>,
    pub color: String,
    pub location: Option<String>,
    pub description: Option<String>,
    /// The video call's link.
    pub meet: Option<String>,
    /// Declined in the invitation.
    pub declined: bool,
    /// A reminder will show on the TV.
    #[serde(default)]
    pub remind: bool,
    /// Grêmio's match, for its match center: the Sports tab's game, as the TV PC sent it.
    pub game: Option<Value>,
}

/// What the Calendar screen shows.
#[derive(Debug, Clone, Serialize)]
pub struct Agenda {
    pub link: Link,
    pub calendars: Vec<Calendar>,
    /// Those between the times asked for and those of the next days, of every calendar.
    pub events: Vec<Event>,
    /// When Google was last read.
    pub synced: Option<i64>,
    /// Reminders on the TV are on (Settings).
    pub reminders: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Prefs {
    /// Calendars shown (true) or hidden on the TV, by id; the rest as ticked in Google Calendar.
    #[serde(default)]
    shown: HashMap<String, bool>,
    /// Events whose reminder was turned off, with when they end: forgotten after that.
    #[serde(default)]
    muted: HashMap<String, i64>,
}

/// Google's events at hand.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Window {
    /// When they were read.
    at: i64,
    from: i64,
    to: i64,
    calendars: Vec<Listed>,
    events: Vec<Event>,
}

/// Grêmio's matches as a TV PC's Sports tab last sent them (`feed.sports`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Feed {
    /// The Sports tab's games (its `Game`, as JSON).
    #[serde(default)]
    pub games: Vec<Value>,
    /// Ids of the games with a reminder on.
    #[serde(default)]
    pub reminders: Vec<String>,
    #[serde(default)]
    pub at: i64,
}

#[derive(Default)]
struct Inner {
    error: Option<String>,
    window: Option<Window>,
    /// The last time Google was read, or tried.
    tried: i64,
    /// Weeks and months outside the window, by (from, to), with when they were read.
    ranges: HashMap<(i64, i64), (i64, Vec<Event>)>,
    /// Reminders already shown: the event's id and start.
    fired: HashSet<(String, i64)>,
}

pub struct State {
    dir: PathBuf,
    token: TokenStore,
    prefs: JsonStore<Prefs>,
    feed: JsonStore<Feed>,
    inner: Mutex<Inner>,
    /// One read of Google at a time; the next caller finds it done.
    reading: tokio::sync::Mutex<()>,
    places: travel::Places,
}

impl State {
    pub fn new(data_dir: &Path) -> Self {
        let token = TokenStore::open(data_dir.join(TOKEN_FILE), "Google Calendar");
        let window = if token.signed_in() { tvl_kit::store::load(&data_dir.join(EVENTS_FILE)) } else { None };
        Self {
            prefs: JsonStore::open(data_dir.join(PREFS_FILE)),
            feed: JsonStore::open_compact(data_dir.join(FEED_FILE)),
            inner: Mutex::new(Inner { window, ..Default::default() }),
            reading: tokio::sync::Mutex::new(()),
            places: travel::Places::new(data_dir),
            dir: data_dir.to_path_buf(),
            token,
        }
    }

    /// Another account (or none): what was read from the last one goes.
    fn forget(&self) {
        let mut inner = self.inner.lock();
        inner.error = None;
        inner.window = None;
        inner.tried = 0;
        inner.ranges.clear();
        let _ = std::fs::remove_file(self.dir.join(EVENTS_FILE));
    }
}

fn state(app: &Ctx) -> &State {
    &app.calendar
}

fn http(_app: &Ctx) -> reqwest::Client {
    tvl_kit::http::api()
}

fn reminders_on(app: &Ctx) -> bool {
    app.settings.get().calendar_reminders
}

/// The "Desktop app" OAuth client from Settings.
pub fn google_client(app: &Ctx) -> Result<google::Client> {
    let h = app.settings.get();
    if h.google_client_id.trim().is_empty() || h.google_client_secret.trim().is_empty() {
        return Err(Error::msg("Add the Google Client ID and secret first, then Save"));
    }
    Ok(google::Client { id: h.google_client_id.trim().into(), secret: h.google_client_secret.trim().into() })
}

pub fn snapshot(app: &Ctx) -> Link {
    let configured = google_client(app).is_ok();
    let signed_in = state(app).token.signed_in();
    let inner = state(app).inner.lock();
    Link {
        configured,
        signed_in,
        connected: signed_in && inner.error.is_none() && inner.window.is_some(),
        error: inner.error.clone(),
    }
}

fn emit(app: &Ctx) {
    app.bus.emit("calendar", &snapshot(app));
}

// ---------- Sign-in ----------

/// Signs in through TV PC `device` (its browser opens Google's page), then reads the calendars at
/// once: an error (the Calendar API off in the Cloud project) comes back from here.
pub async fn sign_in(app: &Ctx, device: &str) -> Result<()> {
    let token = crate::oauth::google(app, device, SCOPE).await?;
    let st = state(app);
    st.token.set(Some(token))?;
    st.forget();
    emit(app);
    sync(app, 0).await
}

pub fn sign_out(app: &Ctx) -> Result<()> {
    let st = state(app);
    st.token.set(None)?;
    st.forget();
    emit(app);
    Ok(())
}

/// A valid access token, refreshed when it's about to expire (one refresh at a time). Asked for
/// while reading only.
async fn access_token(app: &Ctx) -> Result<String> {
    let st = state(app);
    let client = google_client(app)?;
    let result = st.token.access(|tok| async move { google::renew(&tvl_kit::http::api(), &client, &tok).await }).await;
    if result.is_err() && !st.token.signed_in() {
        st.forget();
        emit(app);
    }
    result
}

impl State {
    pub fn feed_is_empty(&self) -> bool {
        self.feed.read(|f| f.games.is_empty())
    }

    pub fn signed_in(&self) -> bool {
        self.token.signed_in()
    }
}

/// A TV PC's Sports tab sent Grêmio's matches.
pub fn feed(app: &Ctx, payload: Value) {
    let mut feed: Feed = match serde_json::from_value(payload) {
        Ok(f) => f,
        Err(e) => return tracing::debug!("a sports feed that doesn't read: {e}"),
    };
    // Nothing this time (ge and ESPN both down on that PC): keep the matches from before.
    if feed.games.is_empty() {
        return;
    }
    feed.at = now_ms();
    if let Err(e) = state(app).feed.set(feed) {
        tracing::warn!("can't keep Grêmio's matches: {e}");
    }
}

// ---------- Reading Google ----------

/// Reads the calendars and the weeks around now, unless they were read (or tried) less than
/// `max_age` ago.
async fn sync(app: &Ctx, max_age: i64) -> Result<()> {
    let st = state(app);
    let _one = st.reading.lock().await;
    let now = now_ms();
    {
        let mut inner = st.inner.lock();
        if !st.token.signed_in() || (max_age > 0 && now - inner.tried < max_age) {
            return Ok(());
        }
        inner.tried = now;
    }
    let result = read_window(app, now).await;
    match &result {
        Ok(w) => {
            if let Err(e) = tvl_kit::store::save(&st.dir.join(EVENTS_FILE), w, false) {
                tracing::warn!("can't keep the events read: {e}");
            }
            let mut inner = st.inner.lock();
            inner.window = Some(w.clone());
            inner.ranges.clear();
            inner.error = None;
        }
        Err(e) => st.inner.lock().error = Some(e.to_string()),
    }
    emit(app);
    result.map(|_| ())
}

async fn read_window(app: &Ctx, now: i64) -> Result<Window> {
    let token = access_token(app).await?;
    let calendars = calendar_list(app, &token).await?;
    let (from, to) = (now - BEHIND, now + AHEAD);
    let events = read_events(app, &token, &calendars, from, to).await?;
    Ok(Window { at: now, from, to, calendars, events })
}

/// A week or month outside the window.
async fn read_range(app: &Ctx, from: i64, to: i64) -> Result<Vec<Event>> {
    let st = state(app);
    let _one = st.reading.lock().await;
    let token = access_token(app).await?;
    let known = st.inner.lock().window.as_ref().map(|w| w.calendars.clone());
    let calendars = match known {
        Some(c) => c,
        None => calendar_list(app, &token).await?,
    };
    read_events(app, &token, &calendars, from, to).await
}

/// One Calendar API call as the account.
async fn get(app: &Ctx, token: &str, url: &str, query: &[(&str, String)]) -> Result<Value> {
    let r = http(app).get(url).query(query).bearer_auth(token).send().await?;
    let status = r.status();
    let v: Value = r.json().await.unwrap_or_default();
    if status.is_success() {
        return Ok(v);
    }
    // "Google Calendar API has not been used in project … before or it is disabled."
    Err(Error::msg(match v["error"]["message"].as_str() {
        Some(m) if !m.is_empty() => format!("Google Calendar: {m}"),
        _ => format!("Google Calendar: {status}"),
    }))
}

async fn calendar_list(app: &Ctx, token: &str) -> Result<Vec<Listed>> {
    let mut out = Vec::new();
    let mut page: Option<String> = None;
    for _ in 0..10 {
        let mut query = vec![("maxResults", "250".to_string())];
        if let Some(p) = page.take() {
            query.push(("pageToken", p));
        }
        let v = get(app, token, &format!("{API}/users/me/calendarList"), &query).await?;
        out.extend(parse_calendars(&v));
        match v["nextPageToken"].as_str() {
            Some(n) if !n.is_empty() => page = Some(n.to_string()),
            _ => break,
        }
    }
    // The main calendar first, the rest as Google orders them.
    out.sort_by_key(|(primary, _)| !primary);
    Ok(out.into_iter().map(|(_, c)| c).collect())
}

/// The events of the calendars shown on the TV. A calendar that fails is left out (and logged),
/// unless they all do.
async fn read_events(app: &Ctx, token: &str, calendars: &[Listed], from: i64, to: i64) -> Result<Vec<Event>> {
    let shown: Vec<Listed> = {
        let prefs = state(app).prefs.get();
        calendars.iter().filter(|c| is_shown(&prefs, &c.id, c.selected)).cloned().collect()
    };
    // Owned values: a closure over references here fails the command future's lifetime check.
    let lists = join_all(shown.into_iter().map(|c| {
        let app = app.clone();
        let token = token.to_string();
        async move {
            let r = calendar_events(&app, &token, &c, from, to).await;
            (c, r)
        }
    }))
    .await;
    let mut events = Vec::new();
    let mut failed = None;
    let mut read = 0;
    let tried = lists.len();
    for (c, r) in lists {
        match r {
            Ok(list) => {
                read += 1;
                events.extend(list);
            }
            Err(e) => {
                tracing::warn!(calendar = c.name, "{e}");
                failed.get_or_insert(e);
            }
        }
    }
    match failed {
        Some(e) if read == 0 && tried > 0 => Err(e),
        _ => Ok(events),
    }
}

async fn calendar_events(app: &Ctx, token: &str, cal: &Listed, from: i64, to: i64) -> Result<Vec<Event>> {
    let url = format!("{API}/calendars/{}/events", urlencoding::encode(&cal.id));
    let mut out = Vec::new();
    let mut page: Option<String> = None;
    for _ in 0..10 {
        let mut query = vec![
            ("singleEvents", "true".to_string()),
            ("orderBy", "startTime".into()),
            ("timeMin", rfc3339(from)),
            ("timeMax", rfc3339(to)),
            ("maxResults", "2500".into()),
            ("fields", FIELDS.into()),
        ];
        if let Some(p) = page.take() {
            query.push(("pageToken", p));
        }
        let v = get(app, token, &url, &query).await?;
        out.extend(v["items"].as_array().into_iter().flatten().filter_map(|e| parse_event(e, cal)));
        match v["nextPageToken"].as_str() {
            Some(n) if !n.is_empty() => page = Some(n.to_string()),
            _ => break,
        }
    }
    Ok(out)
}

/// Unix ms as Google takes times: 2026-10-26T03:00:00Z.
fn rfc3339(ms: i64) -> String {
    let d = civil_from_days(ms.div_euclid(DAY));
    let s = ms.rem_euclid(DAY) / 1000;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", d.y, d.m, d.d, s / 3600, s / 60 % 60, s % 60)
}

/// The calendars in a page of `calendarList.list`, each with whether it's the main one.
fn parse_calendars(v: &Value) -> Vec<(bool, Listed)> {
    v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            if c["deleted"].as_bool() == Some(true) || c["hidden"].as_bool() == Some(true) {
                return None;
            }
            let primary = c["primary"].as_bool() == Some(true);
            let name = c["summaryOverride"].as_str().or(c["summary"].as_str()).unwrap_or("").trim();
            // The main calendar is named after the account's address.
            let name = match name {
                "" if !primary => "Calendar",
                n if primary && (n.is_empty() || n.contains('@')) => "Personal",
                n => n,
            };
            let bg = c["backgroundColor"].as_str().unwrap_or("#9fe1e7").to_lowercase();
            let color = CALENDAR_COLORS
                .iter()
                .find(|(old, _)| *old == bg)
                .map_or(bg.clone(), |(_, now)| now.to_string());
            Some((
                primary,
                Listed {
                    id: c["id"].as_str()?.to_string(),
                    name: name.to_string(),
                    color,
                    selected: c["selected"].as_bool() == Some(true),
                },
            ))
        })
        .collect()
}

/// A start or end: unix ms, and the date when it's all day.
fn when(v: &Value) -> Option<(i64, Option<String>)> {
    if let Some(t) = v["dateTime"].as_str() {
        return Some((rfc3339_ms(t)?, None));
    }
    let d = Day::parse_iso(v["date"].as_str()?)?;
    Some((d.days() * DAY, Some(d.iso())))
}

fn parse_event(v: &Value, cal: &Listed) -> Option<Event> {
    // Where the account works from that day: not an event.
    if v["status"].as_str() == Some("cancelled") || v["eventType"].as_str() == Some("workingLocation") {
        return None;
    }
    let (start, date) = when(&v["start"])?;
    let (end, end_date) = when(&v["end"]).unwrap_or_else(|| match &date {
        Some(d) => (start + DAY, Day::parse_iso(d).map(|d| d.plus(1).iso())),
        None => (start + HOUR, None),
    });
    let meet = v["hangoutLink"].as_str().map(str::to_string).or_else(|| {
        v["conferenceData"]["entryPoints"]
            .as_array()?
            .iter()
            .find(|p| p["entryPointType"].as_str() == Some("video"))?["uri"]
            .as_str()
            .map(str::to_string)
    });
    let color = v["colorId"]
        .as_str()
        .and_then(|c| c.parse::<usize>().ok())
        .and_then(|i| EVENT_COLORS.get(i.wrapping_sub(1)))
        .map_or_else(|| cal.color.clone(), |c| c.to_string());
    let declined = v["attendees"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|a| a["self"].as_bool() == Some(true) && a["responseStatus"].as_str() == Some("declined"));
    let text = |k: &str| v[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    Some(Event {
        id: v["id"].as_str()?.to_string(),
        calendar: cal.id.clone(),
        title: text("summary").unwrap_or_else(|| "(No title)".into()),
        start,
        end,
        date,
        end_date,
        color,
        location: text("location"),
        description: v["description"].as_str().map(plain).filter(|s| !s.is_empty()),
        meet,
        declined,
        remind: false,
        game: None,
    })
}

/// An event's description as text: Google Calendar keeps it in HTML.
fn plain(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        let Some(j) = rest[i..].find('>') else { break };
        out.push_str(&rest[..i]);
        // A line ends at <br> and at the end of a block.
        let tag = rest[i + 1..i + j].to_ascii_lowercase();
        let name = |t: &str| t.split(|c: char| !c.is_ascii_alphanumeric()).next().map(str::to_string);
        let ends = match tag.strip_prefix('/') {
            Some(closing) => matches!(name(closing).as_deref(), Some("p" | "div" | "li" | "tr")),
            None => name(&tag).as_deref() == Some("br"),
        };
        if ends {
            out.push('\n');
        }
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    let text = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    // One blank line at most, none at the ends.
    let mut lines: Vec<&str> = Vec::new();
    for l in text.lines().map(str::trim) {
        if l.is_empty() && lines.last().is_none_or(|p| p.is_empty()) {
            continue;
        }
        lines.push(l);
    }
    while lines.last() == Some(&"") {
        lines.pop();
    }
    lines.join("\n")
}

// ---------- Grêmio ----------

/// Grêmio's matches, as a TV PC's Sports tab last sent them (kept while every TV PC is off).
async fn gremio_games(app: &Ctx) -> Vec<Value> {
    state(app).feed.read(|f| f.games.clone())
}

/// A match as an event of the Grêmio calendar; None for a game that doesn't read. The fields are
/// the Sports tab's `Game`: id, sport, start, phase, home/away (name, score), venue, stage, note,
/// league_name, channels (name).
fn match_event(g: &Value) -> Option<Event> {
    let text = |v: &Value| v.as_str().filter(|s| !s.is_empty()).map(str::to_string);
    let id = text(&g["id"])?;
    let start = g["start"].as_i64()?;
    let length = if g["sport"] == "basketball" { 150 * MIN } else { 2 * HOUR };
    let channels: Vec<String> = g["channels"].as_array().into_iter().flatten().filter_map(|c| text(&c["name"])).collect();
    let place: Vec<String> = [text(&g["venue"]), (!channels.is_empty()).then(|| channels.join(", "))].into_iter().flatten().collect();
    let score = match (g["phase"].as_str(), text(&g["home"]["score"]), text(&g["away"]["score"])) {
        (Some("post"), Some(h), Some(a)) => Some(format!("Final score {h}–{a}")),
        _ => None,
    };
    let about: Vec<String> =
        [text(&g["league_name"]), text(&g["stage"]), text(&g["note"]), score].into_iter().flatten().collect();
    Some(Event {
        id: format!("gremio-{id}"),
        calendar: GREMIO.into(),
        title: format!("{} × {}", text(&g["home"]["name"])?, text(&g["away"]["name"])?),
        start,
        end: start + length,
        date: None,
        end_date: None,
        color: GREMIO_COLOR.into(),
        location: (!place.is_empty()).then(|| place.join(" · ")),
        description: (!about.is_empty()).then(|| about.join(" · ")),
        meet: None,
        declined: false,
        remind: false,
        game: Some(g.clone()),
    })
}

// ---------- What the screens ask for ----------

fn is_shown(prefs: &Prefs, id: &str, selected: bool) -> bool {
    prefs.shown.get(id).copied().unwrap_or(selected)
}

fn calendars(app: &Ctx) -> Vec<Calendar> {
    let st = state(app);
    let listed = st.inner.lock().window.as_ref().map(|w| w.calendars.clone()).unwrap_or_default();
    let prefs = st.prefs.get();
    let mut out: Vec<Calendar> = listed
        .into_iter()
        .map(|c| Calendar { shown: is_shown(&prefs, &c.id, c.selected), id: c.id, name: c.name, color: c.color })
        .collect();
    out.push(Calendar {
        id: GREMIO.into(),
        name: GREMIO_NAME.into(),
        color: GREMIO_COLOR.into(),
        shown: is_shown(&prefs, GREMIO, true),
    });
    out
}

fn shown_ids(app: &Ctx) -> HashSet<String> {
    calendars(app).into_iter().filter(|c| c.shown).map(|c| c.id).collect()
}

/// Those of `events` between `from` and `to`. All-day ones get a day's slack: their times are
/// UTC midnights, and the screen sorts them into its days by date.
fn overlapping(events: &[Event], from: i64, to: i64) -> Vec<Event> {
    events
        .iter()
        .filter(|e| {
            let slack = if e.date.is_some() { DAY } else { 0 };
            e.end + slack > from && e.start - slack < to
        })
        .cloned()
        .collect()
}

/// Google's events between `from` and `to`: from those at hand, else read now and kept a while.
async fn google_between(app: &Ctx, from: i64, to: i64) -> Vec<Event> {
    let st = state(app);
    let now = now_ms();
    {
        let inner = st.inner.lock();
        if !st.token.signed_in() {
            return Vec::new();
        }
        if let Some(w) = inner.window.as_ref().filter(|w| w.from <= from && to <= w.to) {
            return overlapping(&w.events, from, to);
        }
        if let Some((at, list)) = inner.ranges.get(&(from, to)) {
            if now - at < RANGE_MS {
                return list.clone();
            }
        }
    }
    match read_range(app, from, to).await {
        Ok(list) => {
            let mut inner = st.inner.lock();
            inner.ranges.retain(|_, (at, _)| now - *at < RANGE_MS);
            inner.ranges.insert((from, to), (now, list.clone()));
            list
        }
        // Offline: whatever part of it is at hand.
        Err(e) => {
            tracing::warn!("calendar: {e}");
            st.inner.lock().window.as_ref().map(|w| overlapping(&w.events, from, to)).unwrap_or_default()
        }
    }
}

/// Whether the TV reminds of an event, apart from its calendar being shown.
fn reminds(e: &Event, muted: &HashMap<String, i64>) -> bool {
    e.date.is_none() && !e.declined && !muted.contains_key(&e.id)
}

/// Sets each event's `remind`.
fn mark(app: &Ctx, events: &mut [Event]) {
    let on = reminders_on(app);
    let games = state(app).feed.read(|f| f.reminders.clone());
    let shown = shown_ids(app);
    let prefs = state(app).prefs.get();
    for e in events {
        e.remind = match &e.game {
            Some(g) => g["id"].as_str().is_some_and(|id| games.iter().any(|r| r == id)),
            None => on && shown.contains(&e.calendar) && reminds(e, &prefs.muted),
        };
    }
}

/// The Calendar screen: every calendar's events between `from` and `to` and in the next days,
/// Grêmio's matches among them.
pub async fn agenda(app: &Ctx, from: i64, to: i64) -> Result<Agenda> {
    // A failed read is in the link's error; the events read before still show.
    let _ = sync(app, SYNC_MS).await;
    let now = now_ms();
    let mut events = google_between(app, from, to).await;
    events.extend(google_between(app, now - DAY, now + SOON).await);
    events.extend(gremio_games(app).await.iter().filter_map(match_event));
    let mut seen = HashSet::new();
    events.retain(|e| seen.insert(e.id.clone()));
    events.sort_by_key(|e| (e.start, e.end));
    mark(app, &mut events);
    let synced = state(app).inner.lock().window.as_ref().map(|w| w.at);
    Ok(Agenda {
        link: snapshot(app),
        calendars: calendars(app),
        events,
        synced,
        reminders: reminders_on(app),
    })
}

/// Home's next event: the soonest timed one still to start on a shown calendar, within a week.
pub async fn next(app: &Ctx) -> Option<Event> {
    let now = now_ms();
    let mut events = google_between(app, now - DAY, now + SOON).await;
    events.extend(gremio_games(app).await.iter().filter_map(match_event));
    let shown = shown_ids(app);
    let mut next = events
        .into_iter()
        .filter(|e| e.date.is_none() && !e.declined && shown.contains(&e.calendar))
        .filter(|e| e.start > now && e.start < now + NEXT_WITHIN)
        .min_by_key(|e| e.start)?;
    mark(app, std::slice::from_mut(&mut next));
    Some(next)
}

/// The timed events starting between `from` and `to` on the calendars shown, Grêmio's matches
/// among them, soonest first: the day ahead, on an alarm's screen.
pub async fn rest_of_day(app: &Ctx, from: i64, to: i64) -> Vec<Event> {
    let mut events = google_between(app, from, to).await;
    events.extend(gremio_games(app).await.iter().filter_map(match_event));
    let shown = shown_ids(app);
    events.retain(|e| e.date.is_none() && !e.declined && shown.contains(&e.calendar) && e.start >= from && e.start < to);
    events.sort_by_key(|e| e.start);
    events
}

/// Shows or hides a calendar on the TV. One shown again is read from Google at once.
pub async fn show(app: &Ctx, id: &str, on: bool) -> Result<()> {
    state(app).prefs.change(|prefs| {
        prefs.shown.insert(id.to_string(), on);
        Ok(())
    })?;
    if on && id != GREMIO {
        sync(app, 0).await?;
    }
    Ok(())
}

/// Turns the TV reminder of an event on or off; resolves to whether it's on now.
pub fn remind(app: &Ctx, id: &str, end: i64, on: bool) -> Result<bool> {
    let now = now_ms();
    state(app).prefs.change(|prefs| {
        prefs.muted.retain(|_, until| *until > now);
        if on {
            prefs.muted.remove(id);
        } else {
            prefs.muted.insert(id.to_string(), end);
        }
        Ok(())
    })?;
    Ok(on)
}

/// The drive from home (Settings) to `place`; None without a home address, or when either
/// place isn't found.
pub async fn travel(app: &Ctx, place: &str) -> Option<Travel> {
    let home = app.settings.get().home_address.trim().to_string();
    if home.is_empty() {
        return None;
    }
    travel::drive(&http(app), &state(app).places, &home, place).await.unwrap_or_else(|e| {
        tracing::warn!("the drive to {place}: {e}");
        None
    })
}

// ---------- Reminders ----------

/// Shows the reminders due on every TV PC that's on (`calendar.card`): on its launcher, and over
/// the page of the app in front. In shadow mode the old launcher shows its own.
fn check_reminders(app: &Ctx) {
    if !reminders_on(app) || app.config.shadow {
        return;
    }
    let now = now_ms();
    let st = state(app);
    let shown = shown_ids(app);
    let muted = st.prefs.read(|p| p.muted.clone());
    let due: Vec<Event> = {
        let mut inner = st.inner.lock();
        let soon: Vec<Event> = inner
            .window
            .as_ref()
            .map(|w| {
                w.events
                    .iter()
                    .filter(|e| e.start - REMIND_BEFORE <= now && now < e.start)
                    .filter(|e| shown.contains(&e.calendar) && reminds(e, &muted))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        inner.fired.retain(|(_, start)| *start > now - DAY);
        soon.into_iter().filter(|e| inner.fired.insert((e.id.clone(), e.start))).collect()
    };
    for mut e in due {
        e.remind = true;
        card_on_tvs(app, &e);
    }
}

/// A reminder's card on every TV PC that's on (the Live tab's reminders will use it too).
pub fn card_on_tvs(app: &Ctx, e: &Event) {
    let payload = json!({ "event": e });
    for tv in app.devices.tvs().into_iter().filter(|t| t.online) {
        let (ctx, payload) = (app.clone(), payload.clone());
        tokio::spawn(async move {
            if let Err(e) = ctx.agents.call(&tv.id, "calendar.card", payload, None, 5_000, false).await {
                tracing::debug!(device = tv.id, "calendar card not shown: {e:?}");
            }
        });
    }
}

/// Reads Google every few minutes while signed in, and shows the reminders when they're due.
pub fn start(app: &Ctx) {
    let ctx = app.clone();
    tvl_kit::task::spawn_supervised("calendar", app.shutdown.clone(), move || {
        let app = ctx.clone();
        async move {
            loop {
                if let Err(e) = sync(&app, SYNC_MS).await {
                    tracing::warn!("calendar: {e}");
                }
                check_reminders(&app);
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cal() -> Listed {
        Listed { id: "me@gmail.com".into(), name: "Personal".into(), color: "#039BE5".into(), selected: true }
    }

    #[test]
    fn google_times() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        let ms = rfc3339_ms("2026-10-26T22:30:05Z").unwrap();
        assert_eq!(rfc3339(ms), "2026-10-26T22:30:05Z");
        assert_eq!(rfc3339(ms + 999), "2026-10-26T22:30:05Z");
    }

    #[test]
    fn calendars_in_the_web_app_colours() {
        let v: Value = serde_json::from_str(
            r##"{"items":[
              {"id":"family123@group.calendar.google.com","summary":"Família","summaryOverride":"Family","backgroundColor":"#d06b64","selected":true},
              {"id":"me@gmail.com","summary":"me@gmail.com","backgroundColor":"#9fe1e7","selected":true,"primary":true},
              {"id":"pt.brazilian#holiday@group.v.calendar.google.com","summary":"Holidays in Brazil","backgroundColor":"#123456"},
              {"id":"old","summary":"Old","deleted":true}
            ]}"##,
        )
        .unwrap();
        let cals = parse_calendars(&v);
        assert_eq!(cals.len(), 3);
        assert_eq!((cals[0].1.name.as_str(), cals[0].1.color.as_str(), cals[0].0), ("Family", "#E67C73", false));
        assert_eq!((cals[1].1.name.as_str(), cals[1].1.color.as_str(), cals[1].0), ("Personal", "#039BE5", true));
        // A colour picked by hand, and not ticked in Google Calendar.
        assert_eq!((cals[2].1.color.as_str(), cals[2].1.selected), ("#123456", false));
    }

    #[test]
    fn timed_event() {
        let v: Value = serde_json::from_str(
            r#"{"id":"abc_20261026T123000Z","status":"confirmed","summary":" Stand-up ",
                "start":{"dateTime":"2026-10-26T09:30:00-03:00","timeZone":"America/Sao_Paulo"},
                "end":{"dateTime":"2026-10-26T09:45:00-03:00"},
                "colorId":"2","hangoutLink":"https://meet.google.com/abc-defg-hij",
                "attendees":[{"self":true,"responseStatus":"accepted"},{"responseStatus":"declined"}]}"#,
        )
        .unwrap();
        let e = parse_event(&v, &cal()).unwrap();
        assert_eq!(e.title, "Stand-up");
        assert_eq!(e.start, rfc3339_ms("2026-10-26T12:30:00Z").unwrap());
        assert_eq!(e.end - e.start, 15 * MIN);
        assert_eq!((e.date, e.color.as_str()), (None, "#33B679"));
        assert_eq!(e.meet.as_deref(), Some("https://meet.google.com/abc-defg-hij"));
        assert!(!e.declined);
    }

    #[test]
    fn all_day_declined_and_cancelled() {
        let v: Value = serde_json::from_str(
            r#"{"id":"rent","summary":"Pay rent","start":{"date":"2026-10-28"},"end":{"date":"2026-10-29"},
                "conferenceData":{"entryPoints":[{"entryPointType":"phone","uri":"tel:+55"},{"entryPointType":"video","uri":"https://zoom.us/j/1"}]},
                "attendees":[{"self":true,"responseStatus":"declined"}]}"#,
        )
        .unwrap();
        let e = parse_event(&v, &cal()).unwrap();
        assert_eq!((e.date.as_deref(), e.end_date.as_deref()), (Some("2026-10-28"), Some("2026-10-29")));
        assert_eq!(e.end - e.start, DAY);
        assert_eq!((e.color.as_str(), e.meet.as_deref()), ("#039BE5", Some("https://zoom.us/j/1")));
        assert!(e.declined);
        assert!(!reminds(&e, &HashMap::new()));

        let gone: Value = serde_json::from_str(r#"{"id":"x","status":"cancelled","start":{"date":"2026-10-28"}}"#).unwrap();
        assert!(parse_event(&gone, &cal()).is_none());
        let office: Value =
            serde_json::from_str(r#"{"id":"y","eventType":"workingLocation","start":{"date":"2026-10-28"}}"#).unwrap();
        assert!(parse_event(&office, &cal()).is_none());
    }

    #[test]
    fn descriptions_as_text() {
        assert_eq!(
            plain("Plan the week&nbsp;ahead<br><br><br>and <b>clear</b> the inbox &amp; <a href=\"x\">notes</a>"),
            "Plan the week ahead\n\nand clear the inbox & notes"
        );
        assert_eq!(plain("<p>One</p><p>Two</p>"), "One\nTwo");
        assert_eq!(plain("a < b"), "a < b");
    }

    #[test]
    fn match_as_event() {
        // As the TV PC's Sports tab sends it (its `Game`, as JSON).
        let g = json!({
            "id": "1", "league": "bra.1", "league_name": "Brasileirão", "sport": "soccer", "start": 1_800_000_000_000_i64,
            "phase": "post", "status": "Full time", "progress": 1.0,
            "home": {"id": "3458", "name": "Coritiba", "abbr": "CFC", "score": "2", "color": "#006437", "winner": false},
            "away": {"id": "6273", "name": "Grêmio", "abbr": "GRE", "score": "4", "color": "#0d80bf", "winner": true},
            "venue": "Couto Pereira", "stage": null, "note": null,
            "channels": [{"name": "Premiere", "app": "globoplay"}], "mine": "gremio"
        });
        let e = match_event(&g).unwrap();
        assert!(match_event(&json!({"id": "2"})).is_none(), "a game without teams or a start is skipped");
        assert_eq!(e.title, "Coritiba × Grêmio");
        assert_eq!(e.end - e.start, 2 * HOUR);
        assert_eq!(e.location.as_deref(), Some("Couto Pereira · Premiere"));
        assert_eq!(e.description.as_deref(), Some("Brasileirão · Final score 2–4"));
        assert_eq!(e.id, "gremio-1");
    }
}
