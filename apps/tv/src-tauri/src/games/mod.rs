//! The game library: Steam (owned or installed) and Epic (installed), launching them,
//! and noticing when a game starts or exits so the controller goes to the right place.

pub mod epic;
pub mod sgdb;
pub mod steam;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::config::Settings;
use crate::error::{Error, Result};
use crate::focus_win;
use crate::state::{set_mode, AppState, Mode};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const OWNED_CACHE: &str = "steam-owned.json";
const PREFS_FILE: &str = "games.json";
const ART_DIR: &str = "art";
/// How often the owned library and missing art are fetched again.
const REFRESH_EVERY: Duration = Duration::from_secs(10 * 60);
const WATCH_EVERY: Duration = Duration::from_millis(1500);
/// How long an Epic game may take to show up as a process before we stop waiting.
const EPIC_START_TIMEOUT: Duration = Duration::from_secs(120);
const STEAM_COVER: &str = "library_600x900.jpg";
const STEAM_HERO: &str = "library_hero.jpg";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Store {
    Steam,
    Epic,
}

#[derive(Debug, Clone, Serialize)]
pub struct Game {
    /// "steam:<appid>" or "epic:<AppName>".
    pub id: String,
    pub store: Store,
    pub name: String,
    pub installed: bool,
    pub playtime_min: Option<u32>,
    /// Unix seconds.
    pub last_played: Option<i64>,
    /// Image candidates, best first: absolute file paths or URLs. The UI tries them in order.
    pub covers: Vec<String>,
    pub heroes: Vec<String>,
    pub favorite: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Library {
    pub games: Vec<Game>,
    pub steam_found: bool,
    /// The whole owned library is listed (Steam Web API key set and answering).
    pub steam_owned: bool,
    pub epic_found: bool,
    /// SteamGridDB key set: missing covers are being looked up.
    pub art_lookup: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Prefs {
    #[serde(default)]
    favorites: Vec<String>,
}

struct EpicLaunch {
    id: String,
    exe: String,
    started: Instant,
    seen: bool,
}

pub struct GamesState {
    prefs: Mutex<Prefs>,
    refreshing: AtomicBool,
    last_refresh: Mutex<Option<Instant>>,
    /// The game currently running, if any (also while the launcher is in front of it).
    running: Mutex<Option<String>>,
    epic_launch: Mutex<Option<EpicLaunch>>,
}

impl GamesState {
    pub fn new(data_dir: &Path) -> Self {
        let prefs = fs::read_to_string(data_dir.join(PREFS_FILE))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            prefs: Mutex::new(prefs),
            refreshing: AtomicBool::new(false),
            last_refresh: Mutex::new(None),
            running: Mutex::new(None),
            epic_launch: Mutex::new(None),
        }
    }

    pub fn running(&self) -> Option<String> {
        self.running.lock().unwrap().clone()
    }
}

/// Lets the webview load Steam's cached art straight from its install folder.
pub fn allow_steam_art(app: &AppHandle) {
    if let Some(root) = steam::root() {
        let _ = app
            .asset_protocol_scope()
            .allow_directory(root.join("appcache/librarycache"), true);
    }
}

/// The library from what is on disk (Steam files, cached owned list, cached art).
/// Also starts a background refresh when one is due.
pub fn library(app: &AppHandle) -> Library {
    let st = app.state::<AppState>();
    let due = st
        .games
        .last_refresh
        .lock()
        .unwrap()
        .is_none_or(|t| t.elapsed() > REFRESH_EVERY);
    if due {
        tauri::async_runtime::spawn(refresh(app.clone()));
    }
    let favorites = st.games.prefs.lock().unwrap().favorites.clone();
    let settings = st.config.read().unwrap().settings.clone();
    build(&st.data_dir, &favorites, &settings)
}

/// Fetch the owned list and missing art on the next `library` call, e.g. after keys change.
pub fn invalidate(app: &AppHandle) {
    *app.state::<AppState>().games.last_refresh.lock().unwrap() = None;
    let _ = app.emit("games-updated", ());
}

fn build(data_dir: &Path, favorites: &[String], settings: &Settings) -> Library {
    let mut games = Vec::new();
    let root = steam::root();
    let owned: Option<Vec<steam::Owned>> = fs::read_to_string(data_dir.join(OWNED_CACHE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());

    if let Some(root) = &root {
        let played = steam::account(root, settings.steam_id())
            .map(|id| steam::local_played(root, id))
            .unwrap_or_default();
        let installed = steam::installed(root);
        let entries = merge_steam(owned.as_deref().unwrap_or_default(), &installed, &played);
        for (appid, e) in entries {
            let id = format!("steam:{appid}");
            let local = |file| steam::local_art(root, appid, file).map(|p| p.to_string_lossy().into_owned());
            games.push(Game {
                favorite: favorites.contains(&id),
                id,
                store: Store::Steam,
                name: e.name,
                installed: e.installed,
                playtime_min: (e.minutes > 0).then_some(e.minutes),
                last_played: (e.last > 0).then_some(e.last),
                covers: local(STEAM_COVER).into_iter().chain([steam::cdn(appid, STEAM_COVER)]).collect(),
                heroes: local(STEAM_HERO).into_iter().chain([steam::cdn(appid, STEAM_HERO)]).collect(),
            });
        }
    }

    let epic_dir = epic::manifests_dir();
    for item in epic::installed() {
        let id = format!("epic:{}", item.app_name);
        let art = |kind| cached_art(data_dir, &id, kind).map(|p| p.to_string_lossy().into_owned());
        games.push(Game {
            favorite: favorites.contains(&id),
            covers: art(sgdb::Kind::Cover).into_iter().collect(),
            heroes: art(sgdb::Kind::Hero).into_iter().collect(),
            id,
            store: Store::Epic,
            name: item.display_name,
            installed: true,
            playtime_min: None,
            last_played: None,
        });
    }

    Library {
        games,
        steam_found: root.is_some(),
        steam_owned: owned.is_some_and(|o| !o.is_empty()),
        epic_found: epic_dir.is_dir(),
        art_lookup: settings.steamgriddb_key().is_some(),
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
struct SteamEntry {
    name: String,
    installed: bool,
    minutes: u32,
    last: i64,
}

/// Owned games plus installed ones (free games never played aren't "owned"), with play
/// history from whichever source knows more.
fn merge_steam(
    owned: &[steam::Owned],
    installed: &[steam::Installed],
    played: &std::collections::HashMap<u32, steam::Played>,
) -> BTreeMap<u32, SteamEntry> {
    let mut out: BTreeMap<u32, SteamEntry> = BTreeMap::new();
    for o in owned {
        let e = out.entry(o.appid).or_default();
        e.name = o.name.clone();
        e.minutes = o.playtime_forever;
        e.last = o.rtime_last_played;
    }
    for i in installed {
        let e = out.entry(i.appid).or_default();
        e.installed = true;
        if e.name.is_empty() {
            e.name = i.name.clone();
        }
    }
    for (appid, e) in out.iter_mut() {
        if let Some(p) = played.get(appid) {
            e.minutes = e.minutes.max(p.minutes);
            e.last = e.last.max(p.last);
        }
    }
    out.retain(|_, e| !e.name.is_empty() && !steam::is_tool(&e.name));
    out
}

fn art_stem(data_dir: &Path, id: &str, kind: sgdb::Kind) -> PathBuf {
    let safe: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    data_dir.join(ART_DIR).join(format!("{safe}-{}", kind.name()))
}

fn cached_art(data_dir: &Path, id: &str, kind: sgdb::Kind) -> Option<PathBuf> {
    let stem = art_stem(data_dir, id, kind);
    ["png", "jpg", "jpeg", "webp"]
        .iter()
        .map(|ext| stem.with_extension(ext))
        .find(|p| p.is_file())
}

/// Fetches the owned Steam library and missing Epic art; tells the UI when anything changed.
async fn refresh(app: AppHandle) {
    let st = app.state::<AppState>();
    if st.games.refreshing.swap(true, Ordering::SeqCst) {
        return;
    }
    let mut changed = false;
    let settings = st.config.read().unwrap().settings.clone();

    if let (Some(key), Some(root)) = (settings.steam_key(), steam::root()) {
        if let Some(account) = steam::account(&root, settings.steam_id()) {
            match steam::fetch_owned(&st.http, &key, account).await {
                Ok(owned) if !owned.is_empty() => {
                    let path = st.data_dir.join(OWNED_CACHE);
                    let json = serde_json::to_string(&owned).unwrap_or_default();
                    if fs::read_to_string(&path).ok().as_deref() != Some(json.as_str()) {
                        changed |= fs::write(&path, json).is_ok();
                    }
                }
                Ok(_) => eprintln!("Steam returned no games: is the profile's game list private?"),
                Err(e) => eprintln!("Steam library refresh failed: {e}"),
            }
        }
    }

    if let Some(key) = settings.steamgriddb_key() {
        let _ = fs::create_dir_all(st.data_dir.join(ART_DIR));
        for item in epic::installed() {
            let id = format!("epic:{}", item.app_name);
            changed |= fetch_art(&st, &key, &id, &item.display_name).await;
        }
    }

    *st.games.last_refresh.lock().unwrap() = Some(Instant::now());
    st.games.refreshing.store(false, Ordering::SeqCst);
    if changed {
        let _ = app.emit("games-updated", ());
    }
}

/// Downloads a game's missing cover and hero from SteamGridDB. A `.none` marker remembers
/// art that doesn't exist so it isn't asked for again. Returns whether a file was added.
async fn fetch_art(st: &AppState, key: &str, id: &str, name: &str) -> bool {
    let missing: Vec<_> = [sgdb::Kind::Cover, sgdb::Kind::Hero]
        .into_iter()
        .filter(|&k| {
            cached_art(&st.data_dir, id, k).is_none()
                && !art_stem(&st.data_dir, id, k).with_extension("none").exists()
        })
        .collect();
    if missing.is_empty() {
        return false;
    }
    let game = match sgdb::find(&st.http, key, name).await {
        Ok(g) => g,
        Err(e) => {
            eprintln!("SteamGridDB search for {name} failed: {e}");
            return false;
        }
    };
    let mut added = false;
    for kind in missing {
        let stem = art_stem(&st.data_dir, id, kind);
        let url = match game {
            Some(g) => sgdb::image_url(&st.http, key, g, kind).await,
            None => Ok(None),
        };
        match url {
            Ok(Some(url)) => added |= sgdb::download(&st.http, &url, &stem).await.is_ok(),
            Ok(None) => {
                let _ = fs::write(stem.with_extension("none"), "");
            }
            Err(e) => eprintln!("SteamGridDB art for {name} failed: {e}"),
        }
    }
    added
}

pub fn toggle_favorite(app: &AppHandle, id: &str) -> Result<bool> {
    let st = app.state::<AppState>();
    let mut prefs = st.games.prefs.lock().unwrap();
    let now = if let Some(i) = prefs.favorites.iter().position(|f| f == id) {
        prefs.favorites.remove(i);
        false
    } else {
        prefs.favorites.push(id.to_string());
        true
    };
    fs::write(st.data_dir.join(PREFS_FILE), serde_json::to_string_pretty(&*prefs)?)?;
    Ok(now)
}

/// Plays a game (installing it first, for a Steam game that isn't). Resuming one that is
/// already running hands the controller straight back to it.
pub fn launch(app: &AppHandle, id: &str) -> Result<()> {
    let st = app.state::<AppState>();
    let open = |url: String| {
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| Error::msg(e.to_string()))
    };
    if let Some(appid) = id.strip_prefix("steam:") {
        let appid: u32 = appid.parse().map_err(|_| Error::msg(format!("bad game id '{id}'")))?;
        let root = steam::root().ok_or_else(|| Error::msg("Steam isn't installed"))?;
        let installed = steam::installed(&root).iter().any(|i| i.appid == appid);
        open(steam::launch_url(appid, installed))?;
    } else if let Some(name) = id.strip_prefix("epic:") {
        let item = epic::installed()
            .into_iter()
            .find(|i| i.app_name == name)
            .ok_or_else(|| Error::msg("That Epic game isn't installed anymore"))?;
        open(epic::launch_url(&item))?;
        if st.games.running().as_deref() != Some(id) {
            if let Some(exe) = epic::exe_name(&item) {
                *st.games.epic_launch.lock().unwrap() = Some(EpicLaunch {
                    id: id.to_string(),
                    exe,
                    started: Instant::now(),
                    seen: false,
                });
            }
        }
    } else {
        return Err(Error::msg(format!("unknown game '{id}'")));
    }
    if st.games.running().as_deref() == Some(id) {
        set_mode(app, Mode::Game);
    }
    Ok(())
}

/// Home pressed during a game: show the launcher and give it the controller.
/// The game keeps running and can be resumed.
pub fn to_launcher(app: &AppHandle) {
    set_mode(app, Mode::Launcher);
    focus_win::bring_to_front(app);
}

/// Watches for games starting and exiting: Steam reports its running game in the
/// registry; Epic games are found by their process.
pub fn spawn_watcher(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("game-watcher".into())
        .spawn(move || {
            let mut last_steam = 0;
            loop {
                std::thread::sleep(WATCH_EVERY);
                let current = steam::running_app();
                if current != last_steam {
                    if current != 0 {
                        started(&app, format!("steam:{current}"));
                    } else {
                        stopped(&app);
                    }
                    last_steam = current;
                }
                watch_epic(&app);
            }
        });
}

fn watch_epic(app: &AppHandle) {
    let st = app.state::<AppState>();
    let mut slot = st.games.epic_launch.lock().unwrap();
    let Some(launch) = slot.as_mut() else { return };
    let alive = process_running(&launch.exe);
    if alive && !launch.seen {
        launch.seen = true;
        let id = launch.id.clone();
        drop(slot);
        started(app, id);
    } else if !alive && launch.seen {
        *slot = None;
        drop(slot);
        stopped(app);
    } else if !alive && launch.started.elapsed() > EPIC_START_TIMEOUT {
        *slot = None;
    }
}

fn started(app: &AppHandle, id: String) {
    *app.state::<AppState>().games.running.lock().unwrap() = Some(id);
    set_mode(app, Mode::Game);
}

fn stopped(app: &AppHandle) {
    let st = app.state::<AppState>();
    if st.games.running.lock().unwrap().take().is_none() {
        return;
    }
    // A retro game started meanwhile keeps the controller and the screen.
    if st.mode() == Mode::Game && !crate::retro::playing(app) {
        set_mode(app, Mode::Launcher);
        focus_win::bring_to_front(app);
    } else {
        // Already in the launcher: just refresh what it shows about the game.
        set_mode(app, st.mode());
    }
    let _ = app.emit("games-updated", ());
}

fn process_running(exe: &str) -> bool {
    let mut cmd = Command::new("tasklist");
    cmd.args(["/FI", &format!("IMAGENAME eq {exe}"), "/NH", "/FO", "CSV"]);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    cmd.output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .to_lowercase()
                .contains(&format!("\"{}\"", exe.to_lowercase()))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn owned(appid: u32, name: &str, minutes: u32, last: i64) -> steam::Owned {
        steam::Owned { appid, name: name.into(), playtime_forever: minutes, rtime_last_played: last }
    }

    #[test]
    fn merges_owned_installed_and_local_history() {
        let installed = vec![
            steam::Installed { appid: 2, name: "Installed F2P".into() },
            steam::Installed { appid: 1, name: "Local name".into() },
        ];
        let played = HashMap::from([(1, steam::Played { minutes: 500, last: 10 }), (99, steam::Played { minutes: 5, last: 5 })]);
        let m = merge_steam(&[owned(1, "Hades", 300, 20), owned(3, "Not installed", 0, 0)], &installed, &played);

        assert_eq!(m.len(), 3, "history for games neither owned nor installed is ignored");
        assert_eq!(m[&1], SteamEntry { name: "Hades".into(), installed: true, minutes: 500, last: 20 });
        assert_eq!(m[&2].name, "Installed F2P");
        assert!(m[&2].installed);
        assert!(!m[&3].installed);
    }

    #[test]
    fn merge_drops_tools() {
        let m = merge_steam(&[owned(228980, "Steamworks Common Redistributables", 0, 0)], &[], &HashMap::new());
        assert!(m.is_empty());
    }

    /// Reads this PC's real Steam/Epic files: `cargo test real_library -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_library() {
        let dir = std::env::temp_dir();
        let lib = build(&dir, &[], &Settings::default());
        let with_local_cover = lib.games.iter().filter(|g| g.covers.len() > 1).count();
        println!(
            "steam_found={} owned={} epic_found={} games={} local_covers={}",
            lib.steam_found, lib.steam_owned, lib.epic_found, lib.games.len(), with_local_cover
        );
        let mut recent: Vec<_> = lib.games.iter().filter(|g| g.last_played.is_some()).collect();
        recent.sort_by_key(|g| std::cmp::Reverse(g.last_played));
        for g in recent.iter().take(5) {
            println!("  {} {:?} min={:?} covers={}", g.name, g.id, g.playtime_min, g.covers.len());
        }
    }

    #[test]
    fn art_paths_are_safe_file_names() {
        let stem = art_stem(Path::new("/d"), "epic:Some:Game/1", sgdb::Kind::Cover);
        assert_eq!(stem, Path::new("/d").join("art").join("epic_Some_Game_1-cover"));
    }
}
