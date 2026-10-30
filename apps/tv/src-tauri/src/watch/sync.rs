//! Learns what the user watched on the services outside the launcher (another TV, a phone):
//! a hidden browser on the launcher's signed-in profile opens the service's website and sync.js
//! reads it there. Netflix gives its viewing history (episode, position, date); every service
//! gives the Continue watching row on its home page. What it finds goes into the store when it's
//! newer than what the launcher saw (store.rs, apply_sync).
//!
//! Only while nothing else uses the profile: Play stops a sync on the spot (`abort`).

use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use super::session::parse_label;
use super::store::Synced;
use super::{ids, profiles, store};
use crate::cdp;
use crate::history::now_ms;
use crate::launcher;
use crate::state::AppState;
use crate::tmdb;

const SCRIPT: &str = include_str!("sync.js");

/// A service is read again at most this often.
const EVERY_MS: i64 = 10 * 60 * 1000;
/// Time for one service, from starting the browser to its answer.
const BUDGET: Duration = Duration::from_secs(90);
/// Pages render their rows after the load event.
const SETTLE: Duration = Duration::from_millis(3500);

/// The services the launcher has a copy of; the others have nothing to sync into.
const SERVICES: [&str; 5] = ["netflix", "prime", "disney", "hbomax", "globoplay"];

#[derive(Default)]
pub struct SyncState {
    /// The hidden browser, while one runs.
    pid: Mutex<Option<u32>>,
    /// When each service was last read.
    last: Mutex<HashMap<String, i64>>,
    /// Services asked for and not read yet, in order.
    queued: Mutex<Vec<String>>,
    busy: AtomicBool,
    cancel: AtomicBool,
}

/// Sent to the UI as "watch-sync" when a service's progress changed.
#[derive(Debug, Clone, Serialize)]
pub struct SyncEvent {
    pub service: String,
}

/// Reads `services` one after another, in the background; skips those read lately. Asked while
/// a read is under way, they are read after it.
pub fn request(app: &AppHandle, mut services: Vec<String>) {
    // The library's progress is all here: there's no site to read it from.
    services.retain(|s| s != super::library::SERVICE);
    let st = app.state::<AppState>();
    {
        let mut queued = st.watch.sync.queued.lock().unwrap();
        for s in services {
            if !queued.contains(&s) {
                queued.push(s);
            }
        }
    }
    if st.watch.sync.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app.state::<AppState>();
        loop {
            let batch = std::mem::take(&mut *st.watch.sync.queued.lock().unwrap());
            if batch.is_empty() {
                st.watch.sync.busy.store(false, Ordering::SeqCst);
                // A request that came in just now found it still busy: it's read here.
                if st.watch.sync.queued.lock().unwrap().is_empty() || st.watch.sync.busy.swap(true, Ordering::SeqCst) {
                    break;
                }
                continue;
            }
            read(&app, batch).await;
        }
    });
}

/// Reads one batch of requested services.
async fn read(app: &AppHandle, services: Vec<String>) {
    let st = app.state::<AppState>();
    st.watch.sync.cancel.store(false, Ordering::SeqCst);
    // A browser just closed may still hold the profile.
    while launcher::closing(&st) {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    for service in services.iter().filter(|s| SERVICES.contains(&s.as_str())) {
        if st.watch.sync.cancel.load(Ordering::SeqCst) || st.running.lock().unwrap().is_some() {
            break;
        }
        let due = {
            let mut last = st.watch.sync.last.lock().unwrap();
            let now = now_ms();
            let due = last.get(service).is_none_or(|&at| now - at >= EVERY_MS);
            if due {
                last.insert(service.clone(), now);
            }
            due
        };
        if !due {
            continue;
        }
        match tokio::time::timeout(BUDGET, sync_one(app, service)).await {
            Ok(Ok(true)) => {
                let _ = app.emit("watch-sync", SyncEvent { service: service.clone() });
            }
            Ok(Ok(false)) => {}
            Ok(Err(e)) => eprintln!("watch sync {service}: {e}"),
            Err(_) => eprintln!("watch sync {service}: took too long"),
        }
        stop_browser(&st);
    }
}

/// The next request reads `service` again however lately it was read (it plays on another
/// profile now).
pub fn read_again(st: &AppState, service: &str) {
    st.watch.sync.last.lock().unwrap().remove(service);
}

/// Play wants the profile: the hidden browser goes now.
pub fn abort(st: &AppState) {
    st.watch.sync.cancel.store(true, Ordering::SeqCst);
    stop_browser(st);
    kill_stale(st);
}

fn stop_browser(st: &AppState) {
    if let Some(pid) = st.watch.sync.pid.lock().unwrap().take() {
        launcher::kill_tree(pid);
    }
    let _ = std::fs::remove_file(pid_file(st));
}

/// Where the hidden browser's process id is kept while it runs. If the launcher went away
/// mid-sync (a crash, a restart), the browser is still holding the profile, and a browser
/// started on it later would hand its page to that invisible one instead of opening.
fn pid_file(st: &AppState) -> std::path::PathBuf {
    st.data_dir.join("sync-browser.pid")
}

/// Ends a hidden browser left behind by an earlier run, if that process is still a browser.
fn kill_stale(st: &AppState) {
    let file = pid_file(st);
    let Some(pid) = std::fs::read_to_string(&file).ok().and_then(|s| s.trim().parse::<u32>().ok()) else {
        return;
    };
    let _ = std::fs::remove_file(&file);
    let exe = launcher::browser_exe(&st.config.read().unwrap().settings);
    let image = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "msedge.exe".into());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/FI", &format!("PID eq {pid}"), "/FI", &format!("IMAGENAME eq {image}")])
            .creation_flags(0x0800_0000)
            .status();
    }
    #[cfg(not(windows))]
    let _ = (pid, image);
}

/// Reads one service. True when the store changed.
async fn sync_one(app: &AppHandle, service: &str) -> Result<bool, String> {
    let st = app.state::<AppState>();
    let (settings, entry) = {
        let cfg = st.config.read().unwrap();
        let entry = cfg.apps.iter().find(|a| a.id == service).cloned().ok_or("not in apps.toml")?;
        (cfg.settings.clone(), entry)
    };
    let home = entry.url.clone().ok_or("no address")?;
    // History belongs to a profile: the one Play uses, through its switch (which lands on home).
    let wanted = entry.profile.as_deref();
    let home = {
        let links = st.watch.profiles.lock().unwrap();
        links
            .get(profiles::DEFAULT, service, wanted)
            .and_then(|l| profiles::switch_url(service, &l.id))
            .unwrap_or(home)
    };

    kill_stale(&st);
    let port = cdp::free_port().ok_or("no free port")?;
    let profile = st.data_dir.join("browser-profile");
    let mut cmd = Command::new(launcher::browser_exe(&settings));
    cmd.args([
        "--headless=new".to_string(),
        format!("--user-data-dir={}", profile.display()),
        format!("--remote-debugging-port={port}"),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--mute-audio".into(),
        "--window-size=1920,1080".into(),
        "about:blank".into(),
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    *st.watch.sync.pid.lock().unwrap() = Some(child.id());
    let _ = std::fs::write(pid_file(&st), child.id().to_string());

    let ws = cdp::browser_ws(&st.http, port).await?;
    let user_agent = browser_user_agent(&st.http, port).await;
    let (stream, _) = tokio_tungstenite::connect_async(ws.as_str()).await.map_err(|e| e.to_string())?;
    let mut c = Client { ws: stream, next: 0, navigated: false };

    let targets = c.call(None, "Target.getTargets", json!({})).await?;
    let target = targets["targetInfos"]
        .as_array()
        .and_then(|l| l.iter().find(|t| t["type"] == "page"))
        .and_then(|t| t["targetId"].as_str())
        .ok_or("no page")?
        .to_string();
    let attached = c.call(None, "Target.attachToTarget", json!({ "targetId": target, "flatten": true })).await?;
    let session = attached["sessionId"].as_str().ok_or("no session")?.to_string();
    let s = Some(session.as_str());
    // A headless browser says so in its identity; the services shouldn't see a difference.
    if let Some(ua) = user_agent {
        c.call(s, "Emulation.setUserAgentOverride", json!({ "userAgent": ua.replace("HeadlessChrome", "Chrome") }))
            .await?;
    }
    c.call(s, "Page.enable", json!({})).await?;
    c.call(s, "Page.navigate", json!({ "url": home })).await?;

    // Home pages redirect (profile pickers, regional addresses) after they load: a read cut short
    // by a navigation is tried once more on the page it landed on.
    let mut found: Option<Vec<Entry>> = None;
    for _ in 0..2 {
        c.wait_loaded(s).await;
        if c.eval(s, SCRIPT).await.is_err() {
            continue;
        }
        if service == "netflix" {
            found = c.entries(s, service, "history", "window.__tvlSync.netflix()").await.filter(|l| !l.is_empty());
        }
        if found.is_none() {
            found = c.entries(s, service, "row", "window.__tvlSync.row()").await;
        }
        if found.is_some() || !c.navigated {
            break;
        }
    }
    // A clean exit, so the profile's cookies are saved; killed (stop_browser) only if it hangs.
    let _ = c.call(None, "Browser.close", json!({})).await;
    for _ in 0..25 {
        if child.try_wait().ok().flatten().is_some() {
            st.watch.sync.pid.lock().unwrap().take();
            let _ = std::fs::remove_file(pid_file(&st));
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let entries = found.unwrap_or_default();
    if entries.is_empty() {
        return Ok(false);
    }
    let synced = resolve_all(app, service, &entry.tmdb_providers, entries).await;
    eprintln!("watch sync {service}: {} matched on TMDB", synced.len());
    let now = now_ms();
    let mut store = st.watch.store.lock().unwrap();
    let mut changed = false;
    for s in synced {
        changed |= store.apply_sync(s, now);
    }
    if changed {
        store.save(&st.data_dir).map_err(|e| e.to_string())?;
    }
    Ok(changed)
}

async fn browser_user_agent(http: &reqwest::Client, port: u16) -> Option<String> {
    let v: Value = http
        .get(format!("http://127.0.0.1:{port}/json/version"))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    v["User-Agent"].as_str().map(str::to_string)
}

/// Just enough of DevTools for one page: call and wait for the answer.
struct Client {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next: u64,
    /// The last call failed because the page went to another address meanwhile.
    navigated: bool,
}

impl Client {
    async fn call(&mut self, session: Option<&str>, method: &str, params: Value) -> Result<Value, String> {
        self.next += 1;
        let id = self.next;
        let mut m = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            m["sessionId"] = json!(s);
        }
        self.ws.send(Message::Text(m.to_string().into())).await.map_err(|e| e.to_string())?;
        loop {
            match self.ws.next().await {
                Some(Ok(Message::Text(t))) => {
                    let Ok(v) = serde_json::from_str::<Value>(t.as_str()) else {
                        continue;
                    };
                    if v["id"].as_u64() == Some(id) {
                        if let Some(e) = v.get("error") {
                            self.navigated = e["message"].as_str().is_some_and(|m| m.contains("navigated"));
                            return Err(e.to_string());
                        }
                        self.navigated = false;
                        return Ok(v["result"].clone());
                    }
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e.to_string()),
                None => return Err("the browser went away".into()),
            }
        }
    }

    /// Runs `expression` in the page, waiting for a promise; the value it gives.
    async fn eval(&mut self, session: Option<&str>, expression: &str) -> Result<Value, String> {
        let r = self
            .call(
                session,
                "Runtime.evaluate",
                json!({ "expression": expression, "awaitPromise": true, "returnByValue": true }),
            )
            .await?;
        Ok(r["result"]["value"].clone())
    }

    /// A sync.js answer: its entries, or None (logged: why) when the page didn't give them.
    async fn entries(&mut self, session: Option<&str>, service: &str, source: &str, expression: &str) -> Option<Vec<Entry>> {
        let answer = match self.eval(session, expression).await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("watch sync {service} {source}: {e}");
                return None;
            }
        };
        let parsed: Value = answer.as_str().and_then(|t| serde_json::from_str(t).ok()).unwrap_or(Value::Null);
        if let Some(err) = parsed.get("error") {
            eprintln!("watch sync {service} {source}: {err}");
            return None;
        }
        match serde_json::from_value::<Vec<Entry>>(parsed) {
            Ok(list) => {
                eprintln!("watch sync {service} {source}: {} titles", list.len());
                Some(list)
            }
            Err(e) => {
                eprintln!("watch sync {service} {source}: unexpected answer ({e})");
                None
            }
        }
    }

    /// Until the page has loaded, then a moment for its rows to render.
    async fn wait_loaded(&mut self, session: Option<&str>) {
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let ready = self.eval(session, "document.readyState").await.ok();
            if ready.as_ref().and_then(Value::as_str) == Some("complete") {
                break;
            }
        }
        tokio::time::sleep(SETTLE).await;
    }
}

/// One title as sync.js found it.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub title: String,
    pub year: Option<String>,
    /// "tv" or "movie", when the service says.
    pub kind: Option<String>,
    /// The service's own id for the show or movie (Netflix).
    pub service_id: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    /// Seconds.
    pub position: Option<f64>,
    pub duration: Option<f64>,
    /// The bar's share, 0..1, when there are no seconds.
    pub share: Option<f64>,
    /// When it was watched, in milliseconds.
    pub at: Option<f64>,
    /// Episode text on the card ("T2:E5").
    pub label: Option<String>,
    #[serde(default)]
    pub episodes: Vec<(u32, u32, f64)>,
}

async fn resolve_all(app: &AppHandle, service: &str, providers: &[u32], entries: Vec<Entry>) -> Vec<Synced> {
    let st = app.state::<AppState>();
    let settings = st.config.read().unwrap().settings.clone();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (rank, e) in entries.into_iter().enumerate() {
        let Some((media, id)) = tmdb_title(app, service, providers, &e).await else {
            continue;
        };
        if !seen.insert((media.clone(), id)) {
            continue;
        }
        let Ok(details) = tmdb::details(&st.http, &settings, &media, id).await else {
            continue;
        };
        out.push(to_synced(service, &media, &details, &e, rank));
    }
    out
}

/// What the service's entry says, as the store takes it.
pub fn to_synced(service: &str, media: &str, details: &tmdb::Details, e: &Entry, rank: usize) -> Synced {
    let seasons = if media == "tv" { super::season_counts(details) } else { Vec::new() };
    let valid = |(s, ep): (u32, u32)| {
        seasons
            .get((s as usize).wrapping_sub(1))
            .is_some_and(|&n| ep >= 1 && ep <= n)
    };
    let (season, episode) = if media == "tv" {
        e.season
            .zip(e.episode)
            .or_else(|| e.label.as_deref().and_then(parse_label))
            .filter(|&se| valid(se))
            .unwrap_or((0, 0))
    } else {
        (0, 0)
    };
    let position = e.position.filter(|p| p.is_finite() && *p >= 0.0).unwrap_or(0.0);
    let duration = e.duration.filter(|d| d.is_finite() && *d > 0.0).unwrap_or(0.0);
    Synced {
        update: store::Update {
            service: service.to_string(),
            media_type: media.to_string(),
            id: details.title.id,
            title: details.title.title.clone(),
            poster: details.title.poster.clone(),
            backdrop: details.title.backdrop.clone(),
            seasons: seasons.clone(),
            season,
            episode,
            position: if duration > 0.0 { position } else { 0.0 },
            duration,
            share: e.share.map(|s| s.clamp(0.0, 1.0) as f32),
            confirmed: true,
        },
        watched_at: e.at.map(|a| a as i64).filter(|&a| a > 0),
        rank,
        episodes: e
            .episodes
            .iter()
            .filter(|&&(s, ep, _)| valid((s, ep)))
            .map(|&(s, ep, share)| (s, ep, share.clamp(0.0, 1.0) as f32))
            .collect(),
    }
}

/// The TMDB movie or series an entry is: through Wikidata for Netflix's ids, else by name among
/// what the service streams.
async fn tmdb_title(app: &AppHandle, service: &str, providers: &[u32], e: &Entry) -> Option<(String, u64)> {
    let st = app.state::<AppState>();
    if service == "netflix" {
        if let Some(id) = e.service_id.as_deref() {
            if let Some(hit) = ids::reverse(&st.http, &st.data_dir, "P1874", id).await {
                return Some(hit);
            }
        }
    }
    let settings = st.config.read().unwrap().settings.clone();
    for name in name_candidates(&e.title) {
        let Ok(results) = tmdb::search_on(&st.http, &settings, providers, &name).await else {
            continue;
        };
        if let Some(t) = pick(&results, &name, e) {
            return Some((t.media_type.clone(), t.id));
        }
    }
    None
}

/// The card's text and the parts of it that may be the title alone
/// ("Arcane, Season 2, Episode 3" → "Arcane").
pub fn name_candidates(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        let s = s.trim().trim_matches(|c: char| !c.is_alphanumeric() && c != '!' && c != '?').trim();
        if s.chars().count() >= 2 && !out.iter().any(|o| o == s) {
            out.push(s.to_string());
        }
    };
    push(text);
    for sep in [",", " - ", " – ", " | ", " · ", ":"] {
        if let Some((head, _)) = text.split_once(sep) {
            push(head);
        }
    }
    out
}

fn norm(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        let c = match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c => c,
        };
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with(' ') && !out.is_empty() {
            out.push(' ');
        }
    }
    out.trim().to_string()
}

/// The result named `name`: same title, of the kind and year the service gave when it gave them.
pub fn pick<'a>(results: &'a [tmdb::Title], name: &str, e: &Entry) -> Option<&'a tmdb::Title> {
    let want = norm(name);
    if want.is_empty() {
        return None;
    }
    results.iter().find(|t| {
        norm(&t.title) == want
            && e.kind.as_deref().is_none_or(|k| k == t.media_type)
            && e.year.as_deref().zip(t.year.as_deref()).is_none_or(|(a, b)| a == b)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn details(seasons: &[(u32, u32)]) -> tmdb::Details {
        tmdb::Details {
            title: tmdb::Title {
                id: 66732,
                media_type: "tv".into(),
                title: "Stranger Things".into(),
                year: Some("2016".into()),
                overview: String::new(),
                poster: None,
                backdrop: None,
            },
            tagline: None,
            genres: vec![],
            runtime: None,
            certification: None,
            logo: None,
            seasons: seasons
                .iter()
                .map(|&(number, episode_count)| tmdb::Season { number, name: String::new(), episode_count, year: None })
                .collect(),
            cast: vec![],
            trailer: None,
        }
    }

    fn title(id: u64, media: &str, name: &str, year: &str) -> tmdb::Title {
        tmdb::Title {
            id,
            media_type: media.into(),
            title: name.into(),
            year: Some(year.into()),
            overview: String::new(),
            poster: None,
            backdrop: None,
        }
    }

    #[test]
    fn netflix_history_entry() {
        let e: Entry = serde_json::from_str(
            r#"{"title":"Stranger Things","kind":"tv","serviceId":"80057281","season":2,"episode":5,
                "position":1200,"duration":3000,"at":1729000000000,"episodes":[[2,4,0.98],[9,1,0.5]]}"#,
        )
        .unwrap();
        let s = to_synced("netflix", "tv", &details(&[(1, 8), (2, 9)]), &e, 0);
        assert_eq!((s.update.season, s.update.episode), (2, 5));
        assert_eq!((s.update.position, s.update.duration), (1200.0, 3000.0));
        assert_eq!(s.watched_at, Some(1729000000000));
        // Episodes TMDB doesn't have are dropped.
        assert_eq!(s.episodes, vec![(2, 4, 0.98)]);
        assert_eq!(s.update.seasons, vec![8, 9]);
    }

    #[test]
    fn row_card_with_and_without_episode() {
        let e = Entry { title: "Stranger Things".into(), share: Some(0.35), label: Some("T2:E3".into()), ..Default::default() };
        let s = to_synced("hbomax", "tv", &details(&[(1, 8), (2, 9)]), &e, 3);
        assert_eq!((s.update.season, s.update.episode, s.rank), (2, 3, 3));
        assert_eq!(s.update.share, Some(0.35));
        assert_eq!(s.watched_at, None);

        let bare = Entry { title: "Stranger Things".into(), share: Some(0.35), ..Default::default() };
        let s = to_synced("hbomax", "tv", &details(&[(1, 8)]), &bare, 0);
        assert_eq!((s.update.season, s.update.episode), (0, 0));
        // An episode the series doesn't have counts as unknown.
        let wrong = Entry { title: "x".into(), label: Some("T5:E1".into()), ..Default::default() };
        let s = to_synced("hbomax", "tv", &details(&[(1, 8)]), &wrong, 0);
        assert_eq!((s.update.season, s.update.episode), (0, 0));
    }

    #[test]
    fn parses_row_answer() {
        // What sync.js's row() gave for a test page built like the services' rows.
        let list: Vec<Entry> = serde_json::from_str(
            r#"[{"title":"Arcane","share":0.4,"label":null,"href":"/watch/1"},
                {"title":"The Last of Us","share":0.25,"label":"T2:E3","href":"/watch/2"},
                {"title":"Duna: Parte Dois","share":null,"label":null,"href":null}]"#,
        )
        .unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[1].label.as_deref(), Some("T2:E3"));
        assert_eq!(list[2].share, None);
        assert!(list[0].episodes.is_empty());
    }

    #[test]
    fn candidates_from_card_text() {
        assert_eq!(
            name_candidates("Arcane, Temporada 2, Episódio 3"),
            vec!["Arcane, Temporada 2, Episódio 3", "Arcane"]
        );
        assert_eq!(name_candidates("Duna: Parte Dois"), vec!["Duna: Parte Dois", "Duna"]);
        assert_eq!(name_candidates("  "), Vec::<String>::new());
    }

    #[test]
    fn picks_the_same_name_kind_and_year() {
        let results = vec![
            title(1, "movie", "Duna", "1984"),
            title(2, "movie", "Duna: Parte Dois", "2024"),
            title(3, "tv", "Dúna", "2024"),
        ];
        let any = Entry::default();
        assert_eq!(pick(&results, "Duna: Parte Dois", &any).unwrap().id, 2);
        assert_eq!(pick(&results, "duna", &any).unwrap().id, 1);
        let series = Entry { kind: Some("tv".into()), ..Default::default() };
        assert_eq!(pick(&results, "Duna", &series).unwrap().id, 3);
        let dated = Entry { year: Some("2024".into()), ..Default::default() };
        assert_eq!(pick(&results, "Duna", &dated).unwrap().id, 3);
        assert!(pick(&results, "Dune", &any).is_none());
    }
}
