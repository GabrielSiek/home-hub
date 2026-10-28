//! Retro games (design canvas: Games · Retro, Emulators & folders, Use my folder): RetroArch,
//! PCSX2 and Dolphin, each installed by the launcher or linked from the user's own folder; the
//! games in each console's folder, with libretro title screens, play time and save states; and
//! starting and quitting them so the controller is only ever one program's (play.rs).

mod bios;
mod browse;
pub mod emu;
mod install;
mod play;
mod scan;
mod states;
mod systems;
mod thumbs;
mod win;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

pub use browse::{Check, Listing, Target};
use emu::Emu;
pub use play::{launch, quit};

/// Whether a retro game is running (or still closing).
pub fn playing(app: &AppHandle) -> bool {
    play::running(app).is_some()
}
use states::SaveState;
use systems::{Bios, SYSTEMS};

use crate::error::{Error, Result};
use crate::state::AppState;

const STORE_FILE: &str = "retro.json";
/// Folders picked lately, for the picker's Recent.
const RECENT: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Downloaded by the launcher into its own folder.
    Installed,
    /// The user's own copy: read, never updated or changed.
    Linked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Linked {
    pub source: Source,
    pub path: PathBuf,
    pub version: Option<String>,
    /// Bytes, for a copy the launcher installed.
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Played {
    #[serde(default)]
    pub minutes: u32,
    /// Unix seconds.
    #[serde(default)]
    pub last: i64,
    /// What PCSX2 or Dolphin name this game's save states after (states::learn).
    #[serde(default)]
    pub prefix: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub emulators: BTreeMap<Emu, Linked>,
    /// Console id → its game folder.
    #[serde(default)]
    pub folders: BTreeMap<String, PathBuf>,
    #[serde(default)]
    pub bios_folder: Option<PathBuf>,
    #[serde(default)]
    pub recent: Vec<PathBuf>,
    /// Game id → its play time and saves.
    #[serde(default)]
    pub games: BTreeMap<String, Played>,
}

struct Scanned {
    folder: PathBuf,
    stamp: Option<SystemTime>,
    roms: Vec<scan::Rom>,
}

pub struct State {
    pub store: Mutex<Store>,
    library: Mutex<HashMap<&'static str, Scanned>>,
    pub installs: Mutex<HashMap<Emu, install::Progress>>,
    pub cancel: Mutex<HashSet<Emu>>,
    pub errors: Mutex<HashMap<Emu, String>>,
    /// Newest versions found by an update check (or an install).
    pub latest: Mutex<HashMap<Emu, String>>,
    pub checked: AtomicBool,
    pub session: Mutex<Option<play::Session>>,
    art_busy: AtomicBool,
    /// States folders the webview may already load pictures from.
    allowed: Mutex<HashSet<PathBuf>>,
}

impl State {
    pub fn new(data_dir: &Path) -> Self {
        let store = fs::read_to_string(data_dir.join(STORE_FILE))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            store: Mutex::new(store),
            library: Mutex::new(HashMap::new()),
            installs: Mutex::new(HashMap::new()),
            cancel: Mutex::new(HashSet::new()),
            errors: Mutex::new(HashMap::new()),
            latest: Mutex::new(HashMap::new()),
            checked: AtomicBool::new(false),
            session: Mutex::new(None),
            art_busy: AtomicBool::new(false),
            allowed: Mutex::new(HashSet::new()),
        }
    }
}

fn save(st: &AppState) {
    let json = serde_json::to_string_pretty(&*st.retro.store.lock().unwrap()).unwrap_or_default();
    if let Err(e) = fs::write(st.data_dir.join(STORE_FILE), json) {
        eprintln!("saving retro.json failed: {e}");
    }
}

/// Tells the screens to read the snapshot again.
fn changed(app: &AppHandle) {
    let _ = app.emit("retro-updated", ());
}

fn set_emulator(app: &AppHandle, emu: Emu, linked: Option<Linked>) -> Result<()> {
    let st = app.state::<AppState>();
    {
        let mut store = st.retro.store.lock().unwrap();
        match linked {
            Some(l) => store.emulators.insert(emu, l),
            None => store.emulators.remove(&emu),
        };
    }
    save(&st);
    changed(app);
    Ok(())
}

fn remember(st: &AppState, dir: &Path) {
    let mut store = st.retro.store.lock().unwrap();
    store.recent.retain(|p| p != dir);
    store.recent.insert(0, dir.to_path_buf());
    store.recent.truncate(RECENT);
}

/// Background work at startup: the first scan and any missing title screens.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = roms(&app);
        fetch_art(app).await;
    });
}

// ---------- The games ----------

/// Newest change in a folder or the folders right inside it (a game added to its own folder).
fn stamp(dir: &Path) -> Option<SystemTime> {
    let own = fs::metadata(dir).and_then(|m| m.modified()).ok()?;
    let inner = fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.metadata().and_then(|m| m.modified()).ok())
        .max();
    Some(inner.map_or(own, |i| i.max(own)))
}

/// Every game in the game folders, rescanning a folder that changed.
fn roms(app: &AppHandle) -> Vec<scan::Rom> {
    let st = app.state::<AppState>();
    let folders = st.retro.store.lock().unwrap().folders.clone();
    let mut lib = st.retro.library.lock().unwrap();
    lib.retain(|id, s| folders.get(*id) == Some(&s.folder));
    let mut added = false;
    for sys in SYSTEMS {
        let Some(folder) = folders.get(sys.id) else { continue };
        let now = stamp(folder);
        let fresh = lib.get(sys.id).is_some_and(|s| s.stamp == now && now.is_some());
        if !fresh {
            let roms = scan::scan(sys, folder);
            added |= !roms.is_empty();
            lib.insert(sys.id, Scanned { folder: folder.clone(), stamp: now, roms });
        }
    }
    let all: Vec<scan::Rom> = SYSTEMS.iter().filter_map(|s| lib.get(s.id)).flat_map(|s| s.roms.clone()).collect();
    drop(lib);
    if added {
        tauri::async_runtime::spawn(fetch_art(app.clone()));
    }
    all
}

fn rom(app: &AppHandle, id: &str) -> Option<scan::Rom> {
    roms(app).into_iter().find(|r| r.id == id)
}

/// Downloads the title screens games don't have yet, telling the screens every few.
async fn fetch_art(app: AppHandle) {
    let st = app.state::<AppState>();
    if st.retro.art_busy.swap(true, Ordering::SeqCst) {
        return;
    }
    let roms = roms(&app);
    // Per console and kind: the site's names and their index, or None when it couldn't be read.
    type Names = Option<(Vec<String>, HashMap<String, String>)>;
    let mut listings: HashMap<(&str, &str), Names> = HashMap::new();
    let mut got = 0;
    for r in roms {
        let to = thumbs::art_path(&st.data_dir, r.system, &r.stem);
        let none = thumbs::none_marker(&st.data_dir, r.system, &r.stem);
        if to.exists() || none.exists() {
            continue;
        }
        let Some(sys) = systems::get(r.system) else { continue };
        let mut found = false;
        for kind in thumbs::KINDS {
            let key = (sys.id, kind);
            if let std::collections::hash_map::Entry::Vacant(slot) = listings.entry(key) {
                slot.insert(match thumbs::listing(&st.http, &st.data_dir, sys, kind).await {
                    Ok(names) => {
                        let idx = thumbs::index(&names);
                        Some((names, idx))
                    }
                    Err(e) => {
                        eprintln!("libretro thumbnails for {} failed: {e}", sys.name);
                        None
                    }
                });
            }
            let Some((names, idx)) = listings.get(&key).and_then(|l| l.as_ref()) else { continue };
            if let Some(name) = thumbs::pick(&r.stem, names, idx) {
                match thumbs::download(&st.http, sys, kind, name, &to).await {
                    Ok(()) => {
                        found = true;
                        got += 1;
                        if got % 8 == 0 {
                            changed(&app);
                        }
                    }
                    Err(e) => eprintln!("thumbnail {name} failed: {e}"),
                }
                break;
            }
        }
        // Not listed under either kind: don't ask again (a listing that failed to load isn't a no).
        let listed = thumbs::KINDS.iter().all(|k| listings.get(&(sys.id, *k)).is_some_and(|l| l.is_some()));
        if !found && listed {
            if let Some(dir) = none.parent() {
                let _ = fs::create_dir_all(dir);
            }
            let _ = fs::write(&none, "");
        }
    }
    st.retro.art_busy.store(false, Ordering::SeqCst);
    if got > 0 {
        changed(&app);
    }
}

/// The RetroArch core file for a console: in RetroArch's cores folder, or the launcher's own
/// (where cores for a linked RetroArch go, so its folder isn't changed).
fn core_path(st: &AppState, dirs: &emu::Dirs, core: &str) -> Option<PathBuf> {
    let name = format!("{core}_libretro.dll");
    dirs.cores
        .as_ref()
        .map(|c| c.join(&name))
        .filter(|p| p.is_file())
        .or_else(|| Some(own_cores(st).join(&name)).filter(|p| p.is_file()))
}

fn own_cores(st: &AppState) -> PathBuf {
    st.data_dir.join("retro").join("cores")
}

// ---------- The snapshot ----------

#[derive(Serialize)]
pub struct SystemView {
    pub id: &'static str,
    pub name: &'static str,
    pub short: &'static str,
    pub color: &'static str,
    pub emu: Emu,
    pub emu_name: &'static str,
    pub core: Option<&'static str>,
    pub folder: Option<String>,
    /// ".z64 .n64"
    pub exts: String,
    pub count: usize,
    /// "ready", "installing", "bios", "no_core", "no_emulator", "no_folder" or "empty".
    pub status: &'static str,
    /// "RetroArch · ready", "Installing PCSX2 · 64%", "PCSX2 · BIOS missing"
    pub status_text: String,
    /// 0–1 while its emulator installs.
    pub progress: Option<f64>,
}

#[derive(Serialize)]
pub struct CoreView {
    pub name: &'static str,
    pub system: &'static str,
    pub color: &'static str,
    pub installed: bool,
}

#[derive(Serialize)]
pub struct EmuView {
    pub emu: Emu,
    pub name: &'static str,
    /// "N64 · PlayStation · Mega Drive · SNES · GBA"
    pub systems: String,
    pub source: Option<Source>,
    pub path: Option<String>,
    pub version: Option<String>,
    /// A newer version than the installed one, from an update check.
    pub update: Option<String>,
    pub size: Option<u64>,
    /// A linked copy: where its settings are.
    pub settings: Option<String>,
    pub cores: Vec<CoreView>,
    pub install: Option<install::Progress>,
    pub error: Option<String>,
    pub source_label: &'static str,
    /// Where "Install for me" puts it.
    pub target: String,
    /// Where its BIOS goes (RetroArch's system folder, PCSX2's bios folder).
    pub bios_dir: Option<String>,
    /// Its program isn't in its folder anymore.
    pub missing: bool,
}

#[derive(Serialize)]
pub struct GameView {
    pub id: String,
    pub system: &'static str,
    pub name: String,
    pub file: String,
    pub minutes: u32,
    /// Unix seconds.
    pub last_played: Option<i64>,
    /// libretro's title screen, if found.
    pub art: Option<String>,
    /// Newest first.
    pub states: Vec<SaveState>,
    /// Play picks up from the auto-save.
    pub resume: bool,
}

#[derive(Serialize)]
pub struct BiosView {
    pub system: &'static str,
    pub name: &'static str,
    /// "ok", "missing" or "none" (not needed).
    pub status: &'static str,
    pub text: String,
}

#[derive(Serialize)]
pub struct Retro {
    pub systems: Vec<SystemView>,
    pub emulators: Vec<EmuView>,
    pub games: Vec<GameView>,
    pub bios: Vec<BiosView>,
    pub bios_folder: Option<String>,
    pub tv_settings: &'static [&'static str],
    pub running: Option<String>,
    pub checked_updates: bool,
}

fn emu_root(l: &Linked, emu: Emu) -> Option<PathBuf> {
    emu::find_exe(&l.path, emu).and_then(|e| e.parent().map(Path::to_path_buf))
}

pub fn snapshot(app: &AppHandle) -> Retro {
    let st = app.state::<AppState>();
    let all = roms(app);
    let store = st.retro.store.lock().unwrap();
    let installs = st.retro.installs.lock().unwrap().clone();
    let errors = st.retro.errors.lock().unwrap().clone();
    let latest = st.retro.latest.lock().unwrap().clone();

    // Each emulator's folders, read once.
    let roots: HashMap<Emu, PathBuf> =
        store.emulators.iter().filter_map(|(e, l)| emu_root(l, *e).map(|r| (*e, r))).collect();
    let dirs: HashMap<Emu, emu::Dirs> = roots.iter().map(|(e, r)| (*e, emu::dirs(*e, r))).collect();
    {
        let mut allowed = st.retro.allowed.lock().unwrap();
        for d in dirs.values() {
            if allowed.insert(d.states.clone()) {
                let _ = app.asset_protocol_scope().allow_directory(&d.states, true);
            }
        }
    }
    let state_files: HashMap<Emu, Vec<PathBuf>> = dirs.iter().map(|(e, d)| (*e, states::files(&d.states))).collect();
    let shots = st.data_dir.join("retro-art").join("states");

    let ps1 = dirs.get(&Emu::Retroarch).and_then(|d| d.bios.as_deref()).and_then(bios::ps1_in);
    let ps2 = dirs.get(&Emu::Pcsx2).and_then(|d| d.bios.as_deref()).and_then(bios::ps2_in);
    let bios_ok = |b: Bios| match b {
        Bios::None => true,
        Bios::Ps1 => ps1.is_some(),
        Bios::Ps2 => ps2.is_some(),
    };
    let has_core = |core: Option<&str>| {
        core.is_none_or(|c| dirs.get(&Emu::Retroarch).is_some_and(|d| core_path(&st, d, c).is_some()))
    };

    let mut counts: HashMap<&str, usize> = HashMap::new();
    for r in &all {
        *counts.entry(r.system).or_default() += 1;
    }
    let systems = SYSTEMS
        .iter()
        .map(|s| {
            let linked = store.emulators.get(&s.emu);
            let count = counts.get(s.id).copied().unwrap_or(0);
            let folder = store.folders.get(s.id);
            let inst = installs.get(&s.emu);
            let pct = inst.filter(|p| p.total > 0).map(|p| p.done as f64 / p.total as f64);
            let (status, text) = if linked.is_none() || !roots.contains_key(&s.emu) {
                match inst {
                    Some(_) => ("installing", format!("Installing {}{}", s.emu.name(), pct.map(|p| format!(" · {:.0}%", p * 100.0)).unwrap_or_default())),
                    None => ("no_emulator", format!("{} not set up", s.emu.name())),
                }
            } else if !has_core(s.core) {
                ("no_core", format!("{} core missing", s.core.unwrap_or_default()))
            } else if !bios_ok(s.bios) {
                ("bios", format!("{} · BIOS missing", s.emu.name()))
            } else if folder.is_none() {
                ("no_folder", "Choose a game folder".into())
            } else if count == 0 {
                ("empty", "No games in its folder".into())
            } else if linked.is_some_and(|l| l.source == Source::Linked) {
                ("ready", format!("{} · your folder", s.emu.name()))
            } else {
                ("ready", format!("{} · ready", s.emu.name()))
            };
            SystemView {
                id: s.id,
                name: s.name,
                short: s.short,
                color: s.color,
                emu: s.emu,
                emu_name: s.emu.name(),
                core: s.core,
                folder: folder.map(|f| f.display().to_string()),
                exts: s.exts.iter().filter(|e| !matches!(**e, "zip" | "7z")).map(|e| format!(".{e}")).collect::<Vec<_>>().join(" "),
                count,
                status,
                status_text: text,
                progress: if status == "installing" { pct } else { None },
            }
        })
        .collect();

    let emulators = Emu::ALL
        .into_iter()
        .map(|e| {
            let l = store.emulators.get(&e);
            let d = dirs.get(&e);
            let cores = match e {
                Emu::Retroarch => SYSTEMS
                    .iter()
                    .filter_map(|s| {
                        let c = s.core?;
                        Some(CoreView { name: c, system: s.short, color: s.color, installed: d.is_some_and(|d| core_path(&st, d, c).is_some()) })
                    })
                    .collect(),
                _ => vec![],
            };
            let version = l.and_then(|l| l.version.clone());
            EmuView {
                emu: e,
                name: e.name(),
                // "N64 · PlayStation · Mega Drive · SNES · GBA"
                systems: SYSTEMS
                    .iter()
                    .filter(|s| s.emu == e)
                    .map(|s| if s.short.len() <= 4 && s.short != "MD" { s.short } else { s.name })
                    .collect::<Vec<_>>()
                    .join(" · "),
                source: l.map(|l| l.source),
                path: l.map(|l| l.path.display().to_string()),
                update: latest
                    .get(&e)
                    .filter(|v| l.is_some_and(|l| l.source == Source::Installed) && version.as_ref() != Some(*v))
                    .cloned(),
                version,
                size: l.and_then(|l| l.size),
                settings: l.filter(|l| l.source == Source::Linked).and_then(|_| roots.get(&e)).map(|r| emu::settings_label(e, r)),
                cores,
                install: installs.get(&e).cloned(),
                error: errors.get(&e).cloned(),
                source_label: e.source(),
                target: win::install_root_shown().join(e.name()).display().to_string(),
                bios_dir: d.and_then(|d| d.bios.as_ref()).filter(|_| e != Emu::Dolphin).map(|b| b.display().to_string()),
                missing: l.is_some() && !roots.contains_key(&e),
            }
        })
        .collect();

    let games = all
        .iter()
        .map(|r| {
            let sys = systems::get(r.system);
            let played = store.games.get(&r.id).cloned().unwrap_or_default();
            let emu = sys.map_or(Emu::Retroarch, |s| s.emu);
            let files = state_files.get(&emu).map(Vec::as_slice).unwrap_or_default();
            let states = match (emu, &played.prefix) {
                (Emu::Retroarch, _) => states::retroarch(files, &r.stem),
                (Emu::Pcsx2, Some(p)) => states::pcsx2(files, p, &shots),
                (Emu::Dolphin, Some(p)) => states::dolphin(files, p),
                _ => vec![],
            };
            let art = thumbs::art_path(&st.data_dir, r.system, &r.stem);
            GameView {
                id: r.id.clone(),
                system: r.system,
                name: r.name.clone(),
                file: r.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                minutes: played.minutes,
                last_played: (played.last > 0).then_some(played.last),
                art: art.is_file().then(|| art.to_string_lossy().into_owned()),
                resume: states.iter().any(|s| s.slot.is_none()),
                states,
            }
        })
        .collect();

    let ra_bios = dirs.get(&Emu::Retroarch).and_then(|d| d.bios.clone());
    let p2_bios = dirs.get(&Emu::Pcsx2).and_then(|d| d.bios.clone());
    let shown = |p: &Option<PathBuf>, fallback: &str| p.as_ref().map_or(fallback.to_string(), |p| p.display().to_string());
    let bios = vec![
        match &ps1 {
            Some(f) => BiosView { system: "ps1", name: "PlayStation", status: "ok", text: format!("{f} found · used by swanstation") },
            None => BiosView {
                system: "ps1",
                name: "PlayStation",
                status: "missing",
                text: format!("Missing · put it in {}", shown(&ra_bios, r"RetroArch\system")),
            },
        },
        match &ps2 {
            Some(f) => BiosView { system: "ps2", name: "PlayStation 2", status: "ok", text: format!("{f} found · used by PCSX2") },
            None => BiosView {
                system: "ps2",
                name: "PlayStation 2",
                status: "missing",
                text: format!("Missing · put it in {}", shown(&p2_bios, r"PCSX2\bios")),
            },
        },
        BiosView { system: "gc", name: "GameCube", status: "none", text: "Not needed · Dolphin boots games directly".into() },
    ];

    let bios_folder = store.bios_folder.as_ref().map(|p| p.display().to_string());
    drop(store);
    Retro {
        systems,
        emulators,
        games,
        bios,
        bios_folder,
        tv_settings: emu::TV_SETTINGS,
        running: play::running(app),
        checked_updates: st.retro.checked.load(Ordering::SeqCst),
    }
}

// ---------- What the screens ask for ----------

pub fn install(app: &AppHandle, emu: Emu) -> Result<()> {
    let st = app.state::<AppState>();
    if st.retro.installs.lock().unwrap().contains_key(&emu) {
        return Ok(());
    }
    if st.retro.session.lock().unwrap().as_ref().is_some_and(|s| s.emu == emu) {
        return Err(Error::msg(format!("{} is running", emu.name())));
    }
    tauri::async_runtime::spawn(install::install(app.clone(), emu));
    Ok(())
}

pub fn cancel(app: &AppHandle, emu: Emu) {
    app.state::<AppState>().retro.cancel.lock().unwrap().insert(emu);
}

/// What a removed copy leaves in its folder: everything that is the user's, not the program's.
fn keeps(emu: Emu) -> &'static [&'static str] {
    match emu {
        Emu::Retroarch => &["states", "saves", "system", "screenshots", "config"],
        Emu::Pcsx2 => &["bios", "memcards", "sstates", "snaps", "cheats", "inis"],
        Emu::Dolphin => &["User"],
    }
}

/// Unlinks the user's copy, or deletes the launcher's (its program files only: saves, states,
/// memory cards and BIOS stay in its folder).
pub fn remove(app: &AppHandle, emu: Emu) -> Result<()> {
    let st = app.state::<AppState>();
    if st.retro.session.lock().unwrap().as_ref().is_some_and(|s| s.emu == emu) {
        return Err(Error::msg(format!("{} is running", emu.name())));
    }
    let Some(l) = st.retro.store.lock().unwrap().emulators.get(&emu).cloned() else { return Ok(()) };
    if l.source == Source::Installed {
        let keep = keeps(emu);
        for e in fs::read_dir(&l.path)?.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if keep.iter().any(|k| k.eq_ignore_ascii_case(&name)) {
                continue;
            }
            let p = e.path();
            let r = if e.file_type()?.is_dir() { fs::remove_dir_all(&p) } else { fs::remove_file(&p) };
            if let Err(err) = r {
                eprintln!("removing {} failed: {err}", p.display());
            }
        }
        let _ = fs::remove_dir(&l.path);
    }
    st.retro.latest.lock().unwrap().remove(&emu);
    set_emulator(app, emu, None)
}

/// Links the user's own copy in `path`. `move_settings`: copy its settings into the folder
/// (portable mode); `folders`: console folders found beside it, as game folders.
pub fn link(app: &AppHandle, emu: Emu, path: &str, move_settings: bool, folders: Vec<(String, String)>) -> Result<()> {
    let st = app.state::<AppState>();
    let dir = PathBuf::from(path);
    let exe = emu::find_exe(&dir, emu).ok_or_else(|| Error::msg(format!("No {} in that folder", emu.exe())))?;
    let root = exe.parent().map(Path::to_path_buf).unwrap_or_else(|| dir.clone());
    if move_settings {
        browse::move_settings(emu, &root)?;
    }
    {
        let mut store = st.retro.store.lock().unwrap();
        for (sys, p) in folders {
            if systems::get(&sys).is_some() {
                store.folders.insert(sys, PathBuf::from(p));
            }
        }
    }
    if let Some(parent) = dir.parent() {
        remember(&st, parent);
    }
    st.retro.errors.lock().unwrap().remove(&emu);
    set_emulator(app, emu, Some(Linked { source: Source::Linked, path: dir, version: emu::version(emu, &exe), size: None }))?;
    tauri::async_runtime::spawn(fetch_art(app.clone()));
    Ok(())
}

/// Sets (or, with none, clears) consoles' game folders.
pub fn set_folders(app: &AppHandle, folders: Vec<(String, Option<String>)>) -> Result<()> {
    let st = app.state::<AppState>();
    for (sys, path) in folders {
        if systems::get(&sys).is_none() {
            return Err(Error::msg(format!("unknown console '{sys}'")));
        }
        match path {
            Some(p) => {
                let p = PathBuf::from(p);
                if let Some(parent) = p.parent() {
                    remember(&st, parent);
                }
                st.retro.store.lock().unwrap().folders.insert(sys, p);
            }
            None => {
                st.retro.store.lock().unwrap().folders.remove(&sys);
            }
        }
    }
    save(&st);
    changed(app);
    Ok(())
}

/// The folder BIOS files are copied from into the emulators that need them.
pub fn set_bios(app: &AppHandle, path: &str) -> Result<()> {
    let st = app.state::<AppState>();
    let dir = PathBuf::from(path);
    st.retro.store.lock().unwrap().bios_folder = Some(dir.clone());
    remember(&st, &dir);
    save(&st);
    scan_bios(app);
    Ok(())
}

/// "Scan again": copies BIOS files from the BIOS folder in, then shows what's there.
pub fn scan_bios(app: &AppHandle) {
    let st = app.state::<AppState>();
    let (from, emus) = {
        let s = st.retro.store.lock().unwrap();
        (s.bios_folder.clone(), s.emulators.clone())
    };
    if let Some(from) = from {
        let bios_of = |e: Emu| emus.get(&e).and_then(|l| emu_root(l, e)).and_then(|r| emu::dirs(e, &r).bios);
        bios::copy_missing(&from, bios_of(Emu::Retroarch).as_deref(), bios_of(Emu::Pcsx2).as_deref());
    }
    changed(app);
}

/// Downloads the cores RetroArch lacks: into its own cores folder for the launcher's copy, the
/// launcher's for a linked one.
pub fn add_cores(app: &AppHandle) -> Result<()> {
    let st = app.state::<AppState>();
    let l = st
        .retro
        .store
        .lock()
        .unwrap()
        .emulators
        .get(&Emu::Retroarch)
        .cloned()
        .ok_or_else(|| Error::msg("Set up RetroArch first"))?;
    let cores = match l.source {
        Source::Installed => emu_root(&l, Emu::Retroarch)
            .and_then(|r| emu::dirs(Emu::Retroarch, &r).cores)
            .ok_or_else(|| Error::msg("RetroArch isn't in its folder anymore"))?,
        Source::Linked => own_cores(&st),
    };
    tauri::async_runtime::spawn(install::add_cores(app.clone(), cores));
    Ok(())
}

pub fn check_updates(app: &AppHandle) {
    tauri::async_runtime::spawn(install::check_updates(app.clone()));
}

/// Opens the emulator's BIOS folder in Explorer (made first, if it isn't there yet).
pub fn open_bios(app: &AppHandle, emu: Emu) -> Result<()> {
    let st = app.state::<AppState>();
    let l = st.retro.store.lock().unwrap().emulators.get(&emu).cloned().ok_or_else(|| Error::msg("Not set up"))?;
    let root = emu_root(&l, emu).ok_or_else(|| Error::msg("Its folder is gone"))?;
    let dir = emu::dirs(emu, &root).bios.ok_or_else(|| Error::msg(format!("{} needs no BIOS", emu.name())))?;
    fs::create_dir_all(&dir)?;
    app.opener()
        .open_path(dir.display().to_string(), None::<&str>)
        .map_err(|e| Error::msg(e.to_string()))
}

pub fn browse(app: &AppHandle, path: Option<&str>) -> Listing {
    let recent = app.state::<AppState>().retro.store.lock().unwrap().recent.clone();
    browse::list(path, &recent)
}

/// Where the picker opens for `target`: next to what's set now, else the last folder picked.
pub fn browse_start(app: &AppHandle, target: &Target) -> Option<String> {
    let st = app.state::<AppState>();
    let store = st.retro.store.lock().unwrap();
    let current = match target {
        Target::Emulator { emu } => store.emulators.get(emu).filter(|l| l.source == Source::Linked).map(|l| l.path.clone()),
        Target::Folder { system } => store.folders.get(system).cloned(),
        Target::Bios => store.bios_folder.clone(),
    };
    current
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .or_else(|| store.recent.first().cloned())
        .filter(|p| p.is_dir())
        .map(|p| p.display().to_string())
}

pub fn check(target: &Target, path: &str) -> Check {
    browse::check(target, Path::new(path))
}
