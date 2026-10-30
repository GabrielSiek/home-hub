use std::sync::atomic::{AtomicU64, Ordering};

use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// Brings the launcher window back in front of a browser or game.
pub fn bring_to_front(app: &AppHandle) {
    // A browser still starting under the cover is no longer the one to show.
    COVER.fetch_add(1, Ordering::SeqCst);
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    let _ = w.unminimize();
    let _ = w.show();
    // Windows refuses SetForegroundWindow to background processes;
    // going topmost for a moment gets the window in front anyway.
    let _ = w.set_always_on_top(true);
    let _ = w.set_focus();
    let _ = w.set_always_on_top(false);
}

/// Each cover, so a stale one's watch can tell it was replaced or dropped.
static COVER: AtomicU64 = AtomicU64::new(0);

/// Chromium first shows its window as a plain one (title bar, white page) and only then goes
/// fullscreen. The launcher, topmost from before the browser starts, covers that until
/// [`uncover_when_fullscreen`].
pub fn cover(app: &AppHandle) {
    COVER.fetch_add(1, Ordering::SeqCst);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_always_on_top(true);
    }
}

/// Drops the cover, leaving the launcher where it is.
pub fn uncover(app: &AppHandle) {
    COVER.fetch_add(1, Ordering::SeqCst);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_always_on_top(false);
    }
}

/// Longest the cover waits for the browser's window to go fullscreen.
#[cfg(windows)]
const COVER_FOR: std::time::Duration = std::time::Duration::from_secs(10);
/// After it does, a moment for the page to paint.
#[cfg(windows)]
const SETTLE: std::time::Duration = std::time::Duration::from_millis(400);
#[cfg(windows)]
const POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// Drops the cover once the browser `pid` has a fullscreen window, and puts the launcher
/// behind it. A browser closed or replaced meanwhile leaves the launcher as it is.
#[cfg(windows)]
pub fn uncover_when_fullscreen(app: &AppHandle, pid: u32) {
    let running = move |app: &AppHandle| {
        app.state::<AppState>().running.lock().unwrap().as_ref().is_some_and(|r| r.pid == pid)
    };
    uncover_when_shown(app, pid, running, false);
}

/// The same for an emulator, which is then given the keyboard focus too: Dolphin's hotkeys,
/// and RetroArch's pause when it isn't in front, go by it.
#[cfg(windows)]
pub fn uncover_when_game_fullscreen(app: &AppHandle, pid: u32, alive: impl Fn(&AppHandle) -> bool + Send + 'static) {
    uncover_when_shown(app, pid, alive, true);
}

/// How long to wait for the player's window before giving up on it.
#[cfg(windows)]
const PLAYER_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);

/// Keeps the launcher on top of the player `pid`, and gives it back the focus the player's window
/// takes on opening: the launcher's window is the player's controls.
#[cfg(windows)]
pub fn stay_over(app: &AppHandle, pid: u32) {
    use std::time::Instant;

    let app = app.clone();
    let _ = std::thread::Builder::new().name("player-window".into()).spawn(move || {
        let until = Instant::now() + PLAYER_WINDOW;
        while Instant::now() < until && win::browser_window(pid).is_none() {
            std::thread::sleep(POLL);
        }
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.set_always_on_top(true);
            let _ = w.set_focus();
        }
    });
}

#[cfg(not(windows))]
pub fn stay_over(_app: &AppHandle, _pid: u32) {}

#[cfg(windows)]
fn uncover_when_shown(app: &AppHandle, pid: u32, alive: impl Fn(&AppHandle) -> bool + Send + 'static, focus: bool) {
    use std::time::Instant;

    let Some(launcher) = app.get_webview_window("main").and_then(|w| w.hwnd().ok()) else {
        uncover(app);
        return;
    };
    // HWND isn't Send; the handle itself stays valid for the launcher's lifetime.
    let launcher = launcher.0 as isize;
    let cover = COVER.load(Ordering::SeqCst);
    let current = move || COVER.load(Ordering::SeqCst) == cover;
    let app = app.clone();
    let _ = std::thread::Builder::new().name("cover".into()).spawn(move || {
        let until = Instant::now() + COVER_FOR;
        let mut window = None;
        while current() && Instant::now() < until {
            window = win::browser_window(pid);
            if window.is_some_and(|w| w.fullscreen) {
                std::thread::sleep(SETTLE);
                break;
            }
            std::thread::sleep(POLL);
        }
        if !current() {
            return;
        }
        uncover(&app);
        if alive(&app) {
            // Behind the browser's window, or, with none found, behind everything.
            win::put_behind(launcher, window.map(|w| w.hwnd));
            if let Some(w) = window.filter(|_| focus) {
                win::focus(w.hwnd);
            }
        }
    });
}

#[cfg(not(windows))]
pub fn uncover_when_fullscreen(app: &AppHandle, _pid: u32) {
    uncover(app);
}

#[cfg(not(windows))]
pub fn uncover_when_game_fullscreen(app: &AppHandle, _pid: u32, _alive: impl Fn(&AppHandle) -> bool + Send + 'static) {
    uncover(app);
}

#[cfg(windows)]
mod win {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow, SetWindowPos,
        HWND_BOTTOM, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };

    #[derive(Clone, Copy)]
    pub struct BrowserWindow {
        pub hwnd: isize,
        pub fullscreen: bool,
    }

    /// A shown window of process `pid`, a fullscreen one first.
    pub fn browser_window(pid: u32) -> Option<BrowserWindow> {
        struct Search {
            pid: u32,
            found: Option<BrowserWindow>,
        }
        unsafe extern "system" fn visit(hwnd: HWND, search: LPARAM) -> BOOL {
            // SAFETY: `search` points at the Search below, alive for the whole EnumWindows call.
            let search = unsafe { &mut *(search.0 as *mut Search) };
            let mut owner = 0;
            // SAFETY: plain Win32 queries on a window EnumWindows just handed us.
            let shown = unsafe {
                GetWindowThreadProcessId(hwnd, Some(&mut owner));
                owner == search.pid && IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool()
            };
            if !shown {
                return true.into();
            }
            let w = BrowserWindow { hwnd: hwnd.0 as isize, fullscreen: fills_monitor(hwnd) };
            search.found = Some(w);
            (!w.fullscreen).into()
        }
        let mut search = Search { pid, found: None };
        // SAFETY: the callback only touches `search` through the pointer passed here.
        let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize)) };
        search.found
    }

    fn fills_monitor(hwnd: HWND) -> bool {
        let mut r = RECT::default();
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        // SAFETY: plain Win32 queries; both structs outlive the calls.
        unsafe {
            if GetWindowRect(hwnd, &mut r).is_err() {
                return false;
            }
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return false;
            }
        }
        let m = info.rcMonitor;
        (r.left, r.top, r.right, r.bottom) == (m.left, m.top, m.right, m.bottom)
    }

    /// Moves the launcher right behind `window` in the z-order (or to the bottom), without
    /// taking focus from it.
    pub fn put_behind(launcher: isize, window: Option<isize>) {
        let after = window.map_or(HWND_BOTTOM, |w| HWND(w as *mut _));
        // SAFETY: window handles we read; a stale one makes this fail harmlessly.
        let _ = unsafe {
            SetWindowPos(HWND(launcher as *mut _), Some(after), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE)
        };
    }

    /// Gives `window` the keyboard focus. The launcher was in front until now, which is what
    /// lets Windows allow it.
    pub fn focus(window: isize) {
        // SAFETY: a window handle we read; a stale one makes this fail harmlessly.
        let _ = unsafe { SetForegroundWindow(HWND(window as *mut _)) };
    }
}
