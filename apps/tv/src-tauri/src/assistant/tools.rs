//! The assistant's tools that act on this TV (opening apps and games, titles, channels, volume,
//! media keys). The assistant itself runs on the home server, which sends each such call here
//! (`assistant.tool`); the cards, posters and UI steps a tool makes go back with its answer.

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use super::system::{self, MediaKey};
use super::{Card, Media, MediaKind, Ui};
use crate::config::AppEntry;
use crate::error::{Error, Result};
use crate::state::AppState;
use crate::{games, launcher, livetv, tmdb};

/// Posters per lookup: a TV row's worth without crowding the conversation.
const MAX_MEDIA: usize = 8;

/// Cards and UI steps gathered while the tools run.
#[derive(Default)]
pub struct Outcome {
    pub cards: Vec<Card>,
    pub media: Vec<Media>,
    pub ui: Vec<Ui>,
}

impl Outcome {
    /// Adds a poster, once.
    fn show(&mut self, m: Media) {
        if !self.media.iter().any(|x| x.id == m.id) {
            self.media.push(m);
        }
    }
}

/// Runs the server's `assistant.tool {name, args}` (args: the model's JSON text): the answer for
/// the model, and the cards, posters and UI steps it made.
pub async fn run(app: &AppHandle, name: &str, args: &str) -> Value {
    let mut out = Outcome::default();
    let result = match dispatch(app, name, args, &mut out).await {
        Ok(v) => v,
        Err(e) => json!({ "error": e.to_string() }),
    };
    json!({ "result": result, "cards": out.cards, "media": out.media, "ui": out.ui })
}

fn parse<T: DeserializeOwned>(args: &str) -> Result<T> {
    let args = if args.trim().is_empty() { "{}" } else { args };
    serde_json::from_str(args).map_err(|e| Error::msg(format!("bad arguments: {e}")))
}

async fn dispatch(app: &AppHandle, name: &str, args: &str, out: &mut Outcome) -> Result<Value> {
    match name {
        "open_app" => open_app(app, parse(args)?, out),
        "find_titles" => find_titles(app, parse(args)?, out).await,
        "trending_titles" => trending_titles(app, out).await,
        "find_games" => find_games(app, parse(args)?, out),
        "show_title" => show_title(app, parse(args)?, out).await,
        "close_app" => close_app(app, out),
        "watch_channel" => {
            let a: ChannelName = parse(args)?;
            let name = livetv::watch_named(app, &a.channel).await?;
            out.cards.push(Card::new("tv", &name, "Tuning in"));
            out.ui.push(Ui::Close);
            Ok(json!({ "playing": name }))
        }
        "add_news_channel" => {
            let a: Name = parse(args)?;
            let name = livetv::add_news_named(app, &a.name).await?;
            out.cards.push(Card::new("news", &name, "Added to the news channels"));
            Ok(json!({ "added": name, "note": "on the Live tab's News row whenever it's live" }))
        }
        "remove_news_channel" => {
            let a: Name = parse(args)?;
            let name = livetv::remove_news_named(app, &a.name)?;
            out.cards.push(Card::new("news", &name, "Removed from the news channels"));
            Ok(json!({ "removed": name }))
        }
        "set_volume" => set_volume(parse(args)?, out),
        "media" => media(parse(args)?, out),
        _ => Err(Error::msg(format!("this TV doesn't know the tool '{name}'"))),
    }
}

// ---------- Matching names ----------

/// Lowercase letters and digits, accents dropped, words separated by one space.
pub fn normalize(s: &str) -> String {
    let mapped: String = s
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c if c.is_alphanumeric() => c,
            '+' => '+',
            _ => ' ',
        })
        .collect();
    mapped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How well `name` answers to `query`: 0 (not at all) to 100 (same name).
pub fn score(query: &str, name: &str) -> u8 {
    let (q, n) = (normalize(query), normalize(name));
    if q.is_empty() || n.is_empty() {
        return 0;
    }
    if q == n {
        return 100;
    }
    if q.replace(' ', "") == n.replace(' ', "") {
        return 95;
    }
    if n.starts_with(&q) {
        return 80;
    }
    if n.contains(&q) {
        return 65;
    }
    if q.len() >= 3 && q.contains(&n) {
        return 55;
    }
    let words: Vec<&str> = n.split(' ').collect();
    if q.split(' ').all(|w| words.contains(&w)) {
        return 45;
    }
    0
}

/// The best scoring item, ties going to the earlier one.
fn best<'a, T>(query: &str, items: &'a [T], name: impl Fn(&T) -> &str) -> Option<(u8, &'a T)> {
    let mut found: Option<(u8, &T)> = None;
    for it in items {
        let s = score(query, name(it));
        if s > 0 && found.is_none_or(|(b, _)| s > b) {
            found = Some((s, it));
        }
    }
    found
}

// ---------- Apps and games ----------

#[derive(Deserialize)]
struct Name {
    name: String,
}

#[derive(Deserialize)]
struct ChannelName {
    channel: String,
}

fn open_app(app: &AppHandle, a: Name, out: &mut Outcome) -> Result<Value> {
    let st = app.state::<AppState>();
    let apps = st.config.read().unwrap().apps.clone();
    let library = games::library(app).games;
    let programs = system::programs();

    let hit_app = best(&a.name, &apps, |x| x.name.as_str())
        .into_iter()
        .chain(best(&a.name, &apps, |x| x.id.as_str()))
        .max_by_key(|(s, _)| *s);
    let installed: Vec<_> = library.iter().filter(|g| g.installed).cloned().collect();
    let hit_game = best(&a.name, &installed, |g| g.name.as_str());
    let hit_program = best(&a.name, &programs, |p| p.name.as_str());

    // Launcher apps first, then games, then other programs, unless one matches clearly better.
    let s_app = hit_app.map_or(0, |(s, _)| s);
    let s_game = hit_game.map_or(0, |(s, _)| s);
    let s_prog = hit_program.map_or(0, |(s, _)| s);
    if s_app > 0 && s_app >= s_game && s_app >= s_prog {
        let (_, entry) = hit_app.unwrap();
        launcher::launch(app, &entry.id, None, None)?;
        out.cards.push(Card::new("play", &entry.name, "Opening"));
        out.ui.push(Ui::Close);
        return Ok(json!({ "opened": entry.name, "kind": "streaming app" }));
    }
    if s_game > 0 && s_game >= s_prog {
        let (_, g) = hit_game.unwrap();
        show_game(g, out);
        return Ok(json!({ "shown": g.name, "kind": "game", "note": "on the Games screen; the user starts it" }));
    }
    if let Some((_, p)) = hit_program {
        app.opener()
            .open_path(p.path.to_string_lossy(), None::<&str>)
            .map_err(|e| Error::msg(e.to_string()))?;
        out.cards.push(Card::new("play", &p.name, "Opening"));
        return Ok(json!({ "opened": p.name, "kind": "program" }));
    }
    // Owned but not installed: say so rather than starting a download.
    if let Some((_, g)) = best(&a.name, &library, |g| g.name.as_str()) {
        show_game(g, out);
        return Ok(json!({
            "shown": g.name,
            "kind": "game",
            "note": "not installed; on the Games screen the user can install it",
        }));
    }
    Err(Error::msg(format!("nothing called '{}' found among apps, games or programs", a.name)))
}

fn close_app(app: &AppHandle, out: &mut Outcome) -> Result<Value> {
    let st = app.state::<AppState>();
    let running = st.running.lock().unwrap().as_ref().map(|r| r.app_id.clone());
    let Some(id) = running else {
        return Ok(json!({ "closed": null, "note": "no app was open" }));
    };
    let name = app_name(&st.config.read().unwrap().apps, &id);
    launcher::close(app);
    out.cards.push(Card::new("close", &name, "Closed"));
    Ok(json!({ "closed": name }))
}

fn app_name(apps: &[AppEntry], id: &str) -> String {
    apps.iter().find(|a| a.id == id).map_or_else(|| id.to_string(), |a| a.name.clone())
}

// ---------- Movies and series ----------
#[derive(Deserialize)]
struct TitleArg {
    title: String,
}

#[derive(Deserialize)]
struct Titles {
    titles: Vec<String>,
}

async fn find_titles(app: &AppHandle, a: Titles, out: &mut Outcome) -> Result<Value> {
    let st = app.state::<AppState>();
    let settings = st.config.read().unwrap().settings.clone();
    // The best match for each title, looked up side by side.
    let lookups = a.titles.iter().take(MAX_MEDIA).map(|t| tmdb::search(&st.http, &settings, t));
    let mut found = Vec::new();
    let mut first_error = None;
    for r in futures_util::future::join_all(lookups).await {
        match r {
            Ok(list) => found.extend(list.into_iter().next()),
            Err(e) => first_error = first_error.or(Some(e)),
        }
    }
    if let (true, Some(e)) = (found.is_empty(), first_error) {
        return Err(e);
    }
    describe_titles(app, found, out).await
}

async fn trending_titles(app: &AppHandle, out: &mut Outcome) -> Result<Value> {
    let st = app.state::<AppState>();
    let settings = st.config.read().unwrap().settings.clone();
    let mut list = tmdb::search(&st.http, &settings, "").await?;
    list.truncate(MAX_MEDIA);
    describe_titles(app, list, out).await
}

/// Where each title streams, as posters for the user and facts for the model.
async fn describe_titles(app: &AppHandle, titles: Vec<tmdb::Title>, out: &mut Outcome) -> Result<Value> {
    let st = app.state::<AppState>();
    let (settings, apps) = {
        let cfg = st.config.read().unwrap();
        (cfg.settings.clone(), cfg.apps.clone())
    };
    let providers = titles
        .iter()
        .map(|t| tmdb::providers(&st.http, &settings, &apps, &t.media_type, t.id));
    let providers = futures_util::future::join_all(providers).await;

    let mut facts = Vec::new();
    for (t, on) in titles.into_iter().zip(providers) {
        let on = on.unwrap_or_default();
        let series = t.media_type == "tv";
        let overview: String = t.overview.chars().take(300).collect();
        facts.push(json!({
            "title": t.title,
            "year": t.year,
            "type": if series { "series" } else { "movie" },
            "overview": overview,
            "on_user_apps": on.iter().map(|id| app_name(&apps, id)).collect::<Vec<_>>(),
        }));
        let kind = if series { "Series" } else { "Movie" };
        out.show(Media {
            id: format!("{}:{}", t.media_type, t.id),
            kind: if series { MediaKind::Tv } else { MediaKind::Movie },
            subtitle: t.year.as_ref().map_or_else(|| kind.to_string(), |y| format!("{y} · {kind}")),
            title: t.title,
            images: t.poster.into_iter().collect(),
            backdrop: t.backdrop,
            apps: on,
            installed: false,
        });
    }
    Ok(json!({ "results": facts }))
}

#[derive(Deserialize)]
struct GameQuery {
    query: Option<String>,
}

fn find_games(app: &AppHandle, a: GameQuery, out: &mut Outcome) -> Result<Value> {
    let mut library = games::library(app).games;
    match a.query.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => {
            let mut scored: Vec<_> = library
                .into_iter()
                .map(|g| (score(q, &g.name), g))
                .filter(|(s, _)| *s > 0)
                .collect();
            // Best match first; installed ahead of not installed on a tie.
            scored.sort_by_key(|(s, g)| (std::cmp::Reverse(*s), !g.installed));
            library = scored.into_iter().map(|(_, g)| g).collect();
        }
        None => library.sort_by_key(|g| (std::cmp::Reverse(g.last_played.unwrap_or(0)), !g.installed)),
    }
    library.truncate(MAX_MEDIA);
    let facts: Vec<Value> = library
        .iter()
        .map(|g| {
            json!({
                "name": g.name,
                "store": store_name(&g.id),
                "installed": g.installed,
                "hours_played": g.playtime_min.map(|m| m / 60),
            })
        })
        .collect();
    for g in library {
        out.show(game_media(g));
    }
    Ok(json!({ "games": facts }))
}

/// Goes to the Games screen with `g` selected, where the user plays or installs it.
fn show_game(g: &games::Game, out: &mut Outcome) {
    out.show(game_media(g.clone()));
    out.ui.push(Ui::Navigate { screen: "games".into(), query: None, pick: Some(g.id.clone()) });
}

fn store_name(id: &str) -> &'static str {
    if id.starts_with("epic:") {
        "Epic"
    } else {
        "Steam"
    }
}

fn game_media(g: games::Game) -> Media {
    let store = store_name(&g.id);
    let subtitle = match (g.installed, g.playtime_min) {
        (false, _) => format!("{store} · Not installed"),
        (true, Some(m)) if m >= 60 => format!("{store} · {} h played", m / 60),
        (true, _) => store.to_string(),
    };
    Media {
        id: g.id,
        kind: MediaKind::Game,
        title: g.name,
        subtitle,
        images: g.covers,
        backdrop: g.heroes.into_iter().next(),
        apps: Vec::new(),
        installed: g.installed,
    }
}

/// Opens the search screen on the title's best match, selected, with its "Open in" apps.
async fn show_title(app: &AppHandle, a: TitleArg, out: &mut Outcome) -> Result<Value> {
    let st = app.state::<AppState>();
    let settings = st.config.read().unwrap().settings.clone();
    let found = tmdb::search(&st.http, &settings, &a.title).await.ok().and_then(|t| t.into_iter().next());
    let Some(t) = found else {
        // No TMDB, or nothing found: the search screen with the words as given.
        out.ui.push(Ui::Navigate { screen: "search".into(), query: Some(a.title.clone()), pick: None });
        return Ok(json!({ "shown": a.title, "note": "not found on TMDB; showing the search screen" }));
    };
    let (query, pick) = (t.title.clone(), format!("{}:{}", t.media_type, t.id));
    let facts = describe_titles(app, vec![t], out).await?;
    out.ui.push(Ui::Navigate { screen: "search".into(), query: Some(query), pick: Some(pick) });
    Ok(facts)
}

// ---------- PC ----------

#[derive(Deserialize)]
struct VolumeArg {
    level: Option<i32>,
    change: Option<i32>,
    mute: Option<bool>,
}

fn set_volume(a: VolumeArg, out: &mut Outcome) -> Result<Value> {
    let v = system::volume(|v| {
        if let Some(l) = a.level {
            v.level = l.clamp(0, 100) as u8;
        }
        if let Some(c) = a.change {
            v.level = (i32::from(v.level) + c).clamp(0, 100) as u8;
        }
        match a.mute {
            Some(m) => v.muted = m,
            // Asking for a level means wanting to hear it.
            None if a.level.is_some() || a.change.is_some_and(|c| c > 0) => v.muted = false,
            None => {}
        }
    })?;
    let subtitle = if v.muted { "Muted" } else { "Volume" };
    out.cards.push(Card::new("volume", &format!("{}%", v.level), subtitle));
    Ok(json!({ "level": v.level, "muted": v.muted }))
}

#[derive(Deserialize)]
struct MediaArg {
    action: String,
}

fn media(a: MediaArg, out: &mut Outcome) -> Result<Value> {
    let (m, label) = match a.action.as_str() {
        "play_pause" => (MediaKey::PlayPause, "Play / pause"),
        "next" => (MediaKey::Next, "Next"),
        "previous" => (MediaKey::Previous, "Previous"),
        other => return Err(Error::msg(format!("unknown media action '{other}'"))),
    };
    system::media_key(m)?;
    out.cards.push(Card::new("play", label, "Media"));
    Ok(json!({ "ok": true }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_loosely() {
        assert_eq!(score("Netflix", "Netflix"), 100);
        assert_eq!(score("hbo max", "HBO Max"), 100);
        assert_eq!(score("hbomax", "HBO Max"), 95);
        assert_eq!(score("hades", "Hades II"), 80);
        assert_eq!(score("Pokémon", "pokemon"), 100);
        assert_eq!(score("ring", "Elden Ring"), 65);
        assert_eq!(score("open spotify please", "Spotify"), 55);
        assert_eq!(score("ring elden", "Elden Ring"), 45);
        assert_eq!(score("xyz", "Netflix"), 0);
        assert_eq!(score("", "Netflix"), 0);
    }

    #[test]
    fn best_prefers_higher_then_earlier() {
        let items = ["Hades", "Hades II", "Hollow Knight"];
        assert_eq!(best("hades", &items, |s| s).map(|(_, s)| *s), Some("Hades"));
        assert_eq!(best("hades ii", &items, |s| s).map(|(_, s)| *s), Some("Hades II"));
        assert_eq!(best("celeste", &items, |s| s), None);
    }

    #[test]
    fn game_posters_say_whether_installed() {
        let g = |id: &str, installed, playtime_min| games::Game {
            id: id.into(),
            store: games::Store::Steam,
            name: "Hades II".into(),
            installed,
            playtime_min,
            last_played: None,
            covers: vec!["c.jpg".into()],
            heroes: vec!["h.jpg".into()],
            favorite: false,
        };
        let m = game_media(g("steam:1145350", true, Some(150)));
        assert_eq!(m.subtitle, "Steam · 2 h played");
        assert_eq!(m.backdrop.as_deref(), Some("h.jpg"));
        assert!(m.installed);
        assert_eq!(game_media(g("steam:1", false, Some(150))).subtitle, "Steam · Not installed");
        assert_eq!(game_media(g("epic:Fortnite", true, None)).subtitle, "Epic");
    }

}
