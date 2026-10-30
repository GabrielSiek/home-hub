//! Playing a title on a streaming service from the launcher's own copy of it, and keeping
//! track of how far the user got (the services expose no progress API).
//!
//! Play opens the service's page with watch.js in it. The launcher stays in front, showing
//! a curtain, until the video moves; then the browser is shown. While it plays, watch.js reports
//! the video and `Session` turns that into progress. When the player closes, the browser goes
//! away and the launcher is back on the details screen it was showing.

pub mod ids;
pub mod justwatch;
pub mod library;
pub mod profiles;
pub mod session;
pub mod store;
pub mod sync;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{Error, Result};
use crate::focus_win;
use crate::history::now_ms;
use crate::launcher;
use crate::state::{set_mode, AppState, Mode};
use crate::tmdb;

pub use session::Session;
pub use store::{ListItem, Progress, Store};

const SCRIPT: &str = include_str!("watch.js");

/// After this long without the video starting, the browser is shown anyway: the service is
/// probably asking for something (sign-in, a profile PIN) only the user can answer.
const REVEAL_AFTER: Duration = Duration::from_secs(40);
/// After a profile is picked from the header, the browser stays open behind the launcher at
/// most this long for watch.js to read the profile's id.
const PICK_WAIT: Duration = Duration::from_secs(10);
/// Progress is written to disk at most this often while playing (and always at the end).
const SAVE_EVERY: f64 = 10.0;

pub struct WatchState {
    pub store: Mutex<Store>,
    /// The service profile each launcher profile plays on.
    pub profiles: Mutex<profiles::Links>,
    live: Mutex<Option<Session>>,
    /// The service whose "Who's watching?" is open for the user to pick on (`pick_profile`).
    picking: Mutex<Option<String>>,
    /// The library's next episode, after the one playing.
    next: Mutex<Option<(u32, u32)>>,
    /// Tells a reveal timer whether its session is still the current one.
    nonce: AtomicU64,
    pub sync: sync::SyncState,
}

impl WatchState {
    pub fn new(dir: &std::path::Path) -> Self {
        Self {
            store: Mutex::new(Store::load(dir)),
            profiles: Mutex::new(profiles::Links::load(dir)),
            live: Mutex::new(None),
            picking: Mutex::new(None),
            next: Mutex::new(None),
            nonce: AtomicU64::new(0),
            sync: sync::SyncState::default(),
        }
    }
}

/// Sent to the UI as "watch".
#[derive(Debug, Clone, Serialize)]
pub struct WatchEvent {
    /// opening: the curtain is up; preparing: and the player is being fetched; playing: the browser
    /// (or the launcher's player) is shown; reveal: shown without the video having started; closed:
    /// back in the launcher.
    pub phase: &'static str,
    pub service: String,
    pub media_type: String,
    pub id: u64,
    pub title: String,
    pub backdrop: Option<String>,
    pub season: u32,
    pub episode: u32,
}

fn event(phase: &'static str, s: &Session) -> WatchEvent {
    WatchEvent {
        phase,
        service: s.service.clone(),
        media_type: s.media_type.clone(),
        id: s.id,
        title: s.title.clone(),
        backdrop: s.backdrop.clone(),
        season: s.season,
        episode: s.episode,
    }
}

fn now_secs() -> f64 {
    now_ms() as f64 / 1000.0
}

/// Episode counts by season number (index 0 is season 1); seasons TMDB skips count as empty.
fn season_counts(details: &tmdb::Details) -> Vec<u32> {
    let last = details.seasons.iter().map(|s| s.number).max().unwrap_or(0) as usize;
    let mut counts = vec![0; last];
    for s in &details.seasons {
        counts[s.number as usize - 1] = s.episode_count;
    }
    counts
}

/// What watch.js is told to do.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Plan {
    service: String,
    media_type: String,
    /// movie: press Play; resume: the series wherever the service left off; episode: this one.
    goal: &'static str,
    season: u32,
    episode: u32,
    episode_name: String,
    title: String,
    year: Option<String>,
    profile: Option<String>,
    /// The service's id for that profile, once learned. On a service without a switch address
    /// (Disney+), watch.js goes through the picker only when the page is on another profile.
    profile_id: Option<String>,
    /// The page opened is the service's profile switch: once it lands, on to this address.
    next: Option<String>,
    /// The page is the service's search, for the title: open the matching result first.
    search: bool,
}

/// Plays `id` on `service`: a given episode, or (none given) wherever the user is.
pub async fn play(
    app: &AppHandle,
    service: &str,
    media_type: &str,
    id: u64,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<()> {
    if service == library::SERVICE {
        return library::play(app, media_type, id, season, episode).await;
    }
    let pressed = now_secs();
    let st = app.state::<AppState>();
    let (settings, apps, entry) = {
        let cfg = st.config.read().unwrap();
        let entry = cfg
            .apps
            .iter()
            .find(|a| a.id == service)
            .cloned()
            .ok_or_else(|| Error::msg(format!("unknown app '{service}'")))?;
        (cfg.settings.clone(), cfg.apps.clone(), entry)
    };
    // A profile just picked from the header is still being read behind the launcher: Play opens
    // that one, not the one before it.
    while picked(&st) && now_secs() - pressed < PICK_WAIT.as_secs_f64() {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    end(app);

    let details = tmdb::details(&st.http, &settings, media_type, id).await?;
    let series = media_type == "tv";
    let seasons = if series { season_counts(&details) } else { Vec::new() };
    let picked = season.zip(episode);
    let first = (details.seasons.first().map_or(1, |x| x.number), 1);
    // Resuming plays wherever the service left off: the episode is only known if the store knows it.
    let (s, e, confirmed) = if !series {
        (0, 0, true)
    } else if let Some((s, e)) = picked {
        (s, e, true)
    } else {
        let store = st.watch.store.lock().unwrap();
        store
            .get(service, media_type, id)
            .filter(|p| !p.finished && p.season > 0)
            .map(|p| (p.season, p.episode, p.confirmed))
            .unwrap_or((first.0, first.1, false))
    };

    let mut session = Session::new(
        service.to_string(),
        media_type.to_string(),
        id,
        details.title.title.clone(),
        details.title.poster.clone(),
        details.title.backdrop.clone(),
        seasons,
        s,
        e,
    );
    session.runtime = details.runtime.unwrap_or(0) as f64 * 60.0;
    session.confirmed = confirmed;
    session.opened_at = pressed;
    // Curtain first: the lookups below take a moment.
    let _ = app.emit("watch", event("opening", &session));

    // JustWatch follows the region's catalog; Wikidata fills in what it lacks; last, what a
    // search on the service itself found before. The season's episodes are fetched meanwhile.
    let lookups_from = now_secs();
    let episodes = async {
        if series {
            tmdb::season(&st.http, &settings, id, s).await.ok()
        } else {
            None
        }
    };
    let (eps, jw, wd) = tokio::join!(
        episodes,
        justwatch::lookup(&st.http, &st.data_dir, &settings, &apps, media_type, id, &details.title.title),
        ids::lookup(&st.http, &st.data_dir, media_type, id),
    );
    eprintln!("watch: {service} lookups took {:.1}s", now_secs() - lookups_from);

    let mut episode_name = String::new();
    if let Some(eps) = eps {
        session.runtimes = eps
            .iter()
            .filter_map(|x| x.runtime.map(|r| ((x.season, x.number), r as f64 * 60.0)))
            .collect::<HashMap<_, _>>();
        episode_name = eps.iter().find(|x| x.number == e).map(|x| x.name.clone()).unwrap_or_default();
    }
    // What the user's own way through a search played before comes first: it is known to work.
    let learned = ids::learned(&st.data_dir, media_type, id);
    let direct = [learned, jw, wd]
        .iter()
        .find_map(|ids| ids::deep_link(service, media_type, ids, picked.is_some()));
    let search = direct.is_none();
    let url = match direct {
        Some(u) => u,
        None => launcher::target_url(&entry, Some(&details.title.title))
            .ok_or_else(|| Error::msg(format!("{} has no address to open", entry.name)))?,
    };
    // A learned profile opens straight away: its switch first, then the title.
    let wanted = entry.profile.clone().filter(|p| !p.trim().is_empty());
    let link = st.watch.profiles.lock().unwrap().get(profiles::DEFAULT, service, wanted.as_deref()).cloned();
    let switch = link.as_ref().and_then(|l| profiles::switch_url(service, &l.id));
    let (url, next) = match switch {
        Some(sw) => (sw, Some(url)),
        None => (url, None),
    };
    let plan = Plan {
        service: service.to_string(),
        media_type: media_type.to_string(),
        goal: if !series {
            "movie"
        } else if picked.is_some() {
            "episode"
        } else {
            "resume"
        },
        season: s,
        episode: e,
        episode_name,
        title: details.title.title.clone(),
        year: details.title.year.clone(),
        // The name is still the way in when the switch lands on the picker after all.
        profile: wanted.or_else(|| link.as_ref().map(|l| l.name.clone())),
        profile_id: link.map(|l| l.id),
        next,
        search,
    };
    let script = format!("window.__tvlWatch = {};\n{SCRIPT}", json!(plan));

    // The launcher stays on top of the browser until the video plays.
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_always_on_top(true);
    }
    if let Err(err) = launcher::launch_web(app, service, &url, Some(script)) {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.set_always_on_top(false);
        }
        let _ = app.emit("watch", event("closed", &session));
        return Err(err);
    }
    *st.watch.live.lock().unwrap() = Some(session);

    let nonce = st.watch.nonce.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(REVEAL_AFTER).await;
        let st = app.state::<AppState>();
        if st.watch.nonce.load(Ordering::SeqCst) == nonce {
            reveal(&app);
        }
    });
    Ok(())
}

/// Opens the service's "Who's watching?" for the user to pick, with the pad, the profile Play
/// uses from now on. The launcher is back as soon as the user picks (on_picked); the browser
/// closes behind it once the pick is learned (on_profile).
pub fn pick_profile(app: &AppHandle, service: &str) -> Result<()> {
    let url = profiles::picker_url(service).ok_or_else(|| Error::msg(format!("{service} has no profiles to pick")))?;
    end(app);
    let st = app.state::<AppState>();
    let script = format!("window.__tvlWatch = {};
{SCRIPT}", json!({ "service": service, "goal": "profile" }));
    *st.watch.picking.lock().unwrap() = Some(service.to_string());
    if let Err(err) = launcher::launch_web(app, service, url, Some(script)) {
        st.watch.picking.lock().unwrap().take();
        return Err(err);
    }
    Ok(())
}

/// The service profile Play opens, for the header; whether the service has profiles to pick.
#[derive(Debug, Serialize)]
pub struct ProfileState {
    pub name: Option<String>,
    pub pickable: bool,
}

pub fn profile_state(st: &AppState, service: &str) -> ProfileState {
    let wanted = {
        let cfg = st.config.read().unwrap();
        cfg.apps.iter().find(|a| a.id == service).and_then(|a| a.profile.clone())
    };
    let wanted = wanted.filter(|p| !p.trim().is_empty());
    let links = st.watch.profiles.lock().unwrap();
    let name = links.get(profiles::DEFAULT, service, wanted.as_deref()).map(|l| l.name.clone()).or(wanted);
    ProfileState { name, pickable: profiles::picker_url(service).is_some() }
}

/// A profile was picked from the header, and the launcher is in front while its id is read.
fn picked(st: &AppState) -> bool {
    st.watch.picking.lock().unwrap().is_some() && *st.mode.lock().unwrap() == Mode::Launcher
}

/// The user picked on the header's "Who's watching?": the launcher comes back in front at once,
/// with the pad. The browser stays open behind it until watch.js has read the profile's id
/// (on_profile closes it), or PICK_WAIT is over.
fn on_picked(app: &AppHandle) {
    let st = app.state::<AppState>();
    if st.watch.picking.lock().unwrap().is_none() {
        return;
    }
    focus_win::bring_to_front(app);
    // The service's pages still load behind it.
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_always_on_top(true);
    }
    set_mode(app, Mode::Launcher);
    let nonce = st.watch.nonce.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(PICK_WAIT).await;
        if app.state::<AppState>().watch.nonce.load(Ordering::SeqCst) == nonce {
            eprintln!("watch: no profile read {}s after the pick", PICK_WAIT.as_secs());
            close_soon(&app);
        }
    });
}

/// Shows the browser before the video started: the page needs the user (a profile to pick,
/// a PIN, a sign-in).
fn reveal(app: &AppHandle) {
    let st = app.state::<AppState>();
    // Picking from the header: the profile picked asks for its PIN.
    if st.watch.picking.lock().unwrap().is_some() {
        st.watch.nonce.fetch_add(1, Ordering::SeqCst);
        set_mode(app, Mode::Streaming);
        step_aside(app);
        return;
    }
    let waiting = st.watch.live.lock().unwrap().as_ref().filter(|s| !s.started()).map(|s| event("reveal", s));
    if let Some(ev) = waiting {
        step_aside(app);
        let _ = app.emit("watch", ev);
    }
}

/// Takes the curtain down: the launcher steps aside so the browser shows.
fn step_aside(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    let _ = w.set_always_on_top(false);
    // Hidden rather than sent back: Windows won't hand the foreground to the browser otherwise.
    // Coming back (focus_win::bring_to_front) shows it again.
    let _ = w.hide();
}

#[derive(Deserialize)]
struct Report {
    kind: String,
    /// found: the title's page that a search led to.
    #[serde(default)]
    url: String,
    /// profile: the service profile the video plays on, by the service's id for it.
    #[serde(default)]
    id: String,
    /// profile: the name picked on "Who's watching?" this time, if it was asked.
    name: Option<String>,
    /// profile: the one the page was on before the header's pick.
    from: Option<String>,
    /// played: the addresses the tab went through from the search to the video, oldest first.
    #[serde(default)]
    trail: Vec<String>,
}

/// A report from watch.js (see cdp.rs, which collects them).
pub fn on_report(app: &AppHandle, text: &str) {
    let Ok(r) = serde_json::from_str::<Report>(text) else {
        return;
    };
    if r.kind == "found" {
        let st = app.state::<AppState>();
        let live = st.watch.live.lock().unwrap();
        if let Some(s) = live.as_ref() {
            ids::learn(&st.data_dir, &s.service, &s.media_type, s.id, &r.url);
        }
        return;
    }
    // The video plays after a search, whoever clicked the way there (the script or the pad).
    if r.kind == "played" {
        let st = app.state::<AppState>();
        let live = st.watch.live.lock().unwrap();
        if let Some(s) = live.as_ref() {
            match ids::learn_trail(&st.data_dir, &s.service, &s.media_type, s.id, &r.trail) {
                Some(url) => eprintln!("watch: {} {} {} plays from {url} next time", s.service, s.media_type, s.id),
                None => eprintln!("watch: {} played, but no title address in {:?}", s.service, r.trail),
            }
        }
        return;
    }
    if r.kind == "reveal" {
        reveal(app);
        return;
    }
    if r.kind == "picked" {
        on_picked(app);
        return;
    }
    if r.kind == "profile" {
        on_profile(app, r.id, r.name, r.from);
        return;
    }
    if r.kind != "sample" {
        return;
    }
    let Ok(sample) = serde_json::from_str::<session::Sample>(text) else {
        return;
    };
    let (outcome, started) = take_sample(app, sample);
    if let Some(ev) = started {
        step_aside(app);
        let _ = app.emit("watch", ev);
    }
    if outcome == session::Outcome::Close {
        close_soon(app);
    }
}

/// A sample of what's playing: the session moves on and, every so often, the store. Says what to
/// do next, and the event to send if the video just started.
fn take_sample(app: &AppHandle, sample: session::Sample) -> (session::Outcome, Option<WatchEvent>) {
    let st = app.state::<AppState>();
    let now = now_secs();
    let (outcome, started, update) = {
        let mut live = st.watch.live.lock().unwrap();
        let Some(s) = live.as_mut() else {
            return (session::Outcome::Nothing, None);
        };
        let outcome = s.on_sample(sample, now);
        if outcome == session::Outcome::Started {
            eprintln!("watch: {} playing {:.1}s after Play", s.service, now - s.opened_at);
        }
        let started = (outcome == session::Outcome::Started).then(|| event("playing", s));
        let due = now - s.saved_at >= SAVE_EVERY;
        let update = if due { s.update() } else { None };
        if update.is_some() {
            s.saved_at = now;
        }
        (outcome, started, update)
    };
    if let Some(u) = update {
        let mut store = st.watch.store.lock().unwrap();
        store.record(u, now_ms());
        let _ = store.save(&st.data_dir);
    }
    (outcome, started)
}

/// The profile a video plays on: a pick on "Who's watching?" is kept for next time; without one,
/// it should be the profile Play switched to. Picking another profile than the linked one puts
/// the old one's progress aside (store.rs, switch_profile) and has the service read again.
fn on_profile(app: &AppHandle, id: String, name: Option<String>, from: Option<String>) {
    let st = app.state::<AppState>();
    let playing = st.watch.live.lock().unwrap().as_ref().map(|s| s.service.clone());
    let picking = st.watch.picking.lock().unwrap().clone();
    let Some(service) = playing.or(picking.clone()) else {
        return;
    };
    if id.is_empty() {
        return;
    }
    let switched = {
        let mut links = st.watch.profiles.lock().unwrap();
        match name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
            Some(name) => {
                eprintln!("watch: {service} profile '{name}' is {id}");
                // None learned yet: the profile the page was on is the one the store has.
                let before = links.get(profiles::DEFAULT, &service, None).map(|l| l.id.clone()).or(from);
                if links.set(profiles::DEFAULT, &service, profiles::Link { id: id.clone(), name }) {
                    let _ = links.save(&st.data_dir);
                }
                before.filter(|b| *b != id)
            }
            None => {
                if let Some(linked) = links.get(profiles::DEFAULT, &service, None).filter(|l| l.id != id) {
                    eprintln!("watch: {service} plays on profile {id}, not the linked {}", linked.id);
                }
                None
            }
        }
    };
    if let Some(from) = switched {
        eprintln!("watch: {service} moved from profile {from} to {id}");
        {
            let mut store = st.watch.store.lock().unwrap();
            store.switch_profile(&service, &from, &id);
            let _ = store.save(&st.data_dir);
        }
        sync::read_again(&st, &service);
        let _ = app.emit("watch-sync", sync::SyncEvent { service: service.clone() });
    }
    // Picked from the header: that was all, back to the launcher.
    if picking.is_some() {
        close_soon(app);
    }
}

/// Closes the browser and comes back, off the caller's thread (closing waits for the browser).
pub fn close_soon(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || launcher::close(&app));
}

/// Ends the current session, if there is one: saves where it got to and tells the UI.
/// Whether a title is playing (or opening) from the launcher's own service screens.
pub fn active(app: &AppHandle) -> bool {
    app.state::<AppState>().watch.live.lock().unwrap().is_some()
}

/// What plays from the launcher's copy of a service right now, for the companion's "On the TV".
pub fn playing(app: &AppHandle) -> Option<WatchEvent> {
    app.state::<AppState>().watch.live.lock().unwrap().as_ref().map(|s| event("playing", s))
}

pub fn end(app: &AppHandle) {
    let st = app.state::<AppState>();
    st.watch.nonce.fetch_add(1, Ordering::SeqCst);
    if let Some(service) = st.watch.picking.lock().unwrap().take() {
        // Back from picking: the service's screens load its profile (and progress) again.
        let _ = app.emit("watch-sync", sync::SyncEvent { service });
    }
    let Some(s) = st.watch.live.lock().unwrap().take() else {
        return;
    };
    if let Some(u) = s.update() {
        let mut store = st.watch.store.lock().unwrap();
        store.record(u, now_ms());
        let _ = store.save(&st.data_dir);
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_always_on_top(false);
    }
    let _ = app.emit("watch", event("closed", &s));
}

/// A title's progress on `service`, and whether it is in My list.
#[derive(Debug, Serialize)]
pub struct TitleState {
    pub progress: Option<Progress>,
    pub listed: bool,
}

pub fn title_state(st: &AppState, service: &str, media_type: &str, id: u64) -> TitleState {
    let store = st.watch.store.lock().unwrap();
    TitleState {
        progress: store.get(service, media_type, id).cloned(),
        listed: store.listed(service, media_type, id),
    }
}

pub fn continue_watching(st: &AppState, service: Option<&str>) -> Vec<Progress> {
    st.watch.store.lock().unwrap().continue_watching(service)
}

pub fn dismiss(st: &AppState, service: &str, media_type: &str, id: u64) -> Result<()> {
    let mut store = st.watch.store.lock().unwrap();
    store.dismiss(service, media_type, id, now_ms());
    store.save(&st.data_dir)
}

pub fn watchlist(st: &AppState, service: Option<&str>) -> Vec<ListItem> {
    st.watch.store.lock().unwrap().list(service)
}

pub fn toggle_listed(st: &AppState, mut item: ListItem) -> Result<bool> {
    item.added_at = now_ms();
    let mut store = st.watch.store.lock().unwrap();
    let on = store.toggle_listed(item);
    store.save(&st.data_dir)?;
    Ok(on)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn season_counts_by_number() {
        let d = tmdb::Details {
            title: tmdb::Title {
                id: 1,
                media_type: "tv".into(),
                title: "X".into(),
                year: None,
                overview: String::new(),
                poster: None,
                backdrop: None,
            },
            tagline: None,
            genres: vec![],
            runtime: None,
            certification: None,
            logo: None,
            seasons: vec![
                tmdb::Season { number: 1, name: String::new(), episode_count: 8, year: None },
                tmdb::Season { number: 3, name: String::new(), episode_count: 6, year: None },
            ],
            cast: vec![],
            trailer: None,
        };
        assert_eq!(season_counts(&d), vec![8, 0, 6]);
    }

    #[test]
    fn plan_is_camel_case_json() {
        let p = Plan {
            service: "netflix".into(),
            media_type: "tv".into(),
            goal: "episode",
            season: 1,
            episode: 3,
            episode_name: "Passado".into(),
            title: "Dark".into(),
            year: Some("2017".into()),
            profile: None,
            profile_id: Some("ABC".into()),
            next: Some("https://www.netflix.com/watch/1".into()),
            search: true,
        };
        let v = json!(p);
        assert_eq!(v["episodeName"], "Passado");
        assert_eq!(v["mediaType"], "tv");
        assert_eq!(v["search"], true);
        assert_eq!(v["next"], "https://www.netflix.com/watch/1");
        assert_eq!(v["profileId"], "ABC");
    }
}
