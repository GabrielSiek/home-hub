use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::cdp;
use crate::config::{AppEntry, AppKind, Browser, Nav, Settings};
use crate::error::{Error, Result};
use crate::focus_win;
use crate::history::{now_ms, Opened};
use crate::state::{set_mode, AppState, Mode, Running};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Opens an app from apps.toml. `title`, when given, opens the app's search for it.
pub fn launch(
    app: &AppHandle,
    id: &str,
    title: Option<String>,
    backdrop: Option<String>,
) -> Result<()> {
    let st = app.state::<AppState>();
    let (entry, settings) = {
        let cfg = st.config.read().unwrap();
        let entry = cfg
            .apps
            .iter()
            .find(|a| a.id == id)
            .cloned()
            .ok_or_else(|| Error::msg(format!("unknown app '{id}'")))?;
        (entry, cfg.settings.clone())
    };

    // One app at a time: the browser profile can't be shared between two instances.
    crate::watch::end(app);
    stop_running(&st);

    let no_url = || Error::msg(format!("app '{id}' has no url"));
    match entry.kind {
        AppKind::Web => {
            let url = target_url(&entry, title.as_deref()).ok_or_else(no_url)?;
            let page = cdp::Page {
                url,
                nav: entry.nav,
                user_agent: entry.user_agent.clone(),
                watch: None,
                live: None,
            };
            spawn_browser(app, &settings, page, &entry.id)?;
        }
        AppKind::Exec => {
            let exe = entry
                .exec
                .as_deref()
                .ok_or_else(|| Error::msg(format!("app '{id}' has no exec")))?;
            let mut cmd = Command::new(exe);
            cmd.args(&entry.args);
            spawn_tracked(app, cmd, &entry.id)?;
        }
        AppKind::Uri => {
            let url = target_url(&entry, title.as_deref()).ok_or_else(no_url)?;
            app.opener()
                .open_url(url, None::<&str>)
                .map_err(|e| Error::msg(e.to_string()))?;
        }
    }

    // Only media goes in the history; opening an app from its tile is not remembered.
    if title.is_none() {
        return Ok(());
    }
    let mut history = st.history.lock().unwrap();
    history.record_open(Opened {
        app_id: entry.id,
        title,
        backdrop,
        at: now_ms(),
    });
    history.save(&st.data_dir)
}

/// Opens the web app `id` at `url` instead of its own start page. With `watch`, it plays from the
/// launcher's copy of a streaming service (watch/mod.rs), which already ended the last session.
pub fn launch_web(app: &AppHandle, id: &str, url: &str, watch: Option<String>) -> Result<()> {
    open_web(app, id, url, watch, None)
}

/// Opens the web app `id` at a live sports channel, with the live overlay's script (`live`) in
/// every page it loads (sports/mod.rs).
pub fn launch_live(app: &AppHandle, id: &str, url: &str, live: String) -> Result<()> {
    open_web(app, id, url, None, Some(live))
}

fn open_web(app: &AppHandle, id: &str, url: &str, watch: Option<String>, live: Option<String>) -> Result<()> {
    let st = app.state::<AppState>();
    let (entry, settings) = {
        let cfg = st.config.read().unwrap();
        let entry = cfg
            .apps
            .iter()
            .find(|a| a.id == id)
            .cloned()
            .ok_or_else(|| Error::msg(format!("unknown app '{id}'")))?;
        (entry, cfg.settings.clone())
    };
    if entry.kind != AppKind::Web {
        return Err(Error::msg(format!("{} isn't a web app", entry.name)));
    }
    if watch.is_none() {
        crate::watch::end(app);
    }
    stop_running(&st);
    let page = cdp::Page {
        url: url.to_string(),
        nav: entry.nav,
        user_agent: entry.user_agent.clone(),
        watch,
        live,
    };
    spawn_browser(app, &settings, page, id)
}

/// Closes the running app and returns to the launcher. The launcher is back in front at once;
/// the browser, already silenced, shuts down behind it (a clean exit takes a few seconds).
pub fn close(app: &AppHandle) {
    let st = app.state::<AppState>();
    let stopping = let_go(&st);
    crate::watch::end(app);
    set_mode(app, Mode::Launcher);
    focus_win::bring_to_front(app);
    if let Some(s) = stopping {
        let mut slot = st.stopping.lock().unwrap();
        let before = slot.take();
        *slot = Some(std::thread::spawn(move || {
            if let Some(b) = before {
                let _ = b.join();
            }
            s.finish();
        }));
    }
}

/// Whether a closed app is still shutting down (and may still hold the browser profile).
pub fn closing(st: &AppState) -> bool {
    st.stopping.lock().unwrap().as_ref().is_some_and(|h| !h.is_finished())
}

/// Opens a page that isn't in apps.toml (e.g. a sign-in) in the browser's app window,
/// tracked as `id` so Home closes it like any app.
pub fn open_page(app: &AppHandle, url: &str, id: &str) -> Result<()> {
    let st = app.state::<AppState>();
    let settings = st.config.read().unwrap().settings.clone();
    crate::watch::end(app);
    stop_running(&st);
    let page = cdp::Page {
        url: url.to_string(),
        nav: Nav::Tv,
        user_agent: None,
        watch: None,
        live: None,
    };
    spawn_browser(app, &settings, page, id)
}

/// Starts the browser on `page` and takes control of it through DevTools (cdp.rs).
fn spawn_browser(app: &AppHandle, settings: &Settings, page: cdp::Page, app_id: &str) -> Result<()> {
    let profile = app.state::<AppState>().data_dir.join("browser-profile");
    let port = cdp::free_port();
    let mut cmd = Command::new(browser_exe(settings));
    cmd.args(browser_args(&page.url, &profile, page.user_agent.as_deref(), port));
    crate::cursor::off_top(app);
    focus_win::cover(app);
    let pid = spawn_tracked(app, cmd, app_id).inspect_err(|_| focus_win::uncover(app))?;
    focus_win::uncover_when_fullscreen(app, pid);
    // Without a port the pad still works, through the keyboard bridge.
    if let Some(port) = port {
        cdp::connect(app, port, page);
    }
    Ok(())
}

/// Starts the launcher's own player: the running app makes way, as for any other.
pub fn spawn_player(app: &AppHandle, cmd: Command) -> Result<u32> {
    stop_running(&app.state::<AppState>());
    spawn_as(app, cmd, crate::watch::library::SERVICE, Mode::Player)
}

pub fn is_running(app: &AppHandle, id: &str) -> bool {
    app.state::<AppState>()
        .running
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|r| r.app_id == id)
}

/// Closes the running app only if it is still `id`.
pub fn close_if(app: &AppHandle, id: &str) {
    if is_running(app, id) {
        close(app);
    }
}

fn spawn_tracked(app: &AppHandle, cmd: Command, app_id: &str) -> Result<u32> {
    spawn_as(app, cmd, app_id, Mode::Streaming)
}

fn spawn_as(app: &AppHandle, mut cmd: Command, app_id: &str, mode: Mode) -> Result<u32> {
    let mut child = cmd.spawn()?;
    let pid = child.id();
    let (exited_tx, exited) = mpsc::channel();
    *app.state::<AppState>().running.lock().unwrap() = Some(Running {
        pid,
        app_id: app_id.to_string(),
        exited,
    });
    set_mode(app, mode);

    // When the app exits by itself, come back to the launcher. A pid that was already
    // taken out of `running` was closed by us, and `close` handles the return.
    let app = app.clone();
    std::thread::spawn(move || {
        let _ = child.wait();
        let _ = exited_tx.send(());
        let ours = {
            let st = app.state::<AppState>();
            let mut running = st.running.lock().unwrap();
            if running.as_ref().is_some_and(|r| r.pid == pid) {
                *running = None;
                true
            } else {
                false
            }
        };
        if ours {
            crate::watch::end(&app);
            set_mode(&app, Mode::Launcher);
            focus_win::bring_to_front(&app);
        }
    });
    Ok(pid)
}

/// A browser asked to close gets this long to save its profile and exit before it is killed.
const CLOSE_WAIT: Duration = Duration::from_secs(5);

/// An app let go of: no longer the running one, but maybe not gone yet.
struct Stopping {
    running: Running,
    /// The browser was asked to shut down by itself.
    asked: bool,
}

impl Stopping {
    fn finish(self) {
        let r = self.running;
        let timed_out = || matches!(r.exited.recv_timeout(CLOSE_WAIT), Err(RecvTimeoutError::Timeout));
        if !self.asked || timed_out() {
            kill_tree(r.pid);
        }
    }
}

/// Takes the running app out of `running`, a browser silenced and asked to shut down.
fn let_go(st: &AppState) -> Option<Stopping> {
    // A watch-history sync may have the browser profile open: it gives way at once.
    crate::watch::sync::abort(st);
    let asked = cdp::disconnect(st);
    let running = st.running.lock().unwrap().take()?;
    Some(Stopping { running, asked })
}

/// Stops the running app and waits until it, and any app closed before it, is gone: the next
/// browser can't share the profile with one still shutting down.
fn stop_running(st: &AppState) {
    if let Some(s) = let_go(st) {
        s.finish();
    }
    let before = st.stopping.lock().unwrap().take();
    if let Some(b) = before {
        let _ = b.join();
    }
}

pub(crate) fn kill_tree(pid: u32) {
    #[cfg(windows)]
    eprintln!("kill_tree {pid}: {}", process_name(pid).unwrap_or_else(|| "(gone)".into()));
    #[cfg(windows)]
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    #[cfg(not(windows))]
    let _ = Command::new("kill").arg(pid.to_string()).status();
}

/// The image name of process `pid`, for the log.
#[cfg(windows)]
fn process_name(pid: u32) -> Option<String> {
    let out = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let line = String::from_utf8_lossy(&out.stdout).lines().next()?.to_string();
    line.strip_prefix('"')?.split('"').next().map(str::to_string)
}

pub fn target_url(entry: &AppEntry, query: Option<&str>) -> Option<String> {
    match (query, &entry.search_url) {
        (Some(q), Some(search)) if !q.trim().is_empty() => {
            Some(search.replace("{q}", &urlencoding::encode(q.trim())))
        }
        _ => entry.url.clone(),
    }
}

/// App-mode window, fullscreen, on a profile of our own so logins persist.
/// (Edge's --kiosk is avoided on purpose: it always runs InPrivate and forgets logins.)
/// DevTools listens on `devtools_port`, local only (see `cdp`).
pub fn browser_args(
    url: &str,
    profile: &Path,
    user_agent: Option<&str>,
    devtools_port: Option<u16>,
) -> Vec<String> {
    let mut args = vec![
        format!("--app={url}"),
        "--start-fullscreen".into(),
        format!("--user-data-dir={}", profile.display()),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--autoplay-policy=no-user-gesture-required".into(),
    ];
    if let Some(port) = devtools_port {
        args.push(format!("--remote-debugging-port={port}"));
    }
    if let Some(ua) = user_agent {
        args.push(format!("--user-agent={ua}"));
    }
    args
}

pub(crate) fn browser_exe(settings: &Settings) -> PathBuf {
    if let Some(p) = settings.browser_path.as_deref().filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let (exe, fallbacks): (&str, &[&str]) = match settings.browser {
        Browser::Edge => (
            "msedge.exe",
            &[r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"],
        ),
        Browser::Chrome => (
            "chrome.exe",
            &[r"C:\Program Files\Google\Chrome\Application\chrome.exe"],
        ),
    };
    registered_app_path(exe)
        .or_else(|| fallbacks.iter().map(PathBuf::from).find(|p| p.exists()))
        .unwrap_or_else(|| PathBuf::from(exe))
}

#[cfg(windows)]
fn registered_app_path(exe: &str) -> Option<PathBuf> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let sub = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{exe}");
    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE].into_iter().find_map(|hive| {
        let path: String = RegKey::predef(hive).open_subkey(&sub).ok()?.get_value("").ok()?;
        Some(PathBuf::from(path.trim_matches('"'))).filter(|p| p.exists())
    })
}

#[cfg(not(windows))]
fn registered_app_path(_exe: &str) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_apps;

    fn netflix() -> AppEntry {
        parse_apps(
            r#"
            [[apps]]
            id = "netflix"
            name = "Netflix"
            kind = "web"
            url = "https://www.netflix.com/browse"
            search_url = "https://www.netflix.com/search?q={q}"
            "#,
        )
        .unwrap()
        .remove(0)
    }

    #[test]
    fn target_url_uses_search_for_titles() {
        let e = netflix();
        assert_eq!(target_url(&e, None).unwrap(), "https://www.netflix.com/browse");
        assert_eq!(target_url(&e, Some("  ")).unwrap(), "https://www.netflix.com/browse");
        assert_eq!(
            target_url(&e, Some("Dune: Part Two")).unwrap(),
            "https://www.netflix.com/search?q=Dune%3A%20Part%20Two"
        );
    }

    #[test]
    fn target_url_without_search_falls_back_to_home() {
        let mut e = netflix();
        e.search_url = None;
        assert_eq!(target_url(&e, Some("Dark")).unwrap(), "https://www.netflix.com/browse");
    }

    #[test]
    fn browser_args_shape() {
        let args = browser_args("https://x.test/", Path::new("C:/p"), Some("TV"), Some(47950));
        assert_eq!(args[0], "--app=https://x.test/");
        assert!(args.contains(&"--start-fullscreen".to_string()));
        assert!(args.contains(&"--user-data-dir=C:/p".to_string()));
        assert!(!args.iter().any(|a| a.starts_with("--kiosk")));
        // Port 0 would flag the browser as automated (navigator.webdriver).
        assert!(args.contains(&"--remote-debugging-port=47950".to_string()));
        let no_port = browser_args("https://x.test/", Path::new("C:/p"), None, None);
        assert!(!no_port.iter().any(|a| a.starts_with("--remote-debugging-port")));
        assert_eq!(args.last().unwrap(), "--user-agent=TV");
    }
}
