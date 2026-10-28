//! Live on TV: what the channels show now and next, for the Sports tab's rows, the TV guide, the
//! Live tab and the live-channel overlay.
//!
//! - Globoplay's whole lineup (TV Globo, GloboNews, sportv 1–3, Premiere, Multishow, Telecine…):
//!   one GET of Globoplay's "Agora na TV" page. Its Apollo state (`Quicksilver.apolloState`) holds
//!   every live broadcast with its next five EPG slots: `liveBroadcast` tells live from replay, the
//!   media's `availableFor` and `subscriptionServices` what it takes to watch, `assets.thumbUrl` is
//!   a frame of what's on air. Plays at globoplay.globo.com/{slug}/ao-vivo/{mediaId}/. The Sports
//!   tab keeps the sports channels, and TV Globo while it shows a match.
//! - ESPN 1–5: meuguia.tv's channel pages, in Brasília time, a week ahead. A slot ends when the next
//!   one starts; "- Ao Vivo" marks a live broadcast and "VT -" a replay. The page's own NO AR marker
//!   is cached, so what's on now comes from the times. ESPN 6 isn't on meuguia: guiadetv.com's
//!   page lists its day, with an "Ao vivo" badge and a description per programme.
//! - ESPN plays on Disney+, each channel at its own fixed /play/ address.
//! - CazéTV: its YouTube channel's streams tab (cazetv.rs), every live stream a channel of its own.
//! - News channels on YouTube (crate::livetv): each programme is a stream of its own, scheduled
//!   ahead, so a channel's streams tab is its programme guide.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::cazetv::Stream;
use super::dates::{parse_hm, Day};
use super::{fold, ge, league, Game, LEAGUES};
use crate::photos::rfc3339_ms;

pub const GLOBO_URL: &str = "https://globoplay.globo.com/agora-na-tv/";
const MEUGUIA: &str = "https://meuguia.tv/programacao/canal/";
const GUIADETV: &str = "https://www.guiadetv.com/canal/";
const GLOBO_LIVE: &str = "https://globoplay.globo.com";
const DISNEY_PLAY: &str = "https://www.disneyplus.com/pt-br/play/";
/// The YouTube TV app, at a video.
pub const YOUTUBE_WATCH: &str = "https://www.youtube.com/tv#/watch?v=";
/// A slot listed last runs this long, for want of the next one's start.
const LAST_SLOT_MS: i64 = 2 * 3_600_000;
/// CazéTV's live streams shown as channels, the most watched first.
const CAZE_CHANNELS: usize = 4;
/// A live stream's length when its end isn't known: a match with the shows around it.
const STREAM_MS: i64 = 150 * 60_000;
/// A news show's length when the channel's next stream doesn't say.
const NEWS_SHOW_MS: i64 = 2 * 3_600_000;

/// Where an ESPN channel's programme is listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listing {
    /// meuguia.tv's channel code.
    Meuguia(&'static str),
    /// guiadetv.com's channel slug.
    Guiadetv(&'static str),
}

impl Listing {
    pub fn url(self) -> String {
        match self {
            Listing::Meuguia(code) => format!("{MEUGUIA}{code}"),
            Listing::Guiadetv(slug) => format!("{GUIADETV}{slug}"),
        }
    }

    pub fn parse(self, html: &str, now: i64) -> Vec<Slot> {
        match self {
            Listing::Meuguia(_) => parse_meuguia(html, now),
            Listing::Guiadetv(_) => parse_guiadetv(html),
        }
    }
}

pub struct EspnChannel {
    pub key: &'static str,
    pub name: &'static str,
    pub listing: Listing,
    /// Its Disney+ /play/ id.
    pub play: &'static str,
}

const fn espn(key: &'static str, name: &'static str, listing: Listing, play: &'static str) -> EspnChannel {
    EspnChannel { key, name, listing, play }
}

pub const ESPN: [EspnChannel; 6] = [
    espn("espn", "ESPN", Listing::Meuguia("ESP"), "ef857a31-bd76-436b-b327-8889d90e6e5c"),
    espn("espn-2", "ESPN 2", Listing::Meuguia("ES2"), "ab853641-69f3-4e7f-8b56-6c06719d7734"),
    espn("espn-3", "ESPN 3", Listing::Meuguia("ES3"), "aa536155-4878-4b15-be18-011382749efc"),
    espn("espn-4", "ESPN 4", Listing::Meuguia("ES4"), "889e14c4-4e16-4f3f-8e83-90ee0e8157f7"),
    espn("espn-5", "ESPN 5", Listing::Meuguia("ES5"), "cb3c2eb3-3d43-402f-a9ac-31bb4e50ed82"),
    espn("espn-6", "ESPN 6", Listing::Guiadetv("espn-6"), "87ccc7a9-3098-47a1-a135-d57b02f66cd1"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A live event: a match, a race.
    Live,
    /// A show (studio, news, documentary), live or not.
    Air,
    /// A replay of an event.
    Replay,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slot {
    /// Unix ms.
    pub start: i64,
    pub end: i64,
    /// "Grêmio x Internacional", "SportsCenter".
    pub title: String,
    /// "Brasileirão · Round 32", "Studio".
    pub sub: String,
    pub kind: Kind,
    /// Both sides of a match, in the order the source writes them.
    pub teams: Option<(String, String)>,
    /// The competition as the source names it ("Campeonato Brasileiro de Futebol", "NFL").
    pub competition: Option<String>,
    pub desc: Option<String>,
    /// The programme's own picture (Globoplay's cover).
    pub image: Option<String>,
    /// The ESPN game it shows, matched per request (mod.rs).
    #[serde(default)]
    pub game: Option<Game>,
    /// A competition the launcher follows.
    #[serde(default)]
    pub followed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TvChannel {
    /// "sportv-2", "espn-3".
    pub key: String,
    pub name: String,
    pub color: String,
    /// The channel chips: "sportv", "premiere", "espn", "cazetv", "more"; outside sports "news"
    /// and "tv".
    pub group: String,
    /// "globo", "espn" or "youtube".
    pub provider: String,
    /// What it takes to watch: "Globoplay Premium", "Premiere", "Free with login", "Disney+ Premium".
    pub access: String,
    pub free: bool,
    /// A frame of what's on air (Globoplay, YouTube), else the channel's on-air picture.
    pub frame: Option<String>,
    /// apps.toml app it plays in.
    pub app: String,
    /// Where it plays.
    pub url: Option<String>,
    /// Oldest first.
    pub slots: Vec<Slot>,
    /// Watching a YouTube stream now.
    #[serde(default)]
    pub viewers: Option<u64>,
}

/// The groups of the Sports tab's channels.
const SPORTS_GROUPS: [&str; 5] = ["sportv", "premiere", "espn", "cazetv", "more"];

/// Whether a channel of the lineup belongs on the Sports tab: a sports channel with a programme,
/// or TV Globo while it shows a match, which is then filed under More.
pub fn sports_channel(c: &mut TvChannel) -> bool {
    if c.key == "tv-globo" {
        c.group = "more".into();
        return c.slots.iter().any(|s| s.teams.is_some());
    }
    SPORTS_GROUPS.contains(&c.group.as_str()) && !c.slots.is_empty()
}

impl TvChannel {
    /// What's on at `now`.
    pub fn on_at(&self, now: i64) -> Option<&Slot> {
        self.slots.iter().find(|s| s.start <= now && now < s.end)
    }

    pub fn after(&self, now: i64) -> Option<&Slot> {
        self.slots.iter().find(|s| s.start > now)
    }
}

// ---------- Text ----------

const SMALL_WORDS: &[&str] = &["de", "da", "do", "das", "dos", "e", "x", "em", "no", "na"];
const ACRONYMS: &[&str] = &["nfl", "nba", "nbb", "ufc", "atp", "wta", "wnba", "mlb", "nhl", "gp", "eua", "f1", "mma", "ncaa"];

/// Globo's Title Case, tidied: "Gp Dos Eua" → "GP dos EUA", "Nfl" → "NFL".
fn tidy(s: &str) -> String {
    s.split_whitespace()
        .enumerate()
        .map(|(i, w)| {
            let lower = w.to_lowercase();
            if ACRONYMS.contains(&lower.as_str()) {
                w.to_uppercase()
            } else if i > 0 && SMALL_WORDS.contains(&lower.as_str()) {
                lower
            } else {
                w.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .trim()
        .to_string()
}

/// "Grêmio X Internacional", "Sunderland x Leeds": both sides.
fn split_teams(text: &str) -> Option<(String, String)> {
    let (a, b) = text.split_once(" X ").or_else(|| text.split_once(" x "))?;
    let (a, b) = (a.trim(), b.trim());
    let fits = |s: &str| !s.is_empty() && s.len() <= 48 && !s.contains(" - ");
    (fits(a) && fits(b)).then(|| (a.to_string(), b.to_string()))
}

/// The competition the launcher follows that a source's name means, if any: "Campeonato
/// Brasileiro de Futebol", "BRASILEIRÃO 2026", "LALIGA 26/27".
pub fn followed_league(name: &str) -> Option<&'static super::League> {
    let c = fold(name).replace(" de futebol", "");
    let c: String = c.chars().filter(|ch| !ch.is_ascii_digit() && *ch != '/').collect();
    let c = c.split_whitespace().collect::<Vec<_>>().join(" ");
    let c = c.as_str();
    if c.contains("feminin") || c.contains("sub-") || c.contains("sub 2") {
        return None;
    }
    LEAGUES.iter().find(|l| {
        fold(l.name) == c || l.ge.iter().any(|g| fold(g) == c) || (l.id == "nba" && c == "nba")
    })
}

/// "Campeonato Brasileiro de Futebol" → "Brasileirão"; others tidied.
fn short_competition(name: &str) -> String {
    match followed_league(name) {
        Some(l) => l.name.to_string(),
        None => {
            // Without the season: "2026", "26/27" (but "Ligue 1" keeps its 1).
            let mut words: Vec<String> = tidy(name).split_whitespace().map(str::to_string).collect();
            while words.len() > 1
                && words.last().is_some_and(|w| w.contains('/') || (w.len() == 4 && w.chars().all(|c| c.is_ascii_digit())))
            {
                words.pop();
            }
            let t = words.join(" ");
            t.strip_suffix(" de Futebol").map(str::to_string).unwrap_or(t)
        }
    }
}

/// "7ª Rodada" → "Round 7"; "Quartas De Final" → "Quarterfinal".
fn stage_text(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let f = fold(t);
    if f.ends_with("rodada") {
        let n: String = f.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !n.is_empty() {
            return Some(format!("Round {n}"));
        }
    }
    let phase = ge::phase_name(&f);
    Some(if phase == f { tidy(t) } else { phase })
}

/// Words in a programme name that make it an event rather than a show.
const EVENT_WORDS: &[&str] = &[
    "campeonato", "copa", "liga", "mundial", "formula", "nfl", "nba", "nbb", "superliga", "grand prix", "ufc",
    "atp", "wta", "torneio", "olimp", "conmebol", "uefa", "premier league", "champions", "libertadores",
    "sul-americana", "stock car", "motogp", "indy", "brasileirao", "paulistao", "gauchao", "carioca", "volei",
    "basquete", "tenis", "futebol",
];
/// …and the ones that make it a show around an event.
const SHOW_WORDS: &[&str] = &["aquecimento", "pre-jogo", "pre jogo", "pos-jogo", "pos jogo", "news", "programa", "resenha", "debate"];

fn is_event_name(name: &str) -> bool {
    let f = fold(name);
    EVENT_WORDS.iter().any(|w| f.contains(w)) && !SHOW_WORDS.iter().any(|w| f.contains(w))
}

// ---------- Globoplay ----------

/// The JSON object after `apolloState:` in the page.
fn apollo_state(html: &str) -> Option<Value> {
    let at = html.find("apolloState:")? + "apolloState:".len();
    serde_json::Deserializer::from_str(&html[at..]).into_iter::<Value>().next()?.ok()
}

fn deref<'a>(state: &'a Value, v: &Value) -> Option<&'a Value> {
    state.get(v["__ref"].as_str()?)
}

/// A key that takes arguments: `epgCurrentSlots({"limit":5})`.
fn keyed<'a>(v: &'a Value, prefix: &str) -> Option<&'a Value> {
    v.as_object()?.iter().find(|(k, _)| k.starts_with(prefix)).map(|(_, x)| x)
}

fn globo_slot(s: &Value, state: &Value) -> Option<Slot> {
    let start = s["startTime"].as_i64()? * 1000;
    let end = s["endTime"].as_i64()? * 1000;
    let name = tidy(s["name"].as_str().unwrap_or_default());
    let metadata = s["metadata"].as_str().unwrap_or_default().trim();
    let live = s["liveBroadcast"].as_bool().unwrap_or(false);
    let (head, rest) = metadata.split_once(" - ").unwrap_or((metadata, ""));
    let teams = split_teams(head);
    let event = teams.is_some() || s["relatedEventId"].is_string() || is_event_name(&name);
    let kind = match (live, event && !SHOW_WORDS.iter().any(|w| fold(&name).contains(w))) {
        (true, true) => Kind::Live,
        (false, true) => Kind::Replay,
        _ => Kind::Air,
    };
    let image = s["title"]
        .is_object()
        .then(|| deref(state, &s["title"]))
        .flatten()
        .and_then(|t| t["cover"]["x16_9"].as_str())
        .map(str::to_string);
    let desc = s["description"].as_str().map(str::trim).filter(|d| !d.is_empty()).map(str::to_string);
    let (title, sub, competition) = match &teams {
        Some((a, b)) => {
            let comp = short_competition(&name);
            let sub = [Some(comp), stage_text(rest)].into_iter().flatten().collect::<Vec<_>>().join(" · ");
            (format!("{} x {}", tidy(a), tidy(b)), sub, Some(name.clone()))
        }
        None => (name.clone(), tidy(metadata), is_event_name(&name).then(|| name.clone())),
    };
    Some(Slot {
        start,
        end,
        title,
        sub,
        kind,
        teams: teams.map(|(a, b)| (tidy(&a), tidy(&b))),
        competition,
        desc,
        image,
        game: None,
        followed: false,
    })
}

/// What it takes to watch a Globoplay broadcast, and whether that's free.
fn globo_access(media: Option<&Value>, state: &Value) -> (String, bool) {
    let Some(m) = media else {
        return ("Globoplay".into(), false);
    };
    let services: Vec<String> = m["subscriptionServices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| deref(state, r)?["name"].as_str().map(str::to_string))
        .collect();
    match m["availableFor"].as_str() {
        Some("ANONYMOUS") => ("Free".into(), true),
        Some("LOGGED_IN") => ("Free with login".into(), true),
        _ if services.iter().any(|s| s == "Premium") => ("Globoplay Premium".into(), false),
        _ => (services.first().cloned().unwrap_or_else(|| "Globoplay".into()), false),
    }
}

/// The colors of Globoplay's other channels, close to their logos'.
const GLOBO_COLORS: &[(&str, &str)] = &[
    ("globonews", "#C92A2A"),
    ("cbn", "#E03131"),
    ("multishow", "#D6336C"),
    ("gnt", "#F08C00"),
    ("globoplay-novelas", "#AE3EC9"),
    ("universal", "#364FC7"),
    ("studio-universal", "#495057"),
    ("usa", "#1C7ED6"),
    ("megapix", "#A61E4D"),
    ("telecine", "#D9480F"),
    ("canal-brasil", "#5C940D"),
    ("bis", "#E03131"),
    ("canal-off", "#0C8599"),
    ("modo-viagem", "#1098AD"),
    ("futura", "#74B816"),
    ("gloobinho", "#FFD43B"),
    ("gloob", "#FAB005"),
    ("malhacao", "#F783AC"),
    ("dpa", "#4DABF7"),
    ("receitas", "#F76707"),
];

/// A Globoplay channel's group (its chip) and color, from its slug and categories.
fn globo_style(slug: &str, categories: &[&str]) -> (&'static str, &'static str) {
    match slug {
        _ if slug.starts_with("sportv") || slug.starts_with("mosaico-sportv") => ("sportv", "#E8590C"),
        _ if slug.starts_with("premiere") => ("premiere", "#2F9E44"),
        "ge-tv" => ("more", "#0CA678"),
        "combate" => ("more", "#E03131"),
        "tv-globo" => ("tv", "#1E63E9"),
        _ => {
            let color = GLOBO_COLORS.iter().find(|(s, _)| slug.starts_with(s)).map_or("#5c5c66", |(_, c)| c);
            let group = if categories.contains(&"jornalismo") {
                "news"
            } else if categories.contains(&"esportes") {
                "more"
            } else {
                "tv"
            };
            (group, color)
        }
    }
}

/// Globoplay's lineup, from the "Agora na TV" page: every channel, those without a programme
/// listed (CBN's radio) too. None when the page changed.
pub fn parse_globo(html: &str) -> Option<Vec<TvChannel>> {
    let state = apollo_state(html)?;
    let mut out = Vec::new();
    for (key, b) in state.as_object()? {
        if !key.starts_with("Broadcast:") {
            continue;
        }
        let (Some(slug), Some(media_id)) = (b["slug"].as_str(), b["mediaId"].as_str()) else {
            continue;
        };
        let categories: Vec<&str> =
            b["categories"].as_array().into_iter().flatten().filter_map(|c| c["slug"].as_str()).collect();
        let slots: Vec<Slot> = keyed(b, "epgCurrentSlots")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|s| globo_slot(s, &state))
            .collect();
        let (access, free) = globo_access(deref(&state, &b["media"]), &state);
        let (group, color) = globo_style(slug, &categories);
        let frame = b["assets"]["thumbUrl"]
            .as_str()
            .or_else(|| keyed(b, "imageOnAir").and_then(Value::as_str))
            .map(str::to_string);
        out.push(TvChannel {
            key: slug.to_string(),
            name: b["name"].as_str().unwrap_or(slug).to_string(),
            color: color.to_string(),
            group: group.to_string(),
            provider: "globo".into(),
            access,
            free,
            frame,
            app: "globoplay".into(),
            url: Some(format!("{GLOBO_LIVE}/{slug}/ao-vivo/{media_id}/")),
            slots,
            viewers: None,
        });
    }
    out.sort_by_key(|c| (globo_order(&c.key), c.key.clone()));
    (!out.is_empty()).then_some(out)
}

/// Globoplay's channels in the order they're listed, roughly the most watched first; a slug
/// that starts like one listed ("premiere-2", "telecine-cult") goes with it.
const GLOBO_ORDER: &[&str] = &[
    "tv-globo", "premiere", "sportv", "sportv-2", "sportv-3", "ge-tv", "combate", "mosaico-sportv", "globonews",
    "cbn", "multishow", "gnt", "globoplay-novelas", "universal", "studio-universal", "usa", "megapix", "telecine",
    "canal-brasil", "bis", "canal-off", "modo-viagem", "futura", "gloob", "gloobinho", "malhacao", "dpa", "receitas",
];

fn globo_order(slug: &str) -> usize {
    GLOBO_ORDER
        .iter()
        .position(|s| *s == slug)
        .or_else(|| GLOBO_ORDER.iter().position(|s| slug.starts_with(s)))
        .unwrap_or(GLOBO_ORDER.len())
}

// ---------- Listings (ESPN) ----------

fn sport_label(category: &str) -> String {
    let what = category.split_once('/').map_or(category, |(_, w)| w);
    match fold(category).as_str() {
        "jornalismo/esportivo" => "Studio".into(),
        "documentario/esportivo" => "Documentary".into(),
        c if c.ends_with("/futebol americano") => "American football".into(),
        c if c.ends_with("/futebol") => "Football".into(),
        c if c.ends_with("/basquete") => "Basketball".into(),
        c if c.ends_with("/tenis") => "Tennis".into(),
        c if c.ends_with("/volei") => "Volleyball".into(),
        c if c.ends_with("/automobilismo") => "Motorsport".into(),
        _ => what.to_string(),
    }
}

/// One listed programme: "NFL: Bengals x Ravens - Ao Vivo", "VT - Lecce x Juventus". `badge` is
/// a live badge beside it (guiadetv); `category` is meuguia's ("Esporte/Futebol"), which tells an
/// event from a show (without one, a match or an event's name does).
fn listing_slot(start: i64, raw: &str, badge: bool, category: Option<&str>, desc: Option<String>) -> Slot {
    let mut t = decode(raw);
    let mut live = badge;
    if fold(&t).ends_with("- ao vivo") {
        live = true;
        t = t[..t.rfind('-').unwrap_or(t.len())].trim().to_string();
    }
    let replay = t.starts_with("VT -") || t.starts_with("VT:");
    if replay {
        t = t[3..].trim_start_matches([' ', '-', ':']).trim().to_string();
    }
    // "NFL: Bengals x Ravens": the competition, then the match.
    let (competition, body) = match t.split_once(": ") {
        Some((c, rest)) if split_teams(rest).is_some() => (Some(c.trim().to_string()), rest.to_string()),
        _ => (None, t.clone()),
    };
    let teams = split_teams(&body);
    let event = match category {
        Some(c) => fold(c).starts_with("esporte/"),
        None => teams.is_some() || is_event_name(&body),
    };
    let kind = if replay || (!live && teams.is_some()) {
        Kind::Replay
    } else if live && event {
        Kind::Live
    } else {
        Kind::Air
    };
    let label = category.map(sport_label);
    let sub = match (&teams, &competition, label) {
        (Some(_), Some(c), Some(l)) => format!("{c} · {l}"),
        (Some(_), Some(c), None) => c.clone(),
        (_, _, Some(l)) => l,
        _ => String::new(),
    };
    Slot {
        start,
        end: start + LAST_SLOT_MS,
        title: if teams.is_some() { body } else { t },
        sub,
        kind,
        teams,
        competition,
        desc,
        image: None,
        game: None,
        followed: false,
    }
}

/// Oldest first, each ending when the next starts; a long broadcast listed in parts (NFL
/// Redzone hour by hour) becomes one slot.
fn close_slots(out: &mut Vec<Slot>) {
    out.sort_by_key(|x| x.start);
    out.dedup_by(|b, a| a.start == b.start && a.title == b.title);
    for i in 1..out.len() {
        let next = out[i].start;
        if next > out[i - 1].start {
            out[i - 1].end = next;
        }
    }
    out.dedup_by(|b, a| {
        let same = a.title == b.title && a.kind == b.kind && a.end == b.start;
        if same {
            a.end = b.end;
        }
        same
    });
}

/// A meuguia.tv channel page's programmes, oldest first. `now` gives the year the page leaves out.
pub fn parse_meuguia(html: &str, now: i64) -> Vec<Slot> {
    // The page carries its own template in comments.
    let mut clean = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find("<!--") {
        clean.push_str(&rest[..i]);
        rest = rest[i..].find("-->").map_or("", |j| &rest[i + j + 3..]);
    }
    clean.push_str(rest);

    const DAY: &str = "<li class=\"subheader";
    const TIME: &str = "<div class='lileft time'>";
    let today = Day::brt(now);
    let mut day: Option<Day> = None;
    let mut out: Vec<Slot> = Vec::new();
    let mut s = clean.as_str();
    loop {
        let (d, t) = (s.find(DAY), s.find(TIME));
        let at = match (d, t) {
            (None, None) => break,
            (Some(d), Some(t)) => d.min(t),
            (Some(d), None) => d,
            (None, Some(t)) => t,
        };
        s = &s[at..];
        if s.starts_with(DAY) {
            // <li class="subheader devicepadding">domingo, 25/10</li>
            let text = s.find('>').map(|i| &s[i + 1..]).and_then(|x| x.split('<').next()).unwrap_or_default();
            day = text.rsplit(' ').next().and_then(|dm| {
                let (d, m) = dm.split_once('/')?;
                let (d, m): (i64, i64) = (d.trim().parse().ok()?, m.trim().parse().ok()?);
                // December's page lists January's days of next year.
                let y = if m < today.m - 6 { today.y + 1 } else { today.y };
                Some(Day { y, m, d })
            });
            s = &s[DAY.len()..];
            continue;
        }
        s = &s[TIME.len()..];
        let hm = s.split('<').next().unwrap_or_default();
        let field = |s: &str, tag: &str| -> String {
            let open = format!("<{tag}>");
            s.find(&open).and_then(|i| s[i + open.len()..].split(&format!("</{tag}>")).next()).unwrap_or_default().to_string()
        };
        // The fields of this entry only, up to the next one.
        let end = s.find(TIME).unwrap_or(s.len()).min(s.find(DAY).unwrap_or(s.len()));
        let entry = &s[..end];
        let (Some(day), Some((h, m))) = (day, parse_hm(hm)) else { continue };
        let title = field(entry, "h2");
        if title.is_empty() {
            continue;
        }
        let start = day.brt_ms(h, m);
        out.push(listing_slot(start, &title, false, Some(&decode(&field(entry, "h3"))), None));
    }
    close_slots(&mut out);
    out
}

/// A guiadetv.com channel page's programmes: one `<li>` each, with its start (`<time datetime>`,
/// Brasília offset), its name as a link, an "Ao vivo" badge when it's live and a description. The
/// day's last one ends at the page's JSON-LD `endDate`.
pub fn parse_guiadetv(html: &str) -> Vec<Slot> {
    const TIME: &str = "<time datetime=\"";
    let mut out: Vec<Slot> = Vec::new();
    let mut s = html;
    while let Some(i) = s.find(TIME) {
        s = &s[i + TIME.len()..];
        let Some(start) = s.split('"').next().and_then(rfc3339_ms) else { continue };
        let entry = &s[..s.find(TIME).unwrap_or(s.len())];
        let Some(a) = entry.find("href=\"/programa/") else { continue };
        let Some(name) = entry[a..].split_once('>').and_then(|(_, r)| r.split_once("</a>")).map(|(n, _)| n.trim()) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let live = fold(entry).contains(">ao vivo</span>");
        let desc = entry
            .find("<p")
            .and_then(|p| entry[p..].split_once('>'))
            .and_then(|(_, r)| r.split_once("</p>"))
            .map(|(d, _)| decode(d))
            .filter(|d| !d.is_empty());
        out.push(listing_slot(start, name, live, None, desc));
    }
    close_slots(&mut out);
    let last_end = html
        .split("\"endDate\":\"")
        .skip(1)
        .filter_map(|x| x.split('"').next().and_then(rfc3339_ms))
        .max();
    if let (Some(last), Some(end)) = (out.last_mut(), last_end) {
        if end > last.start {
            last.end = end;
        }
    }
    out
}

/// An ESPN channel with its programme, playing at its Disney+ address.
pub fn espn_channel(c: &EspnChannel, slots: Vec<Slot>) -> TvChannel {
    TvChannel {
        key: c.key.to_string(),
        name: c.name.to_string(),
        color: "#D7141A".into(),
        group: "espn".into(),
        provider: "espn".into(),
        access: "Disney+ Premium".into(),
        free: false,
        frame: None,
        app: "disney".into(),
        url: Some(format!("{DISNEY_PLAY}{}", c.play)),
        slots,
        viewers: None,
    }
}

/// Keeps what the screens can show: from a few hours ago to two days ahead.
pub fn trim(slots: &mut Vec<Slot>, now: i64) {
    slots.retain(|s| s.end > now - 6 * 3_600_000 && s.start < now + 48 * 3_600_000);
}

// ---------- CazéTV ----------

/// Club initials kept in capitals when a title written all in capitals is tidied.
const CLUB_INITIALS: &[&str] = &["fc", "ac", "sc", "ec", "cr", "rb", "psg", "psv", "afc", "estac", "usa", "aek", "az", "ii"];
/// …and the names news shows write in initials.
const NEWS_INITIALS: &[&str] = &[
    "cnn", "sbt", "uol", "tv", "cbn", "gzh", "bbc", "stf", "stj", "tse", "pt", "pl", "eua", "onu", "cpi", "pib", "ibge",
    "inss", "sus", "rs", "sp", "rj", "df", "b3", "ceo",
];

/// "PARIS SAINT-GERMAIN X LYON" → "Paris Saint-Germain x Lyon"; text already in mixed case is
/// only tidied.
fn title_case(s: &str) -> String {
    if s.chars().any(char::is_lowercase) {
        return tidy(s);
    }
    s.split_whitespace()
        .enumerate()
        .map(|(i, w)| {
            let lower = w.to_lowercase();
            let bare = lower.trim_matches(|c: char| !c.is_alphanumeric());
            if [CLUB_INITIALS, ACRONYMS, NEWS_INITIALS].iter().any(|l| l.contains(&bare)) {
                return w.to_string();
            }
            if i > 0 && SMALL_WORDS.contains(&lower.as_str()) {
                return lower;
            }
            lower
                .split('-')
                .map(|p| {
                    let mut c = p.chars();
                    c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join("-")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A stream's slot from its title: "AO VIVO: FLUMINENSE X INTERNACIONAL | BRASILEIRÃO 2026 | 33ª
/// RODADA" (match, competition, round), "AO VIVO (SOM AMBIENTE): …" (the field's sound only).
fn stream_slot(s: &Stream, now: i64) -> Slot {
    let raw = s.title.trim();
    let ambient = fold(raw).starts_with("ao vivo (som ambiente)");
    let body = if fold(raw).starts_with("ao vivo") { raw.split_once(':').map_or(raw, |(_, r)| r.trim()) } else { raw };
    let mut parts = body.split(" | ").map(str::trim);
    let head = parts.next().unwrap_or_default();
    let competition = parts.next().filter(|c| !c.is_empty()).map(title_case);
    let stage = parts.next().and_then(stage_text);
    let teams = split_teams(head).map(|(a, b)| (title_case(&a), title_case(&b)));
    let title = match &teams {
        Some((a, b)) => format!("{a} x {b}"),
        None => title_case(head),
    };
    let sub = [competition.as_deref().map(short_competition), stage, ambient.then(|| "Ambient sound".to_string())]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    let (start, end) = if s.live {
        let start = s.since.or(s.scheduled).unwrap_or(now);
        // Still on past its usual length: it runs a little longer.
        (start, (start + STREAM_MS).max(now + 15 * 60_000))
    } else {
        let start = s.scheduled.unwrap_or(now);
        (start, start + STREAM_MS)
    };
    Slot {
        start,
        end,
        kind: if teams.is_some() || is_event_name(&title) { Kind::Live } else { Kind::Air },
        title,
        sub,
        teams,
        competition,
        desc: None,
        image: s.thumb.clone(),
        game: None,
        followed: false,
    }
}

/// CazéTV on the launcher's TV rows: each live stream a channel, the most watched first (so
/// "CazéTV" is the big match), the scheduled ones after the first. With nothing live, one channel
/// with what's scheduled.
pub fn cazetv_channels(streams: &[Stream], now: i64) -> Vec<TvChannel> {
    let mut live: Vec<&Stream> = streams.iter().filter(|s| s.live).collect();
    live.sort_by_key(|s| std::cmp::Reverse(s.viewers));
    let mut later: Vec<&Stream> = streams.iter().filter(|s| !s.live && s.scheduled.is_some_and(|t| t > now - 3_600_000)).collect();
    later.sort_by_key(|s| s.scheduled);
    let channel = |i: usize, key: String, s: &Stream, slots: Vec<Slot>| TvChannel {
        key,
        name: if i == 0 { "CazéTV".to_string() } else { format!("CazéTV {}", i + 1) },
        color: "#F2C200".into(),
        group: "cazetv".into(),
        provider: "youtube".into(),
        access: "Free on YouTube".into(),
        free: true,
        frame: s.live.then(|| s.thumb.clone()).flatten(),
        app: "youtube".into(),
        url: Some(format!("{YOUTUBE_WATCH}{}", s.id)),
        slots,
        viewers: s.live.then_some(s.viewers),
    };
    let mut out: Vec<TvChannel> = live
        .iter()
        .take(CAZE_CHANNELS)
        .enumerate()
        .map(|(i, s)| {
            let first = stream_slot(s, now);
            let from = first.start;
            let mut slots = vec![first];
            if i == 0 {
                slots.extend(later.iter().map(|l| stream_slot(l, now)).filter(|l| l.start >= from));
            }
            channel(i, format!("cazetv-{}", s.id), s, slots)
        })
        .collect();
    if out.is_empty() {
        if let Some(first) = later.first() {
            let slots = later.iter().map(|l| stream_slot(l, now)).collect();
            out.push(channel(0, "cazetv".into(), first, slots));
        }
    }
    out
}

// ---------- News on YouTube ----------

/// Keeps letters, digits and punctuation: emoji and symbols ("🔴") go.
fn plain(s: &str) -> String {
    s.chars().filter(|c| (*c as u32) < 0x2100).collect()
}

const MONTHS_PT: [&str; 12] = [
    "janeiro", "fevereiro", "marco", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro",
];

/// A title in capitals made readable, word by word when it's partly in capitals: "CENTRAL de
/// NOTÍCIAS, com Amanda Klein" → "Central de Notícias, com Amanda Klein". Initials stay.
fn calm(s: &str) -> String {
    if !s.chars().any(char::is_lowercase) {
        return title_case(s);
    }
    let words: Vec<String> = s
        .split(' ')
        .map(|w| {
            let letters = w.chars().filter(|c| c.is_alphabetic()).count();
            let shouted = letters >= 4 && !w.chars().any(char::is_lowercase);
            let bare = w.to_lowercase();
            let bare = bare.trim_matches(|c: char| !c.is_alphanumeric());
            if !shouted || [ACRONYMS, NEWS_INITIALS].iter().any(|l| l.contains(&bare)) {
                return w.to_string();
            }
            let lower = w.to_lowercase();
            let mut c = lower.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        })
        .collect();
    tidy(&words.join(" "))
}

/// A part of a stream's title without the date news channels add to every programme:
/// "JORNAL DA BAND - 26/10/2026" → "JORNAL DA BAND", "Jornalismo Bandeirantes - Manhã - Programa
/// de 26/10/2026" → "Jornalismo Bandeirantes - Manhã", "MANHÃ (26/10/2026)" → "MANHÃ"; a part that's
/// only a date written out ("30 de outubro de 2026") goes.
fn without_date(part: &str) -> String {
    let written = part.split_whitespace().all(|w| {
        let w = fold(w);
        w.chars().all(|c| c.is_ascii_digit()) || w == "de" || MONTHS_PT.contains(&w.as_str())
    });
    if written && part.split_whitespace().any(|w| MONTHS_PT.contains(&fold(w).as_str())) {
        return String::new();
    }
    let mut words: Vec<&str> = part.split_whitespace().collect();
    let mut dated = false;
    while let Some(last) = words.last() {
        let core = last.trim_matches(|c: char| "()[],-–—".contains(c));
        let digits = core.chars().filter(char::is_ascii_digit).count();
        let date = digits >= 3 && core.contains(['/', '.']) && core.chars().all(|c| c.is_ascii_digit() || c == '/' || c == '.');
        let filler = core.is_empty() || (dated && ["de", "do", "programa", "edicao"].contains(&fold(core).as_str()));
        if !date && !filler {
            break;
        }
        dated |= date;
        words.pop();
    }
    words.join(" ").trim_end_matches(|c: char| "-–—|:,·".contains(c) || c.is_whitespace()).to_string()
}

/// A news channel's stream as a programme: "AO VIVO: CNN PRIME TIME - 26/10/2026" → "CNN Prime
/// Time"; "[AO VIVO] BandNews Station | 26/10/2026" → "BandNews Station". A radio's commentary of a
/// match ("AO VIVO: CORITIBA X GRÊMIO | BRASILEIRO 2026 | JORNADA DIGITAL") is that match, so its
/// score can show. A live one runs until `until`, the channel's next stream, when that's known.
pub fn news_slot(s: &Stream, until: Option<i64>, now: i64) -> Slot {
    let mut t = plain(&s.title).trim().to_string();
    // "AO VIVO:", "[AO VIVO]", "AO VIVO |", "AO VIVO -" in front.
    for marker in ["[ao vivo]", "(ao vivo)", "ao vivo"] {
        let f = fold(&t);
        if f.starts_with(marker) && !f[marker.len()..].starts_with(char::is_alphanumeric) {
            t = t.chars().skip(marker.chars().count()).collect::<String>();
            t = t.trim_start_matches(|c: char| ":|-–".contains(c) || c.is_whitespace()).to_string();
        }
    }
    let parts: Vec<String> = t
        .split('|')
        .map(without_date)
        .filter(|p| !p.is_empty() && !["ao vivo", "live"].contains(&fold(p).as_str()))
        .collect();
    let head = parts.first().cloned().unwrap_or_else(|| plain(&s.title));
    let teams = split_teams(&head).map(|(a, b)| (title_case(&a), title_case(&b)));
    let (title, sub, competition) = match &teams {
        Some((a, b)) => {
            let competition = parts.get(1).map(|c| title_case(c));
            let rest = parts.iter().skip(2).map(|p| stage_text(p).filter(|x| x.starts_with("Round")).unwrap_or_else(|| title_case(p)));
            let sub = competition.iter().map(|c| short_competition(c)).chain(rest).collect::<Vec<_>>().join(" · ");
            (format!("{a} x {b}"), sub, competition)
        }
        None => (calm(&head), parts.iter().skip(1).map(|p| calm(p)).collect::<Vec<_>>().join(" · "), None),
    };
    let start = if s.live { s.since.or(s.scheduled).unwrap_or(now) } else { s.scheduled.unwrap_or(now) };
    let end = until.filter(|u| *u > start).unwrap_or(start + NEWS_SHOW_MS);
    Slot {
        start,
        // Still on past when it was due to end: a little longer.
        end: if s.live { end.max(now + 15 * 60_000) } else { end },
        title,
        sub,
        kind: if teams.is_some() { Kind::Live } else { Kind::Air },
        teams,
        competition,
        desc: None,
        image: s.thumb.clone(),
        game: None,
        followed: false,
    }
}

/// The followed competition a slot belongs to, for its ranking.
pub fn mark_followed(slot: &mut Slot) {
    slot.followed = slot.game.as_ref().is_some_and(|g| league(&g.league).is_some())
        || slot.competition.as_deref().and_then(followed_league).is_some();
}

#[cfg(test)]
mod tests {
    use super::*;

    const GLOBO_PAGE: &str = r#"<script> var Quicksilver = { version: "2026.10-11", apolloState: {"ROOT_QUERY":{"__typename":"Query"},
"Broadcast:{\"mediaId\":\"7339117\",\"affiliateCode\":null}":{"__typename":"Broadcast","mediaId":"7339117","name":"sportv 2","slug":"sportv-2",
 "imageOnAir({\"scale\":\"X1080\"})":"https://s2/onair.jpg","categories":[{"slug":"esportes"}],
 "epgCurrentSlots({\"limit\":5})":[
  {"name":"Nfl","metadata":"San Francisco 49ers X Atlanta Falcons","startTime":1792944660,"endTime":1792956660,"description":"Jogo da NFL","liveBroadcast":true,"relatedEventId":null,"title":{"__ref":"Title:{\"titleId\":\"sL7\"}"}},
  {"name":"Aquecimento sportv","metadata":"Nfl - Kansas City Chiefs X Seattle Seahawks","startTime":1792970700,"endTime":1792973940,"liveBroadcast":true,"title":null}],
 "assets":{"thumbUrl":"https://live.video.globo.com/thumbs/spo224ha.jpg"},"media":{"__ref":"Video:7339117"}},
"Broadcast:{\"mediaId\":\"8223631\",\"affiliateCode\":null}":{"__typename":"Broadcast","mediaId":"8223631","name":"Premiere","slug":"premiere","categories":[{"slug":"esportes"}],
 "epgCurrentSlots({\"limit\":5})":[
  {"name":"Campeonato Brasileiro de Futebol","metadata":"Grêmio X Internacional - 31ª Rodada","startTime":1792943647,"endTime":1792945485,"liveBroadcast":false,"title":null}],
 "media":{"__ref":"Video:8223631"}},
"Broadcast:{\"mediaId\":\"11134179\",\"affiliateCode\":null}":{"__typename":"Broadcast","mediaId":"11134179","name":"ge tv","slug":"ge-tv","categories":[{"slug":"gratuitos"},{"slug":"esportes"}],
 "epgCurrentSlots({\"limit\":5})":[{"name":"Que Coffee","metadata":null,"startTime":1792954560,"endTime":1792958160,"liveBroadcast":true,"title":null}],
 "media":{"__ref":"Video:11134179"}},
"Broadcast:{\"mediaId\":\"6120663\",\"affiliateCode\":null}":{"__typename":"Broadcast","mediaId":"6120663","name":"TV Globo","slug":"tv-globo","categories":[{"slug":"gratuitos"}],
 "epgCurrentSlots({\"limit\":5})":[{"name":"Domingão com Huck","metadata":null,"startTime":1792954560,"endTime":1792958160,"liveBroadcast":true}],"media":{"__ref":"Video:6120663"}},
"Broadcast:{\"mediaId\":\"7339101\",\"affiliateCode\":null}":{"__typename":"Broadcast","mediaId":"7339101","name":"GloboNews","slug":"globonews","categories":[{"slug":"jornalismo"}],
 "epgCurrentSlots({\"limit\":5})":[{"name":"Jornal","startTime":1792954560,"endTime":1792958160,"liveBroadcast":true}]},
"Title:{\"titleId\":\"sL7\"}":{"__typename":"Title","cover":{"x16_9":"https://s2/nfl-cover.jpg"}},
"Video:7339117":{"__typename":"Video","availableFor":"SUBSCRIBER","subscriptionServices":[{"__ref":"SubscriptionService:a"}]},
"Video:8223631":{"__typename":"Video","availableFor":"SUBSCRIBER","subscriptionServices":[{"__ref":"SubscriptionService:b"}]},
"Video:11134179":{"__typename":"Video","availableFor":"LOGGED_IN","subscriptionServices":[]},
"SubscriptionService:a":{"name":"Premium"},"SubscriptionService:b":{"name":"Premiere"}
}, rootNodeElement: null };</script>"#;

    #[test]
    fn globoplay_channels() {
        let mut chans = parse_globo(GLOBO_PAGE).unwrap();
        let lineup: Vec<(&str, &str)> = chans.iter().map(|c| (c.key.as_str(), c.group.as_str())).collect();
        assert_eq!(lineup, [("tv-globo", "tv"), ("premiere", "premiere"), ("sportv-2", "sportv"), ("ge-tv", "more"), ("globonews", "news")]);
        // TV Globo shows no match and GloboNews isn't a sports channel.
        chans.retain_mut(sports_channel);
        let keys: Vec<&str> = chans.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, ["premiere", "sportv-2", "ge-tv"]);

        let s2 = &chans[1];
        assert_eq!(s2.url.as_deref(), Some("https://globoplay.globo.com/sportv-2/ao-vivo/7339117/"));
        assert_eq!((s2.access.as_str(), s2.free), ("Globoplay Premium", false));
        assert_eq!(s2.frame.as_deref(), Some("https://live.video.globo.com/thumbs/spo224ha.jpg"));
        let nfl = &s2.slots[0];
        assert_eq!(nfl.kind, Kind::Live);
        assert_eq!(nfl.title, "San Francisco 49ers x Atlanta Falcons");
        assert_eq!(nfl.sub, "NFL");
        assert_eq!(nfl.image.as_deref(), Some("https://s2/nfl-cover.jpg"));
        assert_eq!(nfl.start, 1_792_944_660_000);
        // A pre-game show names the match, but isn't it.
        let pre = &s2.slots[1];
        assert_eq!((pre.kind, pre.teams.is_none()), (Kind::Air, true));
        assert_eq!(pre.sub, "NFL - Kansas City Chiefs x Seattle Seahawks");

        let replay = &chans[0].slots[0];
        assert_eq!(replay.kind, Kind::Replay);
        assert_eq!(replay.title, "Grêmio x Internacional");
        assert_eq!(replay.sub, "Brasileirão · Round 31");
        assert_eq!(chans[0].access, "Premiere");

        let ge_tv = &chans[2];
        assert_eq!((ge_tv.access.as_str(), ge_tv.free, ge_tv.group.as_str()), ("Free with login", true, "more"));
        assert_eq!(ge_tv.slots[0].kind, Kind::Air);
    }

    const MEUGUIA_PAGE: &str = r#"<ul class="mw"> <li class="subheader devicepadding">domingo, 25/10</li>
<!-- <% if dia_atual != "" %> <li class="subheader devicepadding"><%= dia %></li> <div class='lileft time'><%= x %></div> -->
<li> <a title="Sunderland x Leeds - Ao Vivo" class="devicepadding" > <div class='lileft time'>13:20</div> <div class="licontent"> <h2>Sunderland x Leeds - Ao Vivo</h2> <h3>Esporte/Futebol</h3> </div> <div class="liright"><div class="noar">NO AR</div></div> </a> </li>
<li> <a class="devicepadding"> <div class='lileft time'>15:30</div> <div class="licontent"> <h2>Especial Rally dos Sertões</h2> <h3>Documentário/Esportivo</h3> </div> </a> </li>
<li> <a class="devicepadding"> <div class='lileft time'>21:15</div> <div class="licontent"> <h2>Sportscenter - Ao Vivo</h2> <h3>Jornalismo/Esportivo</h3> </div> </a> </li>
<li class="subheader devicepadding">segunda-feira, 26/10</li>
<li> <a class="devicepadding"> <div class='lileft time'>00:15</div> <div class="licontent"> <h2>VT - Liverpool x Brighton &amp; Hove Albion</h2> <h3>Esporte/Futebol</h3> </div> </a> </li>
<li> <a class="devicepadding"> <div class='lileft time'>14:00</div> <div class="licontent"> <h2>NFL: Cincinnati Bengals x Baltimore Ravens - Ao Vivo</h2> <h3>Esporte/Futebol Americano</h3> </div> </a> </li>
</ul>"#;

    #[test]
    fn meuguia_listing() {
        let now = Day { y: 2026, m: 10, d: 25 }.brt_ms(13, 40);
        let slots = parse_meuguia(MEUGUIA_PAGE, now);
        assert_eq!(slots.len(), 5);
        let live = &slots[0];
        assert_eq!((live.title.as_str(), live.kind), ("Sunderland x Leeds", Kind::Live));
        assert_eq!(live.start, Day { y: 2026, m: 10, d: 25 }.brt_ms(13, 20));
        // Ends when the next one starts.
        assert_eq!(live.end, Day { y: 2026, m: 10, d: 25 }.brt_ms(15, 30));
        assert_eq!((slots[1].kind, slots[1].sub.as_str()), (Kind::Air, "Documentary"));
        assert_eq!((slots[2].kind, slots[2].title.as_str(), slots[2].sub.as_str()), (Kind::Air, "Sportscenter", "Studio"));
        let vt = &slots[3];
        assert_eq!((vt.kind, vt.title.as_str()), (Kind::Replay, "Liverpool x Brighton & Hove Albion"));
        assert_eq!(vt.start, Day { y: 2026, m: 10, d: 26 }.brt_ms(0, 15));
        let nfl = &slots[4];
        assert_eq!((nfl.kind, nfl.competition.as_deref()), (Kind::Live, Some("NFL")));
        assert_eq!(nfl.teams, Some(("Cincinnati Bengals".into(), "Baltimore Ravens".into())));
        assert_eq!(nfl.sub, "NFL · American football");
        assert_eq!(nfl.end - nfl.start, LAST_SLOT_MS);
    }

    #[test]
    fn names_and_links() {
        assert_eq!(tidy("Gp Dos Eua"), "GP dos EUA");
        assert_eq!(short_competition("Campeonato Brasileiro de Futebol"), "Brasileirão");
        assert_eq!(short_competition("Campeonato Alemão De Futebol"), "Bundesliga");
        assert_eq!(short_competition("Conmebol Libertadores Feminina 2026"), "Conmebol Libertadores Feminina");
        assert!(followed_league("Campeonato Brasileiro Série B").is_none());
        assert_eq!(stage_text("Quartas De Final").as_deref(), Some("Quarterfinal"));
        assert_eq!(followed_league("LALIGA 26/27").map(|l| l.id), Some("esp.1"));
        assert_eq!(followed_league("BRASILEIRÃO 2026").map(|l| l.id), Some("bra.1"));
        assert_eq!(title_case("PARIS SAINT-GERMAIN X LYON"), "Paris Saint-Germain x Lyon");
        assert_eq!(title_case("ANGERS X FC LORIENT"), "Angers x FC Lorient");
        let espn6 = espn_channel(&ESPN[5], vec![]);
        assert_eq!(espn6.url.as_deref(), Some("https://www.disneyplus.com/pt-br/play/87ccc7a9-3098-47a1-a135-d57b02f66cd1"));
    }

    const GUIADETV_PAGE: &str = r#"<script type="application/ld+json">{"@type":"TelevisionChannel","broadcastEvent":[{"endDate":"2026-10-26T00:30:00-03:00","name":"Bola da Vez","startDate":"2026-10-25T23:30:00-03:00"}]}</script>
<ul><li class="flex"><div><time datetime="2026-10-25T21:30:00-03:00" class="block">21:30</time> <span>2h</span></div><div><div><a href="/programa/vt-psv-x-feyenoord/1" class="font-medium">VT - PSV x Feyenoord</a> </div><p class="mt-1">O clássico holandês.</p></div></li>
<li class="flex"><div><time datetime="2026-10-25T22:30:00-03:00" class="block">22:30</time></div><div><div><a href="/programa/undinese-x-milan/2" class="font-medium">Undinese x Milan</a> <span class="inline-flex rounded-full">Ao vivo</span></div></div></li>
<li class="flex"><div><time datetime="2026-10-25T23:30:00-03:00" class="block">23:30</time></div><div><div><a href="/programa/bola/3" class="font-medium">Bola da Vez</a></div></div></li></ul>"#;

    #[test]
    fn guiadetv_listing() {
        let slots = parse_guiadetv(GUIADETV_PAGE);
        assert_eq!(slots.len(), 3);
        let vt = &slots[0];
        assert_eq!((vt.kind, vt.title.as_str(), vt.desc.as_deref()), (Kind::Replay, "PSV x Feyenoord", Some("O clássico holandês.")));
        assert_eq!(vt.start, Day { y: 2026, m: 10, d: 25 }.brt_ms(21, 30));
        assert_eq!(vt.end, slots[1].start);
        assert_eq!(slots[1].kind, Kind::Live);
        // The last one ends where the page says.
        assert_eq!((slots[2].kind, slots[2].end), (Kind::Air, Day { y: 2026, m: 10, d: 26 }.brt_ms(0, 30)));
    }

    #[test]
    fn cazetv_streams_as_channels() {
        let now = Day { y: 2026, m: 10, d: 25 }.brt_ms(14, 0);
        let stream = |id: &str, title: &str, live: bool, viewers: u64, scheduled: Option<i64>| Stream {
            id: id.into(),
            title: title.into(),
            live,
            viewers,
            scheduled,
            since: live.then_some(now - 30 * 60_000),
            thumb: Some(format!("https://i.ytimg.com/vi/{id}/hq720.jpg")),
        };
        let streams = [
            stream("small", "AO VIVO (SOM AMBIENTE): RENNES X STRASBOURG | LIGUE 1 26/27 | 8ª RODADA", true, 635, None),
            stream("big", "AO VIVO: BARCELONA X REAL MADRID | LALIGA 26/27 | 10ª RODADA", true, 176_000, None),
            stream("soon", "AO VIVO: FLUMINENSE X INTERNACIONAL | BRASILEIRÃO 2026 | 33ª RODADA", false, 17, Some(now + 26 * 3_600_000)),
        ];
        let c = cazetv_channels(&streams, now);
        assert_eq!(c.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["CazéTV", "CazéTV 2"]);
        assert_eq!(c[0].key, "cazetv-big");
        assert_eq!(c[0].url.as_deref(), Some("https://www.youtube.com/tv#/watch?v=big"));
        let on = &c[0].slots[0];
        assert_eq!((on.title.as_str(), on.sub.as_str(), on.kind), ("Barcelona x Real Madrid", "LaLiga · Round 10", Kind::Live));
        assert_eq!(on.teams, Some(("Barcelona".into(), "Real Madrid".into())));
        assert_eq!(on.start, now - 30 * 60_000);
        // The next scheduled stream follows the first channel's.
        assert_eq!(c[0].slots[1].title, "Fluminense x Internacional");
        assert_eq!(c[1].slots[0].sub, "Ligue 1 · Round 8 · Ambient sound");
        // Nothing live: one channel with the schedule.
        let later = cazetv_channels(&streams[2..], now);
        assert_eq!((later.len(), later[0].key.as_str()), (1, "cazetv"));
        assert_eq!((later[0].frame.as_deref(), later[0].viewers), (None, None));
        assert_eq!(c[0].viewers, Some(176_000));
    }

    #[test]
    fn news_programmes() {
        let now = Day { y: 2026, m: 10, d: 26 }.brt_ms(20, 30);
        let stream = |title: &str, live: bool| Stream {
            id: "x".into(),
            title: title.into(),
            live,
            viewers: 14_000,
            scheduled: (!live).then_some(now + 3_600_000),
            since: live.then_some(now - 30 * 60_000),
            thumb: None,
        };
        let title = |t: &str| news_slot(&stream(t, true), None, now).title;
        assert_eq!(title("AO VIVO: CNN PRIME TIME - 26/10/2026"), "CNN Prime Time");
        assert_eq!(title("🔴[AO VIVO] BandNews Station | 26/10/2026"), "BandNews Station");
        assert_eq!(title("🔴 Jornalismo Bandeirantes - Manhã - Programa de 26/10/2026"), "Jornalismo Bandeirantes - Manhã");
        assert_eq!(title("JORNAL JOVEM PAN - 26/10/26"), "Jornal Jovem Pan");
        assert_eq!(title("CNN NOVO DIA- 28/10/2026"), "CNN Novo Dia");
        assert_eq!(title("Jovem Pan News Av. Paulista | AO VIVO"), "Jovem Pan News Av. Paulista");
        assert_eq!(title("ACONTECENDO, COM LASIER MARTINS, 26/10/2026"), "Acontecendo, Com Lasier Martins");
        let band = news_slot(&stream("AO VIVO | Jornalismo Bandeirantes | MANHÃ (26/10/2026)", true), None, now);
        assert_eq!((band.title.as_str(), band.sub.as_str(), band.kind), ("Jornalismo Bandeirantes", "Manhã", Kind::Air));
        assert_eq!(title("CENTRAL de NOTÍCIAS, com Amanda Klein | 27/10/2026"), "Central de Notícias, com Amanda Klein");
        assert_eq!(title("Ao vivo: STF JULGA a CPI"), "STF Julga a CPI");
        let uol = news_slot(&stream("UOL News 1ª Edição ao vivo com Fabíola Cidral | 30 de outubro de 2026", true), None, now);
        assert_eq!((uol.title.as_str(), uol.sub.as_str()), ("UOL News 1ª Edição ao vivo com Fabíola Cidral", ""));

        // A radio's commentary of a match is that match.
        let radio = news_slot(&stream("AO VIVO: CORITIBA X GRÊMIO | BRASILEIRO 2026 | JORNADA DIGITAL | 26/10", true), None, now);
        assert_eq!((radio.title.as_str(), radio.kind), ("Coritiba x Grêmio", Kind::Live));
        assert_eq!(radio.teams, Some(("Coritiba".into(), "Grêmio".into())));
        assert_eq!(radio.sub, "Brasileiro · Jornada Digital");

        // On until the channel's next stream; a scheduled one from its start.
        let on = news_slot(&stream("JORNAL DA BAND", true), Some(now + 3_600_000), now);
        assert_eq!((on.start, on.end), (now - 30 * 60_000, now + 3_600_000));
        let next = news_slot(&stream("NEWS NOITE | 26/10/2026", false), None, now);
        assert_eq!((next.title.as_str(), next.start, next.end), ("News Noite", now + 3_600_000, now + 3_600_000 + NEWS_SHOW_MS));
    }
}
