//! Sports: Grêmio first, then the NBA, then Brazilian, South American and European football.
//!
//! - Scores, clocks, events, stats, lineups, box scores and tables: ESPN's site API, fetched when
//!   a screen asks and cached briefly while a game is live (espn.rs).
//! - Grêmio's schedule, Brazilian channels and highlights: ge's agenda page, read once a week;
//!   ge's daily agenda gives the channels of every other game (ge.rs).
//! - NBA channels in Brazil: Lakers Brasil's list, read every morning; a game it doesn't list is
//!   on League Pass (nba_br.rs).
//! - Highlights: a YouTube search, played inside the launcher like a trailer (youtube.rs).
//! - Live on TV: what Globoplay's channels, ESPN 1–6 and CazéTV show now and next (tv.rs), read
//!   when a screen asks; the Live tab (crate::livetv) shows the whole lineup. A channel plays with
//!   the live overlay (player/livetv.js): the channel rail for zapping, what's on, and the score of
//!   the match on air or of the user's team's game.
//!
//! Fallbacks: without ge, Grêmio's games come from ESPN's team schedules (no channels); without
//! ESPN's standings, the Brasileirão table comes from ge; without a YouTube video, ge's highlights
//! open on Globoplay.

pub mod cazetv;
mod channels;
mod dates;
mod espn;
mod ge;
mod nba_br;
pub mod tv;
mod youtube;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::join_all;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

pub use channels::Channel;
pub(crate) use dates::Day;
use espn::{NbaDetail, SoccerDetail, Stage, Table, Zone};
use ge::GeMatch;
use nba_br::Slate;
pub use tv::TvChannel;
use tv::Slot;

use crate::cdp::{self, Cmd};
use crate::error::{Error, Result};
use crate::history::now_ms;
use crate::launcher;
use crate::state::AppState;

/// Running-app id of a broadcaster's page that isn't in apps.toml (League Pass, Paramount+).
pub const WATCH_PAGE: &str = "sports-watch";

const MIN: i64 = 60_000;
const HOUR: i64 = 60 * MIN;
const DAY: i64 = 24 * HOUR;
/// A source that failed is tried again after this long.
const RETRY: i64 = HOUR;
/// Lakers Brasil updates its list in the morning.
const NBA_BR_FROM_HOUR: i64 = 7;
const REMIND_BEFORE: i64 = 15 * MIN;
const OVERLAY_EVERY: Duration = Duration::from_secs(20);
/// The live overlay is sent what's on this often (the sources behind it are cached).
const LIVE_EVERY: Duration = Duration::from_secs(5);
/// Globoplay's live programme: the page lists five slots ahead, a few hours.
const TV_GLOBO_EVERY: i64 = 15 * MIN;
/// meuguia.tv lists a week ahead, guiadetv (ESPN 6) the day.
const TV_ESPN_EVERY: i64 = 3 * HOUR;
/// CazéTV's streams start and end through the day.
const TV_CAZE_EVERY: i64 = 10 * MIN;
/// Watch pages read for the start of CazéTV's streams, without the Data API: one per channel.
const CAZE_PAGES: usize = 4;
/// Games on the Home's Live now row; a football Saturday has dozens.
const LIVE_GAMES: usize = 8;
const LIVE_SCRIPT: &str = include_str!("../player/livetv.js");
/// A highlights search that found nothing is tried again after this long.
const HIGHLIGHTS_RETRY: i64 = 30 * MIN;
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

/// The user's teams on ESPN.
const GREMIO: &str = "6273";
const LAKERS: &str = "13";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sport {
    Soccer,
    Basketball,
}

/// What comes first after Grêmio, in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Region {
    Nba,
    Brazil,
    SouthAmerica,
    Europe,
}

pub struct League {
    /// ESPN's slug.
    pub id: &'static str,
    pub sport: Sport,
    pub name: &'static str,
    pub color: &'static str,
    pub region: Region,
    /// ge's names for it.
    pub ge: &'static [&'static str],
    /// Where it's usually on in Brazil, for its card; empty when it changes by the match.
    pub channels: &'static str,
}

const fn soccer(
    id: &'static str,
    name: &'static str,
    color: &'static str,
    region: Region,
    ge: &'static [&'static str],
    channels: &'static str,
) -> League {
    League { id, sport: Sport::Soccer, name, color, region, ge, channels }
}

pub const LEAGUES: &[League] = &[
    League {
        id: "nba",
        sport: Sport::Basketball,
        name: "NBA",
        color: "#1D428A",
        region: Region::Nba,
        ge: &[],
        channels: "ESPN · Prime Video · League Pass",
    },
    soccer("bra.1", "Brasileirão", "#2B8A3E", Region::Brazil, &["Campeonato Brasileiro"], "Premiere · Globo · sportv"),
    soccer("bra.copa_do_brazil", "Copa do Brasil", "#1971C2", Region::Brazil, &["Copa do Brasil"], "Prime Video · sportv · Premiere"),
    soccer("bra.camp.gaucho", "Gauchão", "#C92A2A", Region::Brazil, &["Campeonato Gaúcho"], "RBS TV · Premiere"),
    soccer(
        "conmebol.libertadores",
        "Libertadores",
        "#D9A400",
        Region::SouthAmerica,
        &["Taça Conmebol Libertadores", "Copa Libertadores", "Libertadores"],
        "Globo · ESPN · Paramount+",
    ),
    soccer("conmebol.sudamericana", "Sul-Americana", "#1098AD", Region::SouthAmerica, &["Copa Sul-Americana"], "Paramount+ · Disney+"),
    soccer("eng.1", "Premier League", "#7950F2", Region::Europe, &["Premier League", "Campeonato Inglês"], "ESPN · Disney+"),
    soccer("esp.1", "LaLiga", "#E03131", Region::Europe, &["LALIGA", "Campeonato Espanhol"], "CazéTV · YouTube"),
    soccer("ita.1", "Serie A", "#0B7285", Region::Europe, &["Campeonato Italiano"], ""),
    soccer("ger.1", "Bundesliga", "#C2255C", Region::Europe, &["Bundesliga", "Campeonato Alemão"], ""),
    soccer("uefa.champions", "Champions League", "#4C6EF5", Region::Europe, &["Champions League", "Liga dos Campeões"], "HBO Max"),
];

pub fn league(id: &str) -> Option<&'static League> {
    LEAGUES.iter().find(|l| l.id == id)
}

fn league_for_ge(championship: &str) -> Option<&'static League> {
    let c = fold(championship);
    LEAGUES.iter().find(|l| l.ge.iter().any(|g| fold(g) == c))
}

fn soccer_leagues() -> impl Iterator<Item = &'static League> {
    LEAGUES.iter().filter(|l| l.sport == Sport::Soccer)
}

// ---------- Games ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: String,
    pub name: String,
    pub abbr: String,
    pub logo: Option<String>,
    pub color: String,
    pub score: Option<String>,
    pub winner: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Pre,
    Live,
    Post,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    /// ESPN's event id; "ge-<id>" for a match only ge knows.
    pub id: String,
    pub league: String,
    pub league_name: String,
    pub sport: Sport,
    /// Unix ms.
    pub start: i64,
    pub phase: Phase,
    /// "Q3 · 7:46", "67'", "HT", "Final", "Full time"; empty before the start.
    pub status: String,
    /// How far through the game, 0..1.
    pub progress: f32,
    pub home: Team,
    pub away: Team,
    pub venue: Option<String>,
    /// "Round 32", "Semifinals · 1st Leg", "Regular season".
    pub stage: Option<String>,
    /// Where to watch in Brazil, watchable ones first.
    pub channels: Vec<Channel>,
    /// "gremio" or "lakers".
    pub mine: Option<String>,
    /// The second leg of a tie: "Agg 3–1".
    pub aggregate: Option<String>,
    /// ge's highlights on Globoplay.
    pub ge_video: Option<String>,
    /// Postponed, canceled.
    pub note: Option<String>,
}

impl Game {
    fn has(&self, team: &str) -> bool {
        self.home.id == team || self.away.id == team
    }

    fn televised(&self) -> bool {
        self.channels.iter().any(|c| c.app.is_some())
    }

    fn region(&self) -> Region {
        league(&self.league).map_or(Region::Europe, |l| l.region)
    }
}

// ---------- Text helpers ----------

/// Lower case without accents.
pub(crate) fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'ª' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'º' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c => c,
        })
        .collect()
}

/// `fold`, only letters and digits, one space between words.
pub(crate) fn words(s: &str) -> String {
    fold(s)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Words too common in club names to tell two apart.
const GENERIC: &[&str] = &[
    "club", "clube", "sport", "sporting", "esporte", "futebol", "football", "atletico", "athletico", "real",
    "united", "city", "fc", "ec", "sc", "de", "da", "do",
];

/// ge's and ESPN's names for the same club: "Vasco" / "Vasco da Gama", "Bayern de Munique" /
/// "Bayern Munich".
fn same_team(a: &str, b: &str) -> bool {
    let (a, b) = (words(a), words(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b || a.contains(&b) || b.contains(&a) {
        return true;
    }
    let key = |s: &str| -> HashSet<String> {
        s.split(' ')
            .filter(|w| w.len() >= 4 && !GENERIC.contains(w))
            .map(str::to_string)
            .collect()
    };
    !key(&a).is_disjoint(&key(&b))
}

// ---------- State ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Stored<T> {
    fetched_at: i64,
    data: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reminder {
    pub id: String,
    /// Kick-off, unix ms.
    pub at: i64,
    pub text: String,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Saved {
    /// The score in a corner of the broadcaster's page while a game plays.
    #[serde(default = "yes")]
    overlay: bool,
    #[serde(default)]
    reminders: Vec<Reminder>,
}

impl Default for Saved {
    fn default() -> Self {
        Self { overlay: true, reminders: Vec::new() }
    }
}

struct Cached {
    until: i64,
    fetched_at: i64,
    value: Arc<Value>,
}

/// The sources read on a schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Source {
    GeGremio,
    GeAgenda,
    NbaBr,
    TvGlobo,
    TvEspn,
    TvCaze,
}

/// The sources read in the background; the live programme is read only when a screen asks.
const SCHEDULED: [Source; 3] = [Source::GeGremio, Source::GeAgenda, Source::NbaBr];
const TV_SOURCES: [Source; 3] = [Source::TvGlobo, Source::TvEspn, Source::TvCaze];

impl Source {
    fn file(self) -> &'static str {
        match self {
            Source::GeGremio => "ge-gremio.json",
            Source::GeAgenda => "ge-agenda.json",
            Source::NbaBr => "nba-br.json",
            // Globoplay's whole lineup (tv-globo.json held its sports channels only).
            Source::TvGlobo => "tv-globoplay.json",
            Source::TvEspn => "tv-espn.json",
            Source::TvCaze => "tv-cazetv.json",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Source::GeGremio => "ge (Grêmio)",
            Source::GeAgenda => "ge (agenda)",
            Source::NbaBr => "Lakers Brasil",
            Source::TvGlobo => "Globoplay (live TV)",
            Source::TvEspn => "meuguia.tv · guiadetv (ESPN)",
            Source::TvCaze => "YouTube (CazéTV)",
        }
    }

    /// A failed read is tried again after this long.
    fn retry(self) -> i64 {
        match self {
            Source::TvGlobo | Source::TvCaze => 5 * MIN,
            Source::TvEspn => 30 * MIN,
            _ => RETRY,
        }
    }
}

/// The channels the live overlay zaps through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lineup {
    /// The Sports tab's channels.
    Sports,
    /// The Live tab's: Globoplay's whole lineup, ESPN, CazéTV and the news channels on YouTube.
    All,
}

/// The live channel playing, while one does.
struct LiveSession {
    gen: u64,
    /// The app the browser was opened as (globoplay, disney); zapping keeps the same browser.
    app_id: String,
    key: String,
    lineup: Lineup,
    /// Sends what's on at once, for a report that wants an answer.
    wake: Arc<tokio::sync::Notify>,
}

pub struct State {
    dir: PathBuf,
    cache: Mutex<HashMap<String, Cached>>,
    saved: Mutex<Saved>,
    gremio: Mutex<Option<Stored<Vec<GeMatch>>>>,
    agenda: Mutex<Option<Stored<Vec<GeMatch>>>>,
    slate: Mutex<Option<Stored<Slate>>>,
    tv_globo: Mutex<Option<Stored<Vec<TvChannel>>>>,
    tv_espn: Mutex<Option<Stored<EspnSlots>>>,
    tv_caze: Mutex<Option<Stored<Vec<cazetv::Stream>>>>,
    live: Mutex<Option<LiveSession>>,
    /// ge's Brasileirão table (form, position changes), and when it was read.
    ge_table: Mutex<Option<(i64, Vec<ge::GeRow>)>>,
    /// Last try and error of each scheduled source.
    tried: Mutex<HashMap<Source, i64>>,
    errors: Mutex<HashMap<Source, String>>,
    highlights: Mutex<HashMap<String, (i64, Vec<youtube::Video>)>>,
    /// Bumped to stop the score overlay and the live overlay's loop.
    overlay_gen: AtomicU64,
}

/// ESPN's channels' programmes, by channel key.
type EspnSlots = Vec<(String, Vec<Slot>)>;

fn load<T: DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

impl State {
    pub fn new(data_dir: &Path) -> Self {
        let dir = data_dir.join("sports");
        let _ = fs::create_dir_all(&dir);
        Self {
            cache: Mutex::default(),
            saved: Mutex::new(load(&dir.join("settings.json")).unwrap_or_default()),
            gremio: Mutex::new(load(&dir.join(Source::GeGremio.file()))),
            agenda: Mutex::new(load(&dir.join(Source::GeAgenda.file()))),
            slate: Mutex::new(load(&dir.join(Source::NbaBr.file()))),
            tv_globo: Mutex::new(load(&dir.join(Source::TvGlobo.file()))),
            tv_espn: Mutex::new(load(&dir.join(Source::TvEspn.file()))),
            tv_caze: Mutex::new(load(&dir.join(Source::TvCaze.file()))),
            live: Mutex::default(),
            ge_table: Mutex::default(),
            tried: Mutex::default(),
            errors: Mutex::default(),
            highlights: Mutex::default(),
            overlay_gen: AtomicU64::new(0),
            dir,
        }
    }

    fn save_settings(&self, saved: &Saved) -> Result<()> {
        fs::write(self.dir.join("settings.json"), serde_json::to_string_pretty(saved)?)?;
        Ok(())
    }
}

fn state(app: &AppHandle) -> &State {
    &app.state::<AppState>().inner().sports
}

fn http(app: &AppHandle) -> reqwest::Client {
    app.state::<AppState>().http.clone()
}

// ---------- Fetching ----------

/// How long an answer stays fresh, from what it says.
type Ttl = fn(&Value, i64) -> i64;

fn scoreboard_ttl(v: &Value, now: i64) -> i64 {
    let events = v["events"].as_array().cloned().unwrap_or_default();
    let hot = events.iter().any(|e| {
        let state = e["status"]["type"]["state"].as_str();
        let start = e["date"].as_str().and_then(crate::photos::rfc3339_ms).unwrap_or(i64::MAX);
        state == Some("in") || (state == Some("pre") && start - now < 30 * MIN)
    });
    if hot {
        20_000
    } else {
        5 * MIN
    }
}

fn summary_ttl(v: &Value, _: i64) -> i64 {
    match v["header"]["competitions"][0]["status"]["type"]["state"].as_str() {
        Some("in") => 15_000,
        Some("post") => HOUR,
        _ => 5 * MIN,
    }
}

fn slow_ttl(_: &Value, _: i64) -> i64 {
    30 * MIN
}

/// A JSON answer, from the cache while it's fresh. When the fetch fails, a stale answer beats none.
async fn get_json(app: &AppHandle, url: &str, ttl: Ttl) -> Result<Arc<Value>> {
    let now = now_ms();
    let st = state(app);
    if let Some(c) = st.cache.lock().unwrap().get(url) {
        if c.until > now {
            return Ok(c.value.clone());
        }
    }
    let fetched = async {
        let r = http(app).get(url).header("User-Agent", UA).send().await?.error_for_status()?;
        Ok::<Value, Error>(r.json::<Value>().await?)
    }
    .await;
    let mut cache = st.cache.lock().unwrap();
    match fetched {
        Ok(v) => {
            let value = Arc::new(v);
            cache.retain(|_, c| c.fetched_at > now - DAY);
            cache.insert(
                url.to_string(),
                Cached { until: now + ttl(&value, now), fetched_at: now, value: value.clone() },
            );
            Ok(value)
        }
        Err(e) => cache.get(url).map(|c| c.value.clone()).ok_or(e),
    }
}

async fn get_text(app: &AppHandle, url: &str) -> Result<String> {
    Ok(http(app)
        .get(url)
        .header("User-Agent", UA)
        .header("Accept-Language", "pt-BR,pt;q=0.9")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?)
}

async fn scoreboard(app: &AppHandle, league: &'static League, day: Option<Day>) -> Result<Arc<Value>> {
    get_json(app, &espn::scoreboard_url(league, day), scoreboard_ttl).await
}

/// Games of `league` on `day`, empty when ESPN can't be reached.
async fn games_on(app: &AppHandle, league: &'static League, day: Day) -> Vec<Game> {
    match scoreboard(app, league, Some(day)).await {
        Ok(v) => espn::parse_events(league, &v),
        Err(_) => Vec::new(),
    }
}

// ---------- Scheduled sources ----------

fn due(app: &AppHandle, source: Source, now: i64) -> bool {
    let st = state(app);
    if st.tried.lock().unwrap().get(&source).is_some_and(|t| now - t < source.retry()) {
        return false;
    }
    let today = Day::brt(now);
    match source {
        // Weekly, as asked: Grêmio's fixtures and channels don't change by the day.
        Source::GeGremio => st.gremio.lock().unwrap().as_ref().is_none_or(|s| now - s.fetched_at > 7 * DAY),
        Source::GeAgenda => st
            .agenda
            .lock()
            .unwrap()
            .as_ref()
            .is_none_or(|s| Day::brt(s.fetched_at) != today),
        Source::NbaBr => {
            let stale = st.slate.lock().unwrap().as_ref().is_none_or(|s| s.data.day < today);
            let hour = (now - today.brt_ms(0, 0)) / HOUR;
            stale && hour >= NBA_BR_FROM_HOUR
        }
        Source::TvGlobo => st.tv_globo.lock().unwrap().as_ref().is_none_or(|s| now - s.fetched_at > TV_GLOBO_EVERY),
        Source::TvEspn => st.tv_espn.lock().unwrap().as_ref().is_none_or(|s| now - s.fetched_at > TV_ESPN_EVERY),
        Source::TvCaze => st.tv_caze.lock().unwrap().as_ref().is_none_or(|s| now - s.fetched_at > TV_CAZE_EVERY),
    }
}

fn store<T: Serialize>(app: &AppHandle, source: Source, slot: &Mutex<Option<Stored<T>>>, data: T) {
    let stored = Stored { fetched_at: now_ms(), data };
    if let Ok(text) = serde_json::to_string(&stored) {
        let _ = fs::write(state(app).dir.join(source.file()), text);
    }
    *slot.lock().unwrap() = Some(stored);
}

async fn refresh(app: &AppHandle, source: Source) -> Result<()> {
    let st = state(app);
    match source {
        Source::GeGremio => {
            let html = get_text(app, ge::GREMIO_URL).await?;
            let ms = ge::parse_team(&html).ok_or_else(|| Error::msg("ge's Grêmio page changed"))?;
            store(app, source, &st.gremio, ms);
        }
        Source::GeAgenda => {
            let html = get_text(app, ge::AGENDA_URL).await?;
            let ms = ge::parse_agenda(&html).ok_or_else(|| Error::msg("ge's agenda page changed"))?;
            store(app, source, &st.agenda, ms);
        }
        Source::NbaBr => {
            let html = get_text(app, nba_br::URL).await?;
            let slate = nba_br::parse(&html, Day::brt(now_ms()).y)
                .ok_or_else(|| Error::msg("Lakers Brasil's page changed"))?;
            store(app, source, &st.slate, slate);
        }
        Source::TvGlobo => {
            let html = get_text(app, tv::GLOBO_URL).await?;
            let chans = tv::parse_globo(&html).ok_or_else(|| Error::msg("Globoplay's Agora na TV page changed"))?;
            store(app, source, &st.tv_globo, chans);
        }
        Source::TvEspn => {
            let now = now_ms();
            let urls: Vec<String> = tv::ESPN.iter().map(|c| c.listing.url()).collect();
            let pages = join_all(urls.iter().map(|u| get_text(app, u))).await;
            let old = st.tv_espn.lock().unwrap().as_ref().map(|s| s.data.clone()).unwrap_or_default();
            let mut read = 0;
            let mut out = Vec::new();
            for (c, page) in tv::ESPN.iter().zip(pages) {
                let key = c.key;
                let fresh = page.ok().map(|h| c.listing.parse(&h, now)).filter(|s| !s.is_empty());
                read += usize::from(fresh.is_some());
                // A page that didn't answer keeps what it said last time.
                let mut slots = fresh
                    .or_else(|| old.iter().find(|(k, _)| *k == key).map(|(_, s)| s.clone()))
                    .unwrap_or_default();
                tv::trim(&mut slots, now);
                out.push((key.to_string(), slots));
            }
            if read == 0 {
                return Err(Error::msg("ESPN's programme pages didn't answer or changed"));
            }
            store(app, source, &st.tv_espn, out);
        }
        Source::TvCaze => {
            let key = app.state::<AppState>().config.read().unwrap().settings.youtube_key();
            let page = caze_page(app).await;
            let api = match &key {
                Some(key) => Some(caze_api(app, key, page.as_ref().ok()).await),
                None => None,
            };
            let streams = match api {
                Some(Ok(streams)) => streams,
                // Without the API (no key, quota used up): the page, with its own start times.
                other => {
                    if let Some(Err(e)) = other {
                        eprintln!("sports: YouTube Data API: {e}");
                    }
                    caze_starts(app, page?).await
                }
            };
            store(app, source, &st.tv_caze, streams);
        }
    }
    Ok(())
}

/// CazéTV's streams tab: what's live and scheduled.
async fn caze_page(app: &AppHandle) -> Result<Vec<cazetv::Stream>> {
    streams_page(app, cazetv::URL, "CazéTV").await
}

/// A YouTube channel's streams tab (`url`): what's live and scheduled on `who`.
pub(crate) async fn streams_page(app: &AppHandle, url: &str, who: &str) -> Result<Vec<cazetv::Stream>> {
    let html = http(app)
        .get(url)
        .header("User-Agent", UA)
        .header("Accept-Language", "pt-BR,pt;q=0.9")
        .header("Cookie", cazetv::COOKIE)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    cazetv::parse(&html).ok_or_else(|| Error::msg(format!("{who}'s YouTube page changed")))
}

/// One YouTube Data API call; the key goes in a header.
async fn youtube_api(app: &AppHandle, key: &str, url: &str) -> Result<Value> {
    let r = http(app).get(url).header(cazetv::KEY_HEADER, key).send().await?;
    let ok = r.status().is_success();
    let v: Value = r.json().await?;
    match cazetv::api_error(&v) {
        Some(e) => Err(Error::msg(e)),
        None if !ok => Err(Error::msg("YouTube's API refused the request")),
        None => Ok(v),
    }
}

/// The streams' details from the Data API: the page's videos, or (the page gone) the channel's
/// newest uploads.
async fn caze_api(app: &AppHandle, key: &str, page: Option<&Vec<cazetv::Stream>>) -> Result<Vec<cazetv::Stream>> {
    let ids: Vec<String> = match page {
        Some(p) => p.iter().map(|s| s.id.clone()).collect(),
        None => {
            let mut ids = Vec::new();
            let mut token: Option<String> = None;
            for _ in 0..3 {
                let v = youtube_api(app, key, &cazetv::uploads_url(token.as_deref())).await?;
                let (more, next) = cazetv::parse_uploads(&v);
                ids.extend(more);
                token = next;
                if token.is_none() {
                    break;
                }
            }
            ids
        }
    };
    stream_details(app, key, &ids).await
}

/// The live and scheduled ones among `ids`, from the Data API (1 unit per 50): the real start
/// and the viewers.
pub(crate) async fn stream_details(app: &AppHandle, key: &str, ids: &[String]) -> Result<Vec<cazetv::Stream>> {
    let mut streams = Vec::new();
    for chunk in ids.chunks(50) {
        let v = youtube_api(app, key, &cazetv::videos_url(chunk)).await?;
        streams.extend(cazetv::parse_videos(&v));
    }
    Ok(streams)
}

/// When each of the page's live streams started: as seen before or scheduled, else (live when
/// first seen) as its watch page says, for the few that become channels.
async fn caze_starts(app: &AppHandle, streams: Vec<cazetv::Stream>) -> Vec<cazetv::Stream> {
    let old = state(app).tv_caze.lock().unwrap().as_ref().map(|s| s.data.clone()).unwrap_or_default();
    stream_starts(app, streams, &old, CAZE_PAGES).await
}

/// When each live stream started: as it was in `old` (live, or scheduled to start), else, live
/// when first seen, as its watch page says, for the `pages` most watched.
pub(crate) async fn stream_starts(
    app: &AppHandle,
    mut streams: Vec<cazetv::Stream>,
    old: &[cazetv::Stream],
    pages: usize,
) -> Vec<cazetv::Stream> {
    for s in streams.iter_mut().filter(|s| s.live) {
        s.since = old.iter().find(|o| o.id == s.id).and_then(|o| o.since.or(o.scheduled));
    }
    let mut unknown: Vec<(u64, String)> =
        streams.iter().filter(|s| s.live && s.since.is_none()).map(|s| (s.viewers, s.id.clone())).collect();
    unknown.sort_by_key(|(v, _)| std::cmp::Reverse(*v));
    unknown.truncate(pages);
    let urls: Vec<String> = unknown.iter().map(|(_, id)| cazetv::watch_url(id)).collect();
    let pages = join_all(urls.iter().map(|u| get_text(app, u))).await;
    let now = now_ms();
    for ((_, id), page) in unknown.iter().zip(pages) {
        if let Some(s) = streams.iter_mut().find(|s| s.id == *id) {
            s.since = page.ok().and_then(|h| cazetv::started(&h)).or(Some(now));
        }
    }
    streams
}

async fn refresh_due(app: &AppHandle) {
    refresh_sources(app, &SCHEDULED).await;
}

/// The live programme, when it's due.
async fn refresh_tv(app: &AppHandle) {
    refresh_sources(app, &TV_SOURCES).await;
}

async fn refresh_sources(app: &AppHandle, sources: &[Source]) {
    let now = now_ms();
    for &source in sources {
        if !due(app, source, now) {
            continue;
        }
        state(app).tried.lock().unwrap().insert(source, now);
        let result = refresh(app, source).await;
        let mut errors = state(app).errors.lock().unwrap();
        match result {
            Ok(()) => errors.remove(&source),
            Err(e) => {
                eprintln!("sports: {}: {e}", source.name());
                errors.insert(source, e.to_string())
            }
        };
    }
}

/// What ge and Lakers Brasil said, for decorating ESPN's games.
struct Extras {
    gremio: Vec<GeMatch>,
    agenda: Vec<GeMatch>,
    slate: Option<Slate>,
}

fn extras(app: &AppHandle) -> Extras {
    let st = state(app);
    Extras {
        gremio: st.gremio.lock().unwrap().as_ref().map(|s| s.data.clone()).unwrap_or_default(),
        agenda: st.agenda.lock().unwrap().as_ref().map(|s| s.data.clone()).unwrap_or_default(),
        slate: st.slate.lock().unwrap().as_ref().map(|s| s.data.clone()),
    }
}

/// ge's match and ESPN's game are the same one.
fn ge_is(m: &GeMatch, g: &Game) -> bool {
    if league_for_ge(&m.championship).is_none_or(|l| l.id != g.league) {
        return false;
    }
    let close = match m.start {
        Some(t) => (t - g.start).abs() <= 45 * MIN,
        None => m.day == Day::brt(g.start),
    };
    close && (same_team(&m.home.name, &g.home.name) || same_team(&m.away.name, &g.away.name))
}

fn sort_channels(channels: &mut [Channel]) {
    channels.sort_by_key(|c| !c.watchable());
}

/// Adds what ESPN doesn't know: the user's teams, Brazilian channels, ge's round and video.
fn decorate(g: &mut Game, x: &Extras) {
    match g.sport {
        Sport::Soccer => {
            if g.has(GREMIO) {
                g.mine = Some("gremio".into());
            }
            let ge = x.gremio.iter().chain(x.agenda.iter()).find(|m| ge_is(m, g));
            if let Some(m) = ge {
                if g.channels.is_empty() {
                    g.channels = m.channels.clone();
                }
                g.ge_video = g.ge_video.take().or_else(|| m.video.clone());
                if g.stage.is_none() {
                    g.stage = m.round.map(|r| format!("Round {r}"));
                }
            }
        }
        Sport::Basketball => {
            if g.has(LAKERS) {
                g.mine = Some("lakers".into());
            }
            let listed = x.slate.as_ref().filter(|s| s.day == Day::espn(g.start)).and_then(|s| {
                s.games.iter().find(|b| {
                    let code = |c: &str| nba_br::espn_code(c).to_string();
                    let home = code(&b.home) == g.home.abbr.to_lowercase() || same_team(&b.home_name, &g.home.name);
                    let away = code(&b.away) == g.away.abbr.to_lowercase() || same_team(&b.away_name, &g.away.name);
                    home && away
                })
            });
            g.channels = match listed {
                Some(b) => channels::nba(&b.channel),
                None => vec![channels::league_pass()],
            };
        }
    }
    sort_channels(&mut g.channels);
}

fn abbr(name: &str) -> String {
    words(name).replace(' ', "").chars().take(3).collect::<String>().to_uppercase()
}

/// A Grêmio match only ge has, e.g. while ESPN can't be reached.
fn from_ge(m: &GeMatch, now: i64) -> Game {
    let l = league_for_ge(&m.championship);
    let start = m.start.unwrap_or_else(|| m.day.brt_ms(12, 0));
    let phase = if m.live {
        Phase::Live
    } else if m.score.is_some() {
        Phase::Post
    } else {
        Phase::Pre
    };
    let team = |t: &ge::GeTeam, score: Option<i64>, other: Option<i64>| Team {
        id: if t.id == ge::GREMIO_ID { GREMIO.to_string() } else { format!("ge-{}", t.id) },
        name: t.name.clone(),
        abbr: abbr(&t.name),
        logo: t.badge.clone(),
        color: if t.id == ge::GREMIO_ID { "#0093EC".into() } else { "#2a2a2e".into() },
        score: score.filter(|_| phase != Phase::Pre).map(|s| s.to_string()),
        winner: phase == Phase::Post && score > other,
    };
    let (hs, aws) = (m.score.map(|s| s.0), m.score.map(|s| s.1));
    let phase_name = ge::phase_name(&m.phase);
    let stage = if !phase_name.is_empty() {
        Some(phase_name)
    } else {
        m.round.map(|r| format!("Round {r}"))
    };
    let mut channels = m.channels.clone();
    sort_channels(&mut channels);
    Game {
        id: format!("ge-{}", m.id),
        league: l.map_or("ge", |l| l.id).to_string(),
        league_name: l.map_or(m.championship.as_str(), |l| l.name).to_string(),
        sport: Sport::Soccer,
        start,
        phase,
        status: match phase {
            Phase::Live => "Live".into(),
            Phase::Post => "Full time".into(),
            Phase::Pre => String::new(),
        },
        progress: if phase == Phase::Post { 1.0 } else { ((now - start) as f32 / (110 * MIN) as f32).clamp(0.0, 1.0) },
        home: team(&m.home, hs, aws),
        away: team(&m.away, aws, hs),
        venue: m.venue.clone(),
        stage,
        channels,
        mine: Some("gremio".into()),
        aggregate: None,
        ge_video: m.video.clone(),
        note: None,
    }
}

// ---------- Grêmio ----------

/// Grêmio's last results and next matches: ge's list, with ESPN's live scores. Oldest first.
async fn gremio_games(app: &AppHandle, x: &Extras, now: i64) -> Vec<Game> {
    if x.gremio.is_empty() {
        return gremio_from_espn(app, x, now).await;
    }
    let mut list: Vec<&GeMatch> = x.gremio.iter().collect();
    list.sort_by_key(|m| m.start.unwrap_or_else(|| m.day.brt_ms(12, 0)));
    let next = list
        .iter()
        .position(|m| m.score.is_none() || m.live)
        .unwrap_or(list.len());
    let picked: Vec<&GeMatch> = list[next.saturating_sub(2)..(next + 6).min(list.len())].to_vec();
    let resolved = join_all(picked.iter().map(|m| async move {
        let l = league_for_ge(&m.championship)?;
        let day = m.start.map(Day::espn).unwrap_or(m.day);
        let v = scoreboard(app, l, Some(day)).await.ok()?;
        espn::parse_events(l, &v).into_iter().find(|g| g.has(GREMIO))
    }))
    .await;
    picked
        .into_iter()
        .zip(resolved)
        .map(|(m, espn)| match espn {
            Some(mut g) => {
                decorate(&mut g, x);
                if g.channels.is_empty() {
                    g.channels = m.channels.clone();
                }
                if let Some(stage) = from_ge(m, now).stage.filter(|_| g.stage.is_none()) {
                    g.stage = Some(stage);
                }
                g
            }
            None => from_ge(m, now),
        })
        .collect()
}

/// Without ge: Grêmio's schedules on ESPN, without Brazilian channels.
async fn gremio_from_espn(app: &AppHandle, x: &Extras, now: i64) -> Vec<Game> {
    let leagues: Vec<&'static League> = ["bra.1", "bra.copa_do_brazil", "conmebol.libertadores", "conmebol.sudamericana"]
        .into_iter()
        .filter_map(league)
        .collect();
    let urls: Vec<(&'static League, String)> = leagues
        .iter()
        .flat_map(|l| [(*l, espn::schedule_url(l, GREMIO, false)), (*l, espn::schedule_url(l, GREMIO, true))])
        .collect();
    let answers = join_all(urls.iter().map(|(_, u)| get_json(app, u, slow_ttl))).await;
    let mut games: Vec<Game> = Vec::new();
    for ((l, _), v) in urls.iter().zip(answers) {
        let Ok(v) = v else { continue };
        for mut g in espn::parse_events(l, &v) {
            if !games.iter().any(|x| x.id == g.id) {
                decorate(&mut g, x);
                games.push(g);
            }
        }
    }
    games.sort_by_key(|g| g.start);
    let next = games.iter().position(|g| g.phase != Phase::Post || g.start > now).unwrap_or(games.len());
    games.drain(..next.saturating_sub(2));
    games.truncate(8);
    games
}

// ---------- Home ----------

#[derive(Debug, Clone, Serialize)]
pub struct CompetitionCard {
    pub id: String,
    pub name: String,
    pub color: String,
    pub channels: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceInfo {
    pub name: String,
    pub updated: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Home {
    /// Top of the screen: what's live, Grêmio first; else what's next today.
    pub featured: Option<Game>,
    /// Featured now, beside the hero: the hero, the other live games, Grêmio's next.
    pub rail: Vec<Game>,
    /// The hero's details (scorers, events, stats) once it has started.
    pub hero: Option<GameDetail>,
    /// The next Grêmio match.
    pub gremio_next: Option<Game>,
    pub for_you: Vec<Game>,
    pub gremio: Vec<Game>,
    pub nba: Vec<Game>,
    pub football: Vec<Game>,
    /// Live on TV: the sports channels with what's on now and next, the matches they show.
    pub tv: Vec<TvChannel>,
    pub competitions: Vec<CompetitionCard>,
    /// Games with a reminder.
    pub reminders: Vec<String>,
    pub sources: Vec<SourceInfo>,
}

/// Live first, then by start.
fn by_phase(a: &Game, b: &Game) -> std::cmp::Ordering {
    let rank = |g: &Game| match g.phase {
        Phase::Live => 0,
        Phase::Pre => 1,
        Phase::Post => 2,
    };
    rank(a).cmp(&rank(b)).then(a.start.cmp(&b.start))
}

fn priority(g: &Game) -> (u8, Region, bool) {
    let mine = match g.mine.as_deref() {
        Some("gremio") => 0,
        Some("lakers") => 1,
        _ => 2,
    };
    (mine, g.region(), !g.televised())
}

/// Grêmio's games, and today's NBA and football, each with what ge and Lakers Brasil add.
async fn today_games(app: &AppHandle, x: &Extras, now: i64) -> (Vec<Game>, Vec<Game>, Vec<Game>) {
    let today = Day::slate(now);
    let nba_league = league("nba").expect("NBA is listed");
    let (gremio, nba, football) = futures_util::join!(
        gremio_games(app, x, now),
        games_on(app, nba_league, today),
        join_all(soccer_leagues().map(|l| games_on(app, l, today))),
    );
    let mut nba: Vec<Game> = nba
        .into_iter()
        .map(|mut g| {
            decorate(&mut g, x);
            g
        })
        .collect();
    nba.sort_by(|a, b| by_phase(a, b).then(priority(a).cmp(&priority(b))));
    let mut football: Vec<Game> = football
        .into_iter()
        .flatten()
        .map(|mut g| {
            decorate(&mut g, x);
            g
        })
        .collect();
    football.sort_by(|a, b| by_phase(a, b).then(priority(a).cmp(&priority(b))));
    (gremio, nba, football)
}

pub async fn home(app: &AppHandle) -> Result<Home> {
    refresh_due(app).await;
    refresh_tv(app).await;
    let now = now_ms();
    let x = extras(app);
    let today = Day::slate(now);

    let (mut gremio, mut nba, mut football) = today_games(app, &x, now).await;

    // What the sports channels show; a match on one of them is watched there first.
    let mut tv = tv_channels(app, now);
    {
        let all: Vec<&Game> = gremio.iter().chain(nba.iter()).chain(football.iter()).collect();
        match_tv(&mut tv, &all, now);
    }
    let on_tv = tv_watch(&tv, now);
    for g in gremio.iter_mut().chain(nba.iter_mut()).chain(football.iter_mut()) {
        with_tv(g, &on_tv);
    }
    for c in &mut tv {
        c.slots.retain(|s| s.end > now && s.start < now + 6 * HOUR);
    }
    tv.retain(|c| !c.slots.is_empty());

    let all: Vec<&Game> = gremio.iter().chain(nba.iter()).chain(football.iter()).collect();
    let mut live: Vec<&Game> = all.iter().copied().filter(|g| g.phase == Phase::Live).collect();
    live.sort_by_key(|g| priority(g));
    let upcoming_today = |g: &&Game| g.phase == Phase::Pre && Day::slate(g.start) == today && g.note.is_none();
    let gremio_next = gremio.iter().find(|g| g.phase != Phase::Post).cloned();
    let gremio_last = gremio.iter().rev().find(|g| g.phase == Phase::Post);
    let lakers_today = nba.iter().find(|g| g.mine.is_some() && g.phase == Phase::Pre);

    let featured = live
        .first()
        .copied()
        .or_else(|| gremio.iter().find(upcoming_today))
        .or(lakers_today)
        .or_else(|| nba.iter().filter(upcoming_today).find(|g| g.televised()))
        .or_else(|| football.iter().filter(upcoming_today).find(|g| g.televised()))
        .or(gremio_last)
        .cloned();

    // Beside the hero: the hero first, the other live games by priority, then Grêmio's next.
    let mut rail: Vec<Game> = featured.iter().cloned().collect();
    let mut add_rail = |g: &Game| {
        if rail.len() < 4 && !rail.iter().any(|x| x.id == g.id) {
            rail.push(g.clone());
        }
    };
    live.iter().take(3).for_each(|g| add_rail(g));
    if let Some(g) = &gremio_next {
        add_rail(g);
    }
    nba.iter().chain(football.iter()).filter(upcoming_today).filter(|g| g.televised()).for_each(add_rail);

    // The hero's scorers, stats and events.
    let hero = match featured.clone() {
        Some(g) if g.phase != Phase::Pre => game(app, g).await.ok(),
        _ => None,
    };

    let mut for_you: Vec<Game> = Vec::new();
    let mut add = |g: Option<&Game>| {
        if let Some(g) = g {
            if !for_you.iter().any(|x| x.id == g.id) {
                for_you.push(g.clone());
            }
        }
    };
    live.iter().for_each(|g| add(Some(*g)));
    add(gremio_next.as_ref());
    add(lakers_today);
    nba.iter().filter(upcoming_today).filter(|g| g.televised()).for_each(|g| add(Some(g)));
    football
        .iter()
        .filter(upcoming_today)
        .filter(|g| g.region() <= Region::SouthAmerica && g.televised())
        .for_each(|g| add(Some(g)));
    add(gremio_last);
    for_you.truncate(14);

    Ok(Home {
        featured,
        rail,
        hero,
        gremio_next,
        for_you,
        gremio,
        nba,
        football,
        tv,
        competitions: soccer_leagues()
            .map(|l| CompetitionCard {
                id: l.id.into(),
                name: l.name.into(),
                color: l.color.into(),
                channels: l.channels.into(),
            })
            .collect(),
        reminders: reminders(app),
        sources: sources(app),
    })
}

/// Grêmio's matches around now (the last two, the next six), for the calendar.
pub async fn gremio_schedule(app: &AppHandle) -> Vec<Game> {
    gremio_games(app, &extras(app), now_ms()).await
}

fn sources(app: &AppHandle) -> Vec<SourceInfo> {
    let st = state(app);
    let updated = |source: Source| -> Option<i64> {
        match source {
            Source::GeGremio => st.gremio.lock().unwrap().as_ref().map(|s| s.fetched_at),
            Source::GeAgenda => st.agenda.lock().unwrap().as_ref().map(|s| s.fetched_at),
            Source::NbaBr => st.slate.lock().unwrap().as_ref().map(|s| s.fetched_at),
            Source::TvGlobo => st.tv_globo.lock().unwrap().as_ref().map(|s| s.fetched_at),
            Source::TvEspn => st.tv_espn.lock().unwrap().as_ref().map(|s| s.fetched_at),
            Source::TvCaze => st.tv_caze.lock().unwrap().as_ref().map(|s| s.fetched_at),
        }
    };
    SCHEDULED
        .into_iter()
        .chain(TV_SOURCES)
        .map(|s| SourceInfo {
            name: s.name().to_string(),
            updated: updated(s),
            error: st.errors.lock().unwrap().get(&s).cloned(),
        })
        .collect()
}

// ---------- Live on TV ----------

/// The sports channels with their programme: Globoplay's, ESPN's, then CazéTV's streams.
fn tv_channels(app: &AppHandle, now: i64) -> Vec<TvChannel> {
    let mut out = tv_lineup(app, now);
    out.retain_mut(tv::sports_channel);
    out
}

/// Every channel with its programme, as last read: Globoplay's lineup, ESPN's, then CazéTV's
/// streams.
pub fn tv_lineup(app: &AppHandle, now: i64) -> Vec<TvChannel> {
    let st = state(app);
    let mut out: Vec<TvChannel> = st.tv_globo.lock().unwrap().as_ref().map(|s| s.data.clone()).unwrap_or_default();
    for c in &mut out {
        // The frame is a live picture: a new address each minute gets past the caches.
        if let Some(f) = c.frame.as_mut().filter(|f| f.contains("live.video.globo.com")) {
            *f = format!("{f}?t={}", now / MIN);
        }
        c.slots.retain(|s| s.end > now - 6 * HOUR);
    }
    let espn = st.tv_espn.lock().unwrap().as_ref().map(|s| s.data.clone()).unwrap_or_default();
    for c in &tv::ESPN {
        let mut slots = espn.iter().find(|(k, _)| *k == c.key).map(|(_, s)| s.clone()).unwrap_or_default();
        tv::trim(&mut slots, now);
        if !slots.is_empty() {
            out.push(tv::espn_channel(c, slots));
        }
    }
    let caze = st.tv_caze.lock().unwrap().as_ref().map(|s| s.data.clone()).unwrap_or_default();
    out.extend(tv::cazetv_channels(&caze, now));
    out
}

/// The match on TV is `g`: both sides named alike, and on the air near its start (a replay
/// up to a day later).
fn shows(g: &Game, s: &Slot) -> bool {
    let Some((a, b)) = &s.teams else { return false };
    let sides = (same_team(a, &g.home.name) && same_team(b, &g.away.name))
        || (same_team(a, &g.away.name) && same_team(b, &g.home.name));
    let close = match s.kind {
        tv::Kind::Replay => s.start >= g.start - HOUR && s.start <= g.start + 30 * HOUR,
        _ => (s.start - g.start).abs() <= 3 * HOUR,
    };
    sides && close
}

/// Fills in the ESPN game each match on TV shows, and marks the followed competitions.
fn match_tv(channels: &mut [TvChannel], games: &[&Game], now: i64) {
    for c in channels.iter_mut() {
        for s in c.slots.iter_mut() {
            if s.end > now - 30 * MIN && s.start < now + 12 * HOUR {
                s.game = games.iter().find(|g| shows(g, s)).map(|g| (*g).clone());
            }
            tv::mark_followed(s);
        }
    }
}

/// The live channels showing each game now or within two hours, by game id.
fn tv_watch(channels: &[TvChannel], now: i64) -> HashMap<String, Vec<Channel>> {
    let mut out: HashMap<String, Vec<Channel>> = HashMap::new();
    for c in channels {
        for s in &c.slots {
            let Some(g) = &s.game else { continue };
            if s.kind == tv::Kind::Replay || s.end <= now || s.start > now + 2 * HOUR {
                continue;
            }
            let list = out.entry(g.id.clone()).or_default();
            if !list.iter().any(|x| x.tv.as_deref() == Some(c.key.as_str())) {
                list.push(Channel::live(&c.name, &c.app, c.url.clone(), &c.key));
            }
        }
    }
    out
}

/// The live channels go first among a game's channels, in place of ge's names for them.
fn with_tv(g: &mut Game, on_tv: &HashMap<String, Vec<Channel>>) {
    let Some(list) = on_tv.get(&g.id) else { return };
    let names: Vec<String> = list.iter().map(|c| words(&c.name)).collect();
    g.channels.retain(|c| !names.contains(&words(&c.name)));
    for (i, c) in list.iter().enumerate() {
        g.channels.insert(i, c.clone());
    }
}

/// Every sports channel's programme, for the TV guide, with the games they show.
pub async fn tv(app: &AppHandle) -> Result<Vec<TvChannel>> {
    refresh_tv(app).await;
    let now = now_ms();
    let x = extras(app);
    let (gremio, nba, football) = today_games(app, &x, now).await;
    let all: Vec<&Game> = gremio.iter().chain(nba.iter()).chain(football.iter()).collect();
    let mut channels = tv_channels(app, now);
    match_tv(&mut channels, &all, now);
    if channels.is_empty() {
        let errors = state(app).errors.lock().unwrap();
        if let Some(e) = TV_SOURCES.iter().find_map(|s| errors.get(s)) {
            return Err(Error::msg(format!("The TV programme isn't loading: {e}")));
        }
    }
    Ok(channels)
}

/// Globoplay's lineup, ESPN's channels and CazéTV's streams, read again when they're due, for
/// the Live tab.
pub async fn lineup(app: &AppHandle) -> Vec<TvChannel> {
    refresh_tv(app).await;
    tv_lineup(app, now_ms())
}

/// Fills in the games `channels` show now and soon, and marks the followed competitions.
pub async fn with_games(app: &AppHandle, channels: &mut [TvChannel]) {
    let now = now_ms();
    let (gremio, nba, football) = today_games(app, &extras(app), now).await;
    let all: Vec<&Game> = gremio.iter().chain(nba.iter()).chain(football.iter()).collect();
    match_tv(channels, &all, now);
}

/// What went wrong the last time each part of the live programme was read.
pub fn tv_errors(app: &AppHandle) -> Vec<String> {
    let errors = state(app).errors.lock().unwrap();
    TV_SOURCES.iter().filter_map(|s| errors.get(s).map(|e| format!("{}: {e}", s.name()))).collect()
}

/// Sports on the Home's Live now row.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Live {
    /// The games live now that can be watched, by priority: Grêmio, the Lakers, then by region.
    pub games: Vec<Game>,
    /// The channels with a live event on, each with only that slot: the ones showing `games`
    /// (for their pictures) and any other live sport.
    pub channels: Vec<TvChannel>,
}

/// What's live now, for the Home's Live now row.
pub async fn live(app: &AppHandle) -> Result<Live> {
    refresh_due(app).await;
    refresh_tv(app).await;
    let now = now_ms();
    let x = extras(app);
    let (gremio, nba, football) = today_games(app, &x, now).await;
    let all: Vec<&Game> = gremio.iter().chain(nba.iter()).chain(football.iter()).collect();
    let mut tv = tv_channels(app, now);
    match_tv(&mut tv, &all, now);
    let on_tv = tv_watch(&tv, now);
    let mut games: Vec<Game> = Vec::new();
    for g in all.into_iter().filter(|g| g.phase == Phase::Live) {
        if !games.iter().any(|x| x.id == g.id) {
            let mut g = g.clone();
            with_tv(&mut g, &on_tv);
            games.push(g);
        }
    }
    games.retain(|g| g.channels.iter().any(Channel::watchable));
    games.sort_by_key(priority);
    games.truncate(LIVE_GAMES);
    let channels = tv
        .into_iter()
        .filter_map(|mut c| {
            let slot = c.on_at(now).filter(|s| s.kind == tv::Kind::Live)?.clone();
            c.slots = vec![slot];
            Some(c)
        })
        .collect();
    Ok(Live { games, channels })
}

/// The ESPN game a match on TV is, from the competition's scoreboard (or every followed one's).
async fn find_game(app: &AppHandle, s: &Slot) -> Option<Game> {
    s.teams.as_ref()?;
    let leagues: Vec<&'static League> = match s.competition.as_deref().and_then(tv::followed_league) {
        Some(l) => vec![l],
        None => LEAGUES.iter().collect(),
    };
    let day = Day::espn(s.start);
    for l in leagues {
        if let Some(g) = games_on(app, l, day).await.into_iter().find(|g| shows(g, s)) {
            return Some(g);
        }
    }
    None
}

/// Where a channel plays.
fn live_url(ch: &TvChannel) -> Result<String> {
    ch.url.clone().ok_or_else(|| Error::msg(format!("{} has nothing to play right now", ch.name)))
}

/// Whether two apps open the same kind of browser (identity, TV mode), so zapping between them
/// can stay in the page; the YouTube TV app's can't.
fn same_browser(app: &AppHandle, a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let cfg = app.state::<AppState>().config.read().unwrap().apps.clone();
    let look = |id: &str| cfg.iter().find(|e| e.id == id).map(|e| (e.nav, e.user_agent.clone()));
    matches!((look(a), look(b)), (Some(x), Some(y)) if x == y)
}

/// The channels the overlay zaps through, as last read.
fn session_channels(app: &AppHandle, lineup: Lineup, now: i64) -> Vec<TvChannel> {
    match lineup {
        Lineup::Sports => tv_channels(app, now),
        Lineup::All => crate::livetv::channels(app, now),
    }
}

/// Reads the overlay's channels again, the parts that are due.
async fn refresh_lineup(app: &AppHandle, lineup: Lineup) {
    refresh_tv(app).await;
    if lineup == Lineup::All {
        crate::livetv::refresh(app).await;
    }
}

/// Plays the live channel `key` with the live overlay, zapping through `lineup`.
pub async fn watch_tv(app: &AppHandle, key: &str, lineup: Lineup) -> Result<()> {
    refresh_lineup(app, lineup).await;
    let now = now_ms();
    let channels = session_channels(app, lineup, now);
    let ch = channels
        .iter()
        .find(|c| c.key == key)
        .ok_or_else(|| Error::msg("That channel's programme isn't loaded right now"))?;
    let url = live_url(ch)?;
    let overlay = state(app).saved.lock().unwrap().overlay;
    let init = live_view(&channels, key, lineup, None, None, &[], overlay, now);
    let script = format!("window.__tvlLiveInit = {init};\n{LIVE_SCRIPT}");
    launcher::launch_live(app, &ch.app, &url, script)?;
    let st = state(app);
    let gen = st.overlay_gen.fetch_add(1, Ordering::SeqCst) + 1;
    let wake = Arc::new(tokio::sync::Notify::new());
    *st.live.lock().unwrap() = Some(LiveSession {
        gen,
        app_id: ch.app.clone(),
        key: key.to_string(),
        lineup,
        wake: wake.clone(),
    });
    tauri::async_runtime::spawn(live_loop(app.clone(), gen, wake));
    Ok(())
}

/// The rail's heading over a channel: by what it is on the Live tab, by where it plays on Sports.
fn rail_section(c: &TvChannel, lineup: Lineup) -> &'static str {
    match (lineup, c.provider.as_str(), c.group.as_str()) {
        (Lineup::All, _, "news") => "NEWS",
        (Lineup::All, _, "tv") => "TV",
        (Lineup::All, ..) => "SPORTS",
        (_, "globo", _) => "GLOBOPLAY",
        (_, "espn", _) => "ESPN ON DISNEY+",
        _ => "CAZÉTV ON YOUTUBE",
    }
}

/// The stats the overlay's info bar shows, in its order.
const LIVE_STATS: [&str; 12] = [
    "Possession", "Shots", "Shots on target", "Corners", "Fouls", "Saves",
    "FG %", "3-pointers", "Rebounds", "Assists", "Turnovers", "Steals",
];

/// What the live overlay shows: the channels' now and next, the score bug, the match's stats,
/// and the user's teams' games on now (`following`).
#[allow(clippy::too_many_arguments)]
fn live_view(
    channels: &[TvChannel],
    key: &str,
    lineup: Lineup,
    game: Option<&Game>,
    detail: Option<&GameDetail>,
    following: &[Game],
    overlay: bool,
    now: i64,
) -> Value {
    let slot = |s: Option<&Slot>| {
        s.map(|s| json!({ "title": s.title, "sub": s.sub, "start": s.start, "end": s.end, "kind": s.kind }))
    };
    let list: Vec<Value> = channels
        .iter()
        .map(|c| {
            json!({
                "key": c.key, "name": c.name, "color": c.color, "provider": c.provider, "access": c.access,
                "free": c.free, "frame": c.frame, "section": rail_section(c, lineup), "viewers": c.viewers,
                "now": slot(c.on_at(now)), "next": slot(c.after(now)),
            })
        })
        .collect();
    // Basketball writes the away team first.
    let sides = |g: &Game| match g.sport {
        Sport::Basketball => (g.away.clone(), g.home.clone()),
        Sport::Soccer => (g.home.clone(), g.away.clone()),
    };
    let side = |t: &Team| json!({ "abbr": t.abbr, "color": t.color, "score": t.score });
    let bug = game.filter(|g| overlay && g.phase != Phase::Pre).map(|g| {
        let (a, b) = sides(g);
        json!({ "a": side(&a), "b": side(&b), "clock": g.status, "live": g.phase == Phase::Live })
    });
    let stats: Vec<Value> = detail
        .map(|d| {
            let list = d.soccer.as_ref().map(|s| &s.stats).or(d.nba.as_ref().map(|n| &n.stats));
            let away_first = d.game.sport == Sport::Basketball;
            LIVE_STATS
                .iter()
                .filter_map(|name| list?.iter().find(|s| s.name == *name))
                .take(6)
                .map(|s| {
                    let (a, b, av, bv) = if away_first {
                        (&s.away, &s.home, s.away_v, s.home_v)
                    } else {
                        (&s.home, &s.away, s.home_v, s.away_v)
                    };
                    json!({ "name": s.name, "a": a, "b": b, "av": av, "bv": bv })
                })
                .collect()
        })
        .unwrap_or_default();
    let teams = game.map(|g| {
        let (a, b) = sides(g);
        json!({ "a": side(&a), "b": side(&b) })
    });
    // The one on air is in the bug already.
    let following: Vec<Value> = following
        .iter()
        .filter(|g| overlay && game.is_none_or(|x| x.id != g.id))
        .map(|g| {
            let (a, b) = sides(g);
            let on = channels
                .iter()
                .find(|c| c.on_at(now).and_then(|s| s.game.as_ref()).is_some_and(|x| x.id == g.id))
                .map(|c| c.name.clone())
                .or_else(|| g.channels.first().map(|c| c.name.clone()));
            let sub = on.into_iter().chain([g.league_name.clone()]).collect::<Vec<_>>().join(" · ");
            json!({
                "a": side(&a), "b": side(&b), "clock": g.status,
                "title": format!("{} x {}", a.name, b.name), "sub": sub,
            })
        })
        .collect();
    json!({
        "current": key, "channels": list, "bug": bug, "stats": stats, "teams": teams,
        "hasMatch": game.is_some(), "following": following, "now": now,
    })
}

/// Sends the live overlay what's on, every few seconds and at once when it asks, while the
/// channel plays.
async fn live_loop(app: AppHandle, gen: u64, wake: Arc<tokio::sync::Notify>) {
    loop {
        let (app_id, key, lineup) = match state(&app).live.lock().unwrap().as_ref() {
            Some(s) if s.gen == gen => (s.app_id.clone(), s.key.clone(), s.lineup),
            _ => return,
        };
        if !launcher::is_running(&app, &app_id) {
            let mut live = state(&app).live.lock().unwrap();
            if live.as_ref().is_some_and(|s| s.gen == gen) {
                *live = None;
            }
            return;
        }
        refresh_lineup(&app, lineup).await;
        let now = now_ms();
        let mut channels = session_channels(&app, lineup, now);
        // Today's games are cached: a few seconds old at most while one is live.
        let (gremio, nba, football) = today_games(&app, &extras(&app), now).await;
        let all: Vec<&Game> = gremio.iter().chain(nba.iter()).chain(football.iter()).collect();
        match_tv(&mut channels, &all, now);
        let following: Vec<Game> = all.iter().filter(|g| g.mine.is_some() && g.phase == Phase::Live).map(|g| (*g).clone()).collect();
        let on = channels.iter().find(|c| c.key == key).and_then(|c| c.on_at(now)).cloned();
        let game = match &on {
            Some(s) if s.game.is_some() => s.game.clone(),
            Some(s) if s.teams.is_some() => find_game(&app, s).await,
            _ => None,
        };
        let detail = match game.clone() {
            Some(g) if g.phase != Phase::Pre => self::game(&app, g).await.ok(),
            _ => None,
        };
        let overlay = state(&app).saved.lock().unwrap().overlay;
        let view = live_view(
            &channels,
            &key,
            lineup,
            detail.as_ref().map(|d| &d.game).or(game.as_ref()),
            detail.as_ref(),
            &following,
            overlay,
            now,
        );
        cdp::send(&app, Cmd::Eval(format!("window.__tvlLive && window.__tvlLive.update({view})")));
        tokio::select! {
            _ = tokio::time::sleep(LIVE_EVERY) => {}
            _ = wake.notified() => {}
        }
    }
}

#[derive(Deserialize)]
struct LiveReport {
    kind: String,
    #[serde(default)]
    key: Option<String>,
}

/// A report from the live overlay (player/livetv.js) rather than watch.js.
pub fn is_live_report(text: &str) -> bool {
    text.contains("\"kind\":\"live-")
}

pub fn on_live_report(app: &AppHandle, text: &str) {
    let Ok(r) = serde_json::from_str::<LiveReport>(text) else {
        return;
    };
    match r.kind.as_str() {
        // A page loaded: it wants what's on.
        "live-hello" => {}
        "live-zap" => {
            if let Some(key) = r.key {
                zap(app, &key);
            }
        }
        "live-match" => open_match(app),
        _ => return,
    }
    if let Some(s) = state(app).live.lock().unwrap().as_ref() {
        s.wake.notify_one();
    }
}

/// Tunes the playing browser to channel `key`: in the page when it plays in the same kind of
/// browser, else in a new one (the YouTube TV app's identity doesn't suit the other sites).
fn zap(app: &AppHandle, key: &str) {
    let Some((running, lineup)) = state(app).live.lock().unwrap().as_ref().map(|s| (s.app_id.clone(), s.lineup)) else {
        return;
    };
    let channels = session_channels(app, lineup, now_ms());
    let Some(ch) = channels.iter().find(|c| c.key == key) else { return };
    let Ok(url) = live_url(ch) else { return };
    if same_browser(app, &running, &ch.app) {
        if let Some(s) = state(app).live.lock().unwrap().as_mut() {
            s.key = key.to_string();
        }
        cdp::send(app, Cmd::Eval(format!("location.href = {}", json!(url))));
        return;
    }
    let app = app.clone();
    let key = key.to_string();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = watch_tv(&app, &key, lineup).await {
            eprintln!("sports: zap to {key}: {e}");
        }
    });
}

/// Y on the overlay: the match on air, in the launcher's match center.
fn open_match(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some((key, lineup)) = state(&app).live.lock().unwrap().as_ref().map(|s| (s.key.clone(), s.lineup)) else { return };
        let now = now_ms();
        let on = session_channels(&app, lineup, now).into_iter().find(|c| c.key == key).and_then(|c| c.on_at(now).cloned());
        let Some(slot) = on else { return };
        let Some(mut g) = find_game(&app, &slot).await else { return };
        decorate(&mut g, &extras(&app));
        let _ = app.emit("sports-open-game", g);
        std::thread::spawn(move || launcher::close(&app));
    });
}

// ---------- One game ----------

#[derive(Debug, Clone, Serialize)]
pub struct GameDetail {
    pub game: Game,
    pub soccer: Option<SoccerDetail>,
    pub nba: Option<NbaDetail>,
    /// The team's next game, for a finished one of the user's teams.
    pub next: Option<Game>,
    pub reminder: bool,
    pub overlay: bool,
    /// The live channels showing it now or soon, each with that slot only.
    pub on_tv: Vec<TvChannel>,
}

pub async fn game(app: &AppHandle, game: Game) -> Result<GameDetail> {
    let x = extras(app);
    let now = now_ms();
    let (game, soccer, nba) = match league(&game.league).filter(|_| !game.id.starts_with("ge-")) {
        Some(l) => {
            let v = get_json(app, &espn::summary_url(l, &game.id), summary_ttl).await?;
            let comp = &v["header"]["competitions"][0];
            let event = json!({
                "id": game.id,
                "date": comp["date"],
                "status": comp["status"],
                "season": v["header"]["season"],
                "competitions": [comp],
            });
            let mut fresh = espn::parse_event(l, &event).unwrap_or_else(|| game.clone());
            fresh.channels = game.channels.clone();
            fresh.stage = fresh.stage.or(game.stage.clone());
            decorate(&mut fresh, &x);
            if fresh.channels.is_empty() {
                fresh.channels = game.channels.clone();
            }
            fresh.ge_video = fresh.ge_video.or(game.ge_video.clone());
            let (s, n) = match l.sport {
                Sport::Soccer => (Some(espn::parse_soccer(&v, &fresh)), None),
                Sport::Basketball => (None, Some(espn::parse_nba(&v, &fresh))),
            };
            (fresh, s, n)
        }
        None => (game, None, None),
    };
    let next = match game.mine.as_deref() {
        Some("gremio") => x
            .gremio
            .iter()
            .filter(|m| m.start.is_some_and(|t| t > game.start + 3 * HOUR) && m.score.is_none())
            .min_by_key(|m| m.start)
            .map(|m| from_ge(m, now)),
        _ => None,
    };
    // Read when the channels' programme is already in (the Sports tab reads it).
    let mut on_tv = tv_channels(app, now);
    match_tv(&mut on_tv, &[&game], now);
    for c in &mut on_tv {
        c.slots.retain(|s| s.game.is_some() && s.kind != tv::Kind::Replay && s.end > now);
    }
    on_tv.retain(|c| !c.slots.is_empty());
    let mut game = game;
    with_tv(&mut game, &tv_watch(&on_tv, now));
    let saved = state(app).saved.lock().unwrap().clone();
    Ok(GameDetail {
        reminder: saved.reminders.iter().any(|r| r.id == game.id),
        overlay: saved.overlay,
        on_tv,
        game,
        soccer,
        nba,
        next,
    })
}

// ---------- Competitions ----------

#[derive(Debug, Clone, Serialize)]
pub struct Tie {
    pub stage: String,
    pub home: Team,
    pub away: Team,
    /// Oldest first.
    pub legs: Vec<Game>,
    /// Goals over both legs, `home` first.
    pub aggregate: Option<(i64, i64)>,
    pub mine: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchDay {
    /// 2026-10-26.
    pub day: String,
    pub games: Vec<Game>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RoadStep {
    pub stage: String,
    pub opponent: String,
    /// "0–0 away · 3–1 home", or the dates when not played yet.
    pub legs: String,
    pub result: String,
    pub next: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GremioIn {
    /// "15th · 28 pts · 7W 7D 10L · GD −5".
    pub summary: Option<String>,
    pub next: Option<Game>,
    /// Knockout stages, latest first.
    pub road: Vec<RoadStep>,
    /// The next few in this competition (the run-in), the live one first.
    pub fixtures: Vec<Game>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Competition {
    pub id: String,
    pub name: String,
    pub color: String,
    pub channels: String,
    pub season: Option<String>,
    pub tables: Vec<Table>,
    /// "ESPN" or "ge".
    pub table_source: Option<String>,
    pub ties: Vec<Tie>,
    pub stages: Vec<Stage>,
    pub days: Vec<MatchDay>,
    pub gremio: Option<GremioIn>,
    /// A league's current round, of how many.
    pub round: Option<i64>,
    pub rounds: Option<i64>,
    /// That round's games.
    pub round_games: Vec<Game>,
}

fn ordinal(n: i64) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// ge's Brasileirão table, read at most every ten minutes.
async fn ge_rows(app: &AppHandle) -> Option<Vec<ge::GeRow>> {
    let now = now_ms();
    if let Some((at, rows)) = state(app).ge_table.lock().unwrap().as_ref() {
        if now - at < 10 * MIN {
            return Some(rows.clone());
        }
    }
    let html = get_text(app, ge::TABLE_URL).await.ok()?;
    let rows = ge::parse_table(&html)?;
    *state(app).ge_table.lock().unwrap() = Some((now, rows.clone()));
    Some(rows)
}

/// The Brasileirão table from ge, when ESPN's is missing.
async fn ge_table(app: &AppHandle) -> Option<Table> {
    let rows = ge_rows(app).await?;
    Some(Table {
        name: None,
        rows: rows
            .into_iter()
            .map(|r| espn::Row {
                rank: r.rank,
                team: Team {
                    id: if r.abbr == "GRE" { GREMIO.to_string() } else { format!("ge-{}", r.abbr) },
                    name: r.name,
                    abbr: r.abbr,
                    logo: r.badge,
                    color: "#2a2a2e".into(),
                    score: None,
                    winner: false,
                },
                played: r.played.to_string(),
                wins: r.wins.to_string(),
                draws: r.draws.to_string(),
                losses: r.losses.to_string(),
                goal_diff: if r.goal_diff > 0 { format!("+{}", r.goal_diff) } else { r.goal_diff.to_string() },
                points: r.points.to_string(),
                zone: None,
                form: r.form,
                live: None,
            })
            .collect(),
        zones: Vec::new(),
    })
}

/// The Brasileirão's zones, which ESPN leaves out: Libertadores and relegation.
fn brasileirao_zones(t: &mut Table) {
    if !t.zones.is_empty() {
        return;
    }
    let (lib, sula, down) = ("#4c8dff", "#20c997", "#eb0400");
    let n = t.rows.len() as i64;
    for r in &mut t.rows {
        r.zone = if r.rank <= 6 {
            Some(lib.into())
        } else if r.rank <= 12 {
            Some(sula.into())
        } else if r.rank > n - 4 {
            Some(down.into())
        } else {
            None
        };
    }
    t.zones = vec![
        Zone { color: lib.into(), label: "Libertadores".into() },
        Zone { color: sula.into(), label: "Sul-Americana".into() },
        Zone { color: down.into(), label: "Relegation".into() },
    ];
}

/// A table number as written: "+5", "−3", "28".
fn table_number(s: &str) -> i64 {
    s.replace('+', "").replace('−', "-").trim().parse().unwrap_or(0)
}

/// Each team playing now, moved by its live score: points, wins and goal difference as if the
/// game ended so, ranked again against the table as it stands.
fn live_moves(t: &mut Table, live: &[&Game]) {
    let result = |id: &str| -> Option<(i64, i64, i64)> {
        let g = live.iter().find(|g| g.home.id == id || g.away.id == id)?;
        let goals = |x: &Team| x.score.as_deref().and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
        let (us, them) = if g.home.id == id { (goals(&g.home), goals(&g.away)) } else { (goals(&g.away), goals(&g.home)) };
        Some(match us.cmp(&them) {
            std::cmp::Ordering::Greater => (3, 1, us - them),
            std::cmp::Ordering::Equal => (1, 0, 0),
            std::cmp::Ordering::Less => (0, 0, us - them),
        })
    };
    let place = |key: &dyn Fn(&espn::Row) -> (i64, i64, i64)| -> HashMap<String, i64> {
        let mut list: Vec<(String, (i64, i64, i64))> = t.rows.iter().map(|r| (r.team.id.clone(), key(r))).collect();
        list.sort_by_key(|x| std::cmp::Reverse(x.1));
        list.into_iter().enumerate().map(|(i, (id, _))| (id, i as i64)).collect()
    };
    let base = |r: &espn::Row| (table_number(&r.points), table_number(&r.wins), table_number(&r.goal_diff));
    let before = place(&base);
    let after = place(&|r: &espn::Row| {
        let (p, w, d) = base(r);
        let (dp, dw, dd) = result(&r.team.id).unwrap_or((0, 0, 0));
        (p + dp, w + dw, d + dd)
    });
    for r in &mut t.rows {
        if result(&r.team.id).is_some() {
            r.live = Some(before[&r.team.id] - after[&r.team.id]);
        }
    }
}

fn road(matches: &[&GeMatch], now: i64) -> Vec<RoadStep> {
    let mut steps: Vec<(String, Vec<&GeMatch>)> = Vec::new();
    for m in matches {
        let stage = ge::phase_name(&m.phase);
        if stage.is_empty() || stage == "Group stage" {
            continue;
        }
        match steps.iter_mut().find(|(s, _)| *s == stage) {
            Some((_, ms)) => ms.push(m),
            None => steps.push((stage, vec![m])),
        }
    }
    let mut out: Vec<RoadStep> = steps
        .into_iter()
        .map(|(stage, ms)| {
            let home = |m: &GeMatch| m.home.id == ge::GREMIO_ID;
            let opponent = ms
                .first()
                .map(|m| if home(m) { m.away.name.clone() } else { m.home.name.clone() })
                .unwrap_or_default();
            let played = ms.iter().all(|m| m.score.is_some() && !m.live);
            let legs = ms
                .iter()
                .map(|m| {
                    let side = if home(m) { "home" } else { "away" };
                    match m.score {
                        Some((h, a)) if !m.live => {
                            let (us, them) = if home(m) { (h, a) } else { (a, h) };
                            format!("{us}–{them} {side}")
                        }
                        _ => format!("{} {} {side}", MONTHS[(m.day.m - 1) as usize], m.day.d),
                    }
                })
                .collect::<Vec<_>>()
                .join(" · ");
            let result = if played {
                let (us, them) = ms.iter().fold((0, 0), |(u, t), m| {
                    let (h, a) = m.score.unwrap_or_default();
                    if home(m) { (u + h, t + a) } else { (u + a, t + h) }
                });
                let pens = ms.last().and_then(|m| {
                    m.penalties.map(|(h, a)| if home(m) { (h, a) } else { (a, h) })
                });
                match (us.cmp(&them), pens) {
                    (std::cmp::Ordering::Equal, Some((p, q))) if p > q => format!("Won {p}–{q} on penalties"),
                    (std::cmp::Ordering::Equal, Some((p, q))) => format!("Lost {p}–{q} on penalties"),
                    (std::cmp::Ordering::Greater, _) => format!("Won {us}–{them}"),
                    (std::cmp::Ordering::Less, _) => format!("Lost {us}–{them}"),
                    _ => format!("Drew {us}–{them}"),
                }
            } else {
                "Up next".to_string()
            };
            let next = !played && ms.iter().any(|m| m.start.is_none_or(|t| t + 3 * HOUR > now));
            RoadStep { stage, opponent, legs, result, next }
        })
        .collect();
    out.reverse();
    out
}

pub async fn competition(app: &AppHandle, id: &str) -> Result<Competition> {
    let l = league(id).ok_or_else(|| Error::msg(format!("unknown competition '{id}'")))?;
    let now = now_ms();
    let x = extras(app);
    let today = Day::slate(now);

    let current = scoreboard(app, l, None).await.ok();
    let stages = current.as_deref().map(|v| espn::stages(v, now)).unwrap_or_default();
    let season = current
        .as_deref()
        .and_then(|v| v["leagues"][0]["season"]["year"].as_i64())
        .map(|y| y.to_string());
    let cup = stages.len() > 1;
    let window: Vec<Day> = if cup { (-4..=10).map(|d| today.plus(d)).collect() } else { (-3..7).map(|d| today.plus(d)).collect() };

    let standings_url = espn::standings_url(l);
    let (standings, boards) = futures_util::join!(
        get_json(app, &standings_url, slow_ttl),
        join_all(window.iter().map(|d| scoreboard(app, l, Some(*d)))),
    );
    let mut tables = standings.map(|v| espn::parse_standings(&v)).unwrap_or_default();
    let mut table_source = (!tables.is_empty()).then(|| "ESPN".to_string());
    if tables.is_empty() && l.id == "bra.1" {
        if let Some(t) = ge_table(app).await {
            tables.push(t);
            table_source = Some("ge".into());
        }
    }
    if l.id == "bra.1" {
        tables.iter_mut().for_each(brasileirao_zones);
        // ESPN has no form guide: ge's last five.
        if let Some(rows) = ge_rows(app).await {
            for r in tables.iter_mut().flat_map(|t| t.rows.iter_mut()).filter(|r| r.form.is_empty()) {
                if let Some(g) = rows.iter().find(|g| g.abbr == r.team.abbr || same_team(&g.name, &r.team.name)) {
                    r.form = g.form.clone();
                }
            }
        }
    }

    // Every event in the window, plus the current matchday's.
    let mut raw: Vec<Value> = Vec::new();
    for v in boards.into_iter().flatten().chain(current.clone()) {
        for e in v["events"].as_array().into_iter().flatten() {
            if !raw.iter().any(|r| r["id"] == e["id"]) {
                raw.push(e.clone());
            }
        }
    }
    let mut games: Vec<(Game, Option<espn::TieInfo>)> = raw
        .iter()
        .filter_map(|e| {
            let mut g = espn::parse_event(l, e)?;
            decorate(&mut g, &x);
            Some((g, espn::tie_info(e)))
        })
        .collect();
    games.sort_by_key(|(g, _)| g.start);

    let mut ties: Vec<Tie> = Vec::new();
    for (g, info) in &games {
        let Some(info) = info else { continue };
        let pair = |t: &Tie| (t.home.id == g.home.id && t.away.id == g.away.id) || (t.home.id == g.away.id && t.away.id == g.home.id);
        match ties.iter_mut().find(|t| t.stage == info.title && pair(t)) {
            Some(t) => t.legs.push(g.clone()),
            None => ties.push(Tie {
                stage: info.title.clone(),
                home: Team { score: None, ..g.home.clone() },
                away: Team { score: None, ..g.away.clone() },
                legs: vec![g.clone()],
                aggregate: None,
                mine: g.has(GREMIO),
            }),
        }
        if let Some(t) = ties.iter_mut().find(|t| t.stage == info.title && pair(t)) {
            if let (Some(h), Some(a)) = (info.aggregate.get(&t.home.id), info.aggregate.get(&t.away.id)) {
                if g.phase != Phase::Pre {
                    t.aggregate = Some((*h, *a));
                }
            }
        }
    }
    for t in &mut ties {
        if t.aggregate.is_none() && t.legs.iter().any(|g| g.phase != Phase::Pre) {
            let goals = |id: &str| -> i64 {
                t.legs
                    .iter()
                    .filter(|g| g.phase != Phase::Pre)
                    .map(|g| {
                        let s = if g.home.id == id { &g.home.score } else { &g.away.score };
                        s.as_deref().and_then(|x| x.parse::<i64>().ok()).unwrap_or(0)
                    })
                    .sum()
            };
            t.aggregate = Some((goals(&t.home.id), goals(&t.away.id)));
        }
    }
    ties.sort_by_key(|t| !t.mine);

    let all_games: Vec<Game> = games.iter().map(|(g, _)| g.clone()).collect();
    let live: Vec<&Game> = all_games.iter().filter(|g| g.phase == Phase::Live).collect();
    if !live.is_empty() {
        tables.iter_mut().for_each(|t| live_moves(t, &live));
    }
    // The round (ge's number on the games it knows), and its games.
    let round_of = |g: &Game| g.stage.as_deref().and_then(|s| s.strip_prefix("Round ")).and_then(|n| n.parse::<i64>().ok());
    let round = if cup {
        None
    } else {
        let near: Vec<i64> = all_games
            .iter()
            .filter(|g| (g.start - now).abs() < 4 * DAY)
            .filter_map(round_of)
            .collect();
        let mut counts: Vec<(i64, usize)> = Vec::new();
        for n in near {
            match counts.iter_mut().find(|(r, _)| *r == n) {
                Some((_, c)) => *c += 1,
                None => counts.push((n, 1)),
            }
        }
        // The most games; of two as many, the later round.
        counts.into_iter().max_by_key(|(r, c)| (*c, *r)).map(|(r, _)| r)
    };
    let rounds = (!cup && tables.len() == 1 && tables[0].rows.len() >= 10).then(|| (tables[0].rows.len() as i64 - 1) * 2);
    let round_games: Vec<Game> = match round {
        Some(n) => {
            let listed: Vec<&Game> = all_games.iter().filter(|g| round_of(g) == Some(n)).collect();
            let (from, to) = (
                listed.iter().map(|g| g.start).min().unwrap_or(now),
                listed.iter().map(|g| g.start).max().unwrap_or(now),
            );
            // Games ge didn't number, played among the round's.
            all_games
                .iter()
                .filter(|g| round_of(g) == Some(n) || (g.stage.is_none() && g.start >= from && g.start <= to))
                .cloned()
                .collect()
        }
        None => Vec::new(),
    };

    let mut days: Vec<MatchDay> = Vec::new();
    for (g, _) in games {
        if Day::slate(g.start) < today.plus(-1) && !cup {
            continue;
        }
        let day = Day::brt(g.start).iso();
        match days.iter_mut().find(|d| d.day == day) {
            Some(d) => d.games.push(g),
            None => days.push(MatchDay { day, games: vec![g] }),
        }
    }

    let mine: Vec<&GeMatch> = {
        let mut v: Vec<&GeMatch> = x
            .gremio
            .iter()
            .filter(|m| league_for_ge(&m.championship).is_some_and(|ml| ml.id == l.id))
            .collect();
        v.sort_by_key(|m| m.start.unwrap_or_else(|| m.day.brt_ms(12, 0)));
        v
    };
    let summary = tables.iter().flat_map(|t| t.rows.iter()).find(|r| r.team.id == GREMIO || r.team.abbr == "GRE").map(|r| {
        let gd = r.goal_diff.replace('-', "−");
        format!("{} · {} pts · {}W {}D {}L · GD {gd}", ordinal(r.rank), r.points, r.wins, r.draws, r.losses)
    });
    let next = mine
        .iter()
        .find(|m| m.score.is_none() || m.live)
        .map(|m| from_ge(m, now));
    // ESPN's live score where it has the game, ge's otherwise.
    let fixtures: Vec<Game> = mine
        .iter()
        .filter(|m| m.score.is_none() || m.live)
        .take(4)
        .map(|m| all_games.iter().find(|g| ge_is(m, g)).cloned().unwrap_or_else(|| from_ge(m, now)))
        .collect();
    let gremio = (!mine.is_empty() || summary.is_some()).then(|| GremioIn {
        summary,
        next,
        road: road(&mine, now),
        fixtures,
    });

    Ok(Competition {
        id: l.id.into(),
        name: l.name.into(),
        color: l.color.into(),
        channels: l.channels.into(),
        season,
        tables,
        table_source,
        ties,
        stages,
        days,
        gremio,
        round,
        rounds,
        round_games,
    })
}

// ---------- Watching ----------

/// Opens `channel`: its app at the deep link, or a page in the launcher's browser. With the
/// overlay on and a game that's on (or about to be), its score shows in a corner of the page.
pub async fn watch(app: &AppHandle, channel: Channel, game: Option<Game>) -> Result<()> {
    // A channel with its programme known plays with the live overlay instead.
    if let Some(key) = &channel.tv {
        return watch_tv(app, key, Lineup::Sports).await;
    }
    let target = match (&channel.app, &channel.url) {
        (Some(id), Some(url)) => match launcher::launch_web(app, id, url, None) {
            Ok(()) => id.clone(),
            // The app was taken out of apps.toml: the plain page still works.
            Err(_) => {
                launcher::open_page(app, url, WATCH_PAGE)?;
                WATCH_PAGE.to_string()
            }
        },
        (Some(id), None) => {
            launcher::launch(app, id, None, None)?;
            id.clone()
        }
        (None, Some(url)) => {
            launcher::open_page(app, url, WATCH_PAGE)?;
            WATCH_PAGE.to_string()
        }
        (None, None) => return Err(Error::msg(format!("{} can't be opened from the launcher", channel.name))),
    };
    let st = state(app);
    let gen = st.overlay_gen.fetch_add(1, Ordering::SeqCst) + 1;
    let on = st.saved.lock().unwrap().overlay;
    if let Some(g) = game.filter(|g| on && g.phase != Phase::Post && !g.id.starts_with("ge-")) {
        tauri::async_runtime::spawn(overlay_loop(app.clone(), gen, target, g));
    }
    Ok(())
}

/// The overlay's script: a small score box in the page's top right corner, also over
/// a full-screen video.
fn overlay_script(g: Option<&Game>) -> String {
    let text = g.filter(|g| g.phase != Phase::Pre).map(|g| {
        let score = |t: &Team| t.score.clone().unwrap_or_default();
        let (first, second) = match g.sport {
            Sport::Basketball => (&g.away, &g.home),
            Sport::Soccer => (&g.home, &g.away),
        };
        format!("{} {}  –  {} {}   ·   {}", first.abbr, score(first), score(second), second.abbr, g.status)
    });
    format!(
        r#"(() => {{
  const text = {};
  let el = document.getElementById('__tvl_score');
  if (text === null) {{ if (el) el.remove(); return; }}
  if (!el) {{
    el = document.createElement('div');
    el.id = '__tvl_score';
    el.style.cssText = 'position:fixed;top:32px;right:32px;z-index:2147483647;padding:12px 22px;border-radius:999px;background:rgba(12,12,20,.82);color:#f2f2f3;font:600 24px/1.2 Poppins,Segoe UI,sans-serif;letter-spacing:.02em;pointer-events:none;white-space:pre;box-shadow:0 12px 32px rgba(0,0,0,.45)';
  }}
  const host = document.fullscreenElement || document.body || document.documentElement;
  if (el.parentNode !== host) host.appendChild(el);
  el.textContent = text;
}})()"#,
        serde_json::to_string(&text).unwrap_or_else(|_| "null".into())
    )
}

async fn overlay_loop(app: AppHandle, gen: u64, target: String, game: Game) {
    let Some(l) = league(&game.league) else { return };
    // Give the page time to load.
    tokio::time::sleep(Duration::from_secs(8)).await;
    loop {
        if state(&app).overlay_gen.load(Ordering::SeqCst) != gen || !launcher::is_running(&app, &target) {
            return;
        }
        if !state(&app).saved.lock().unwrap().overlay {
            cdp::send(&app, Cmd::Eval(overlay_script(None)));
            return;
        }
        let fresh = scoreboard(&app, l, Some(Day::espn(game.start)))
            .await
            .ok()
            .and_then(|v| espn::parse_events(l, &v).into_iter().find(|g| g.id == game.id));
        if let Some(g) = &fresh {
            cdp::send(&app, Cmd::Eval(overlay_script(Some(g))));
        }
        tokio::time::sleep(OVERLAY_EVERY).await;
    }
}

pub fn set_overlay(app: &AppHandle, on: bool) -> Result<()> {
    let st = state(app);
    let mut saved = st.saved.lock().unwrap();
    saved.overlay = on;
    st.save_settings(&saved)
}

// ---------- Reminders ----------

/// Ids of the games with a reminder.
pub fn reminders(app: &AppHandle) -> Vec<String> {
    state(app).saved.lock().unwrap().reminders.iter().map(|r| r.id.clone()).collect()
}

/// Turns the reminder for `game` on or off; resolves to whether it's on now.
pub fn remind(app: &AppHandle, game: &Game, on: bool) -> Result<bool> {
    let st = state(app);
    let mut saved = st.saved.lock().unwrap();
    saved.reminders.retain(|r| r.id != game.id);
    if on {
        let channel = game.channels.first().map(|c| format!(" · {}", c.name)).unwrap_or_default();
        saved.reminders.push(Reminder {
            id: game.id.clone(),
            at: game.start,
            text: format!("{} x {} starts in 15 minutes{channel}", game.home.name, game.away.name),
        });
    }
    st.save_settings(&saved)?;
    Ok(on)
}

fn check_reminders(app: &AppHandle) {
    let now = now_ms();
    let st = state(app);
    let mut saved = st.saved.lock().unwrap();
    let (due, keep): (Vec<Reminder>, Vec<Reminder>) =
        saved.reminders.drain(..).partition(|r| now >= r.at - REMIND_BEFORE);
    saved.reminders = keep;
    if due.is_empty() {
        return;
    }
    for r in due.iter().filter(|r| now < r.at + 30 * MIN) {
        let _ = app.emit("sports-reminder", r.text.clone());
    }
    let _ = st.save_settings(&saved);
}

// ---------- Highlights ----------

#[derive(Debug, Clone, Serialize)]
pub struct Highlights {
    /// Played inside the launcher, best first.
    pub youtube: Vec<youtube::Video>,
    /// ge's video, opened on Globoplay.
    pub globoplay: Option<String>,
    /// Its picture.
    pub globoplay_thumb: Option<String>,
}

/// A Globoplay video's picture: globoplay.globo.com/v/13830630/ → its frame on glbimg.
fn globoplay_thumb(url: &str) -> Option<String> {
    let id = url.trim_end_matches('/').rsplit('/').next()?;
    id.chars().all(|c| c.is_ascii_digit()).then(|| format!("https://s04.video.glbimg.com/x720/{id}.jpg"))
}

pub async fn highlights(app: &AppHandle, game: &Game) -> Result<Highlights> {
    let now = now_ms();
    let st = state(app);
    let cached = st.highlights.lock().unwrap().get(&game.id).cloned();
    let youtube = match cached {
        Some((at, v)) if !v.is_empty() || now - at < HIGHLIGHTS_RETRY => v,
        _ => {
            let found = youtube::highlights(&http(app), game, now).await.unwrap_or_else(|e| {
                eprintln!("sports: highlights search: {e}");
                Vec::new()
            });
            st.highlights.lock().unwrap().insert(game.id.clone(), (now, found.clone()));
            found
        }
    };
    let x = extras(app);
    let globoplay = game.ge_video.clone().or_else(|| {
        x.gremio.iter().chain(x.agenda.iter()).find(|m| ge_is(m, game)).and_then(|m| m.video.clone())
    });
    let globoplay_thumb = globoplay.as_deref().and_then(globoplay_thumb);
    Ok(Highlights { youtube, globoplay, globoplay_thumb })
}

// ---------- Background ----------

/// Reads the scheduled sources when they're due (ge weekly and daily, Lakers Brasil every
/// morning) and sends the reminders.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            refresh_due(&app).await;
            check_reminders(&app);
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sample_game(home: &str, away: &str, hs: &str, as_: &str) -> Game {
        let team = |name: &str, score: &str| Team {
            id: name.into(),
            name: name.into(),
            abbr: abbr(name),
            logo: None,
            color: "#2a2a2e".into(),
            score: Some(score.into()),
            winner: false,
        };
        Game {
            id: "1".into(),
            league: "bra.1".into(),
            league_name: "Brasileirão".into(),
            sport: Sport::Soccer,
            start: crate::photos::rfc3339_ms("2026-10-18T19:00Z").unwrap(),
            phase: Phase::Post,
            status: "Full time".into(),
            progress: 1.0,
            home: team(home, hs),
            away: team(away, as_),
            venue: None,
            stage: None,
            channels: Vec::new(),
            mine: None,
            aggregate: None,
            ge_video: None,
            note: None,
        }
    }

    #[test]
    fn team_names_across_sources() {
        assert!(same_team("Vasco", "Vasco da Gama"));
        assert!(same_team("Bayern de Munique", "Bayern Munich"));
        assert!(same_team("Atlético-MG", "Atlético-MG"));
        assert!(same_team("Bragantino", "Red Bull Bragantino"));
        assert!(!same_team("Atlético-MG", "Athletico-PR"));
        assert!(!same_team("Real Madrid", "Real Sociedad"));
        assert_eq!(words("GRÊMIO 2 X 4 CRUZEIRO | 31ª RODADA"), "gremio 2 x 4 cruzeiro 31a rodada");
    }

    #[test]
    fn ge_names_map_to_leagues() {
        assert_eq!(league_for_ge("Campeonato Brasileiro").map(|l| l.id), Some("bra.1"));
        assert_eq!(league_for_ge("Taça Conmebol Libertadores").map(|l| l.id), Some("conmebol.libertadores"));
        assert_eq!(league_for_ge("LALIGA").map(|l| l.id), Some("esp.1"));
        assert!(league_for_ge("Campeonato Saudita").is_none());
    }

    #[test]
    fn nba_channels_from_the_daily_list() {
        let mut g = sample_game("Celtics", "Knicks", "0", "0");
        g.sport = Sport::Basketball;
        g.league = "nba".into();
        g.home.abbr = "BOS".into();
        g.away.abbr = "NY".into();
        g.start = crate::photos::rfc3339_ms("2026-10-23T23:00Z").unwrap();
        let slate = Slate {
            day: Day { y: 2026, m: 10, d: 23 },
            games: vec![nba_br::BrGame {
                hour: 20,
                minute: 0,
                away: "nyk".into(),
                home: "bos".into(),
                away_name: "Knicks".into(),
                home_name: "Celtics".into(),
                channel: "Prime".into(),
            }],
        };
        let x = Extras { gremio: vec![], agenda: vec![], slate: Some(slate) };
        decorate(&mut g, &x);
        assert_eq!(g.channels[0].name, "Prime Video");
        // Not on the list: League Pass.
        let mut other = g.clone();
        other.home.abbr = "MIA".into();
        other.home.name = "Heat".into();
        decorate(&mut other, &x);
        assert_eq!(other.channels[0].name, "League Pass");
    }

    #[test]
    fn overlay_text() {
        let mut g = sample_game("Grêmio", "Cruzeiro", "1", "0");
        g.phase = Phase::Live;
        g.status = "67'".into();
        let js = overlay_script(Some(&g));
        assert!(js.contains(r#""GRE 1  –  0 CRU   ·   67'""#), "{js}");
        g.phase = Phase::Pre;
        assert!(overlay_script(Some(&g)).contains("const text = null;"));
    }

    #[test]
    fn matches_on_tv_get_watched_there() {
        let mut g = sample_game("Coritiba", "Grêmio", "0", "1");
        g.id = "401".into();
        g.phase = Phase::Live;
        g.channels = vec![channels::from_ge("Premiere", "Assine", None, &[]).unwrap()];
        let now = g.start + 67 * MIN;
        let slot = |title: &str, teams: Option<(&str, &str)>, kind: tv::Kind, start: i64| Slot {
            start,
            end: start + 2 * HOUR,
            title: title.into(),
            sub: String::new(),
            kind,
            teams: teams.map(|(a, b)| (a.to_string(), b.to_string())),
            competition: Some("Campeonato Brasileiro de Futebol".into()),
            desc: None,
            image: None,
            game: None,
            followed: false,
        };
        let channel = |key: &str, name: &str, slots: Vec<Slot>| TvChannel {
            key: key.into(),
            name: name.into(),
            color: "#2F9E44".into(),
            group: "premiere".into(),
            provider: "globo".into(),
            access: "Premiere".into(),
            free: false,
            frame: None,
            app: "globoplay".into(),
            url: Some(format!("https://globoplay.globo.com/{key}/ao-vivo/1/")),
            slots,
            viewers: None,
        };
        let mut tv = vec![
            channel("premiere", "Premiere", vec![slot("Coritiba x Grêmio", Some(("Coritiba", "Grêmio")), tv::Kind::Live, g.start - 5 * MIN)]),
            // Last week's meeting, again: not this game.
            channel("sportv", "sportv", vec![slot("Grêmio x Coritiba", Some(("Grêmio", "Coritiba")), tv::Kind::Replay, g.start - 7 * DAY)]),
            channel("ge-tv", "ge tv", vec![slot("Rodada ge", None, tv::Kind::Air, g.start)]),
        ];
        match_tv(&mut tv, &[&g], now);
        assert_eq!(tv[0].slots[0].game.as_ref().map(|x| x.id.as_str()), Some("401"));
        assert!(tv[0].slots[0].followed);
        assert!(tv[1].slots[0].game.is_none());
        assert!(tv[2].slots[0].game.is_none());

        with_tv(&mut g, &tv_watch(&tv, now));
        // The live channel first, in place of ge's generic Premiere.
        assert_eq!(g.channels.len(), 1);
        assert_eq!(g.channels[0].tv.as_deref(), Some("premiere"));
        assert_eq!(g.channels[0].url.as_deref(), Some("https://globoplay.globo.com/premiere/ao-vivo/1/"));
    }

    #[test]
    fn live_table_moves() {
        let row = |id: &str, rank: i64, pts: i64, wins: i64, gd: i64| espn::Row {
            rank,
            team: Team { id: id.into(), name: id.into(), abbr: id.into(), logo: None, color: "#000".into(), score: None, winner: false },
            played: "31".into(),
            wins: wins.to_string(),
            draws: "0".into(),
            losses: "0".into(),
            goal_diff: if gd > 0 { format!("+{gd}") } else { gd.to_string() },
            points: pts.to_string(),
            zone: None,
            form: String::new(),
            live: None,
        };
        let mut t = Table { name: None, rows: vec![row("A", 1, 40, 12, 10), row("B", 2, 39, 12, 5), row("C", 3, 38, 11, -2)], zones: vec![] };
        let mut g = sample_game("C", "A", "2", "0");
        g.home.id = "C".into();
        g.away.id = "A".into();
        g.phase = Phase::Live;
        live_moves(&mut t, &[&g]);
        // C on 41 goes top, A (40) drops to second, B isn't playing.
        assert_eq!(t.rows[2].live, Some(2));
        assert_eq!(t.rows[0].live, Some(-1));
        assert_eq!(t.rows[1].live, None);
    }

    #[test]
    fn road_through_a_cup() {
        let team = |id: u64, name: &str| ge::GeTeam { id, name: name.into(), badge: None };
        let m = |id: u64, phase: &str, home: bool, score: Option<(i64, i64)>, day: u32| GeMatch {
            id,
            championship: "Copa do Brasil".into(),
            phase: phase.into(),
            round: None,
            day: Day { y: 2026, m: 8, d: day as i64 },
            start: Some(Day { y: 2026, m: 8, d: day as i64 }.brt_ms(21, 30)),
            home: if home { team(284, "Grêmio") } else { team(1936, "Internacional") },
            away: if home { team(1936, "Internacional") } else { team(284, "Grêmio") },
            score,
            penalties: None,
            live: false,
            venue: None,
            channels: vec![],
            video: None,
            url: None,
        };
        let a = m(1, "Quartas de final", false, Some((0, 0)), 25);
        let b = m(2, "Quartas de final", true, Some((3, 1)), 28);
        let steps = road(&[&a, &b], Day { y: 2026, m: 10, d: 1 }.brt_ms(0, 0));
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].stage, "Quarterfinal");
        assert_eq!(steps[0].opponent, "Internacional");
        assert_eq!(steps[0].legs, "0–0 away · 3–1 home");
        assert_eq!(steps[0].result, "Won 3–1");
        assert_eq!(ordinal(15), "15th");
        assert_eq!(ordinal(22), "22nd");
    }
}


/// Against the real pages and today's games: cargo test real_live_tv -- --ignored --nocapture
#[cfg(test)]
mod live_pipeline {
    use super::*;

    #[test]
    #[ignore]
    fn real_live_tv() {
        tauri::async_runtime::block_on(async {
            let http = reqwest::Client::new();
            let text = |u: String| {
                let http = http.clone();
                async move { http.get(u).header("User-Agent", UA).send().await.unwrap().text().await.unwrap() }
            };
            let now = now_ms();
            let mut chans = tv::parse_globo(&text(tv::GLOBO_URL.into()).await).unwrap();
            for c in &tv::ESPN {
                let slots = c.listing.parse(&text(c.listing.url()).await, now);
                chans.push(tv::espn_channel(c, slots));
            }
            let caze = http.get(cazetv::URL).header("User-Agent", UA).header("Cookie", cazetv::COOKIE).send().await.unwrap().text().await.unwrap();
            let mut streams = cazetv::parse(&caze).unwrap();
            // With YOUTUBE_API_KEY (from .env), the API's details for the page's streams.
            let _ = dotenvy::dotenv();
            if let Some(key) = std::env::var("YOUTUBE_API_KEY").ok().filter(|k| !k.trim().is_empty()) {
                let ids: Vec<String> = streams.iter().map(|s| s.id.clone()).collect();
                let v: Value = http.get(cazetv::videos_url(&ids[..ids.len().min(50)])).header(cazetv::KEY_HEADER, key.trim()).send().await.unwrap().json().await.unwrap();
                assert!(cazetv::api_error(&v).is_none(), "{:?}", cazetv::api_error(&v));
                streams = cazetv::parse_videos(&v);
                println!("CazéTV through the Data API: {} live, {} scheduled", streams.iter().filter(|s| s.live).count(), streams.iter().filter(|s| !s.live).count());
                for s in streams.iter().filter(|s| s.live) {
                    println!("   {} viewers={} since={:?}", s.title, s.viewers, s.since.map(|t| (now - t) / 60_000));
                }
            }
            chans.extend(tv::cazetv_channels(&streams, now));
            let mut games = Vec::new();
            for l in LEAGUES {
                let v: Value = serde_json::from_str(&text(espn::scoreboard_url(l, Some(Day::slate(now)))).await).unwrap();
                games.extend(espn::parse_events(l, &v));
            }
            println!("{} games today", games.len());
            let refs: Vec<&Game> = games.iter().collect();
            match_tv(&mut chans, &refs, now);
            for c in &chans {
                if let Some(s) = c.on_at(now) {
                    let g = s.game.as_ref().map(|g| format!("{} {:?}-{:?} {} [{}]", g.home.name, g.home.score, g.away.score, g.away.name, g.status));
                    println!("{:10} {:?} {} | {} | followed={} | {:?}", c.name, s.kind, s.title, s.sub, s.followed, g);
                }
            }
            let watch = tv_watch(&chans, now);
            println!("watch channels: {:?}", watch.iter().map(|(id, l)| (id, l.iter().map(|c| c.name.clone()).collect::<Vec<_>>())).collect::<Vec<_>>());
        });
    }
}
