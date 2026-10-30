//! Keeps the mouse pointer out of the way: while the launcher is in front, a pointer left
//! over a tile would hover-focus it. When the launcher comes to the front, and again once the
//! mouse has sat still for a while, the pointer is moved to the top-right corner and hidden
//! (the webview gets a `cursor` event). Moving the mouse shows it again.

use std::time::{Duration, Instant};

use tauri::AppHandle;

/// How long the mouse sits still before it's put away again.
pub const IDLE: Duration = Duration::from_secs(3);
#[cfg(windows)]
const POLL: Duration = Duration::from_millis(150);

/// What to do with the pointer after a poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Move it to the corner and hide it.
    Park,
    /// It moved, or the launcher lost the front: show it.
    Show,
}

/// Decides when to park the pointer, from where it is on each poll.
pub struct Tracker {
    active: bool,
    hidden: bool,
    last: Option<(i32, i32)>,
    still_since: Instant,
}

impl Tracker {
    pub fn new(now: Instant) -> Self {
        Self { active: false, hidden: false, last: None, still_since: now }
    }

    /// `active`: the setting is on and the launcher is in front. `pos`: the pointer, if readable.
    pub fn update(&mut self, active: bool, pos: Option<(i32, i32)>, now: Instant) -> Option<Step> {
        let became_active = active && !self.active;
        self.active = active;
        if !active {
            self.last = pos;
            return std::mem::take(&mut self.hidden).then_some(Step::Show);
        }
        if became_active {
            return self.park(now);
        }
        if pos != self.last {
            self.last = pos;
            self.still_since = now;
            return std::mem::take(&mut self.hidden).then_some(Step::Show);
        }
        if !self.hidden && now.duration_since(self.still_since) >= IDLE {
            return self.park(now);
        }
        None
    }

    /// Call with where the pointer ended up after a `Park`, so that isn't taken as a move.
    pub fn parked_at(&mut self, pos: Option<(i32, i32)>) {
        self.last = pos;
    }

    fn park(&mut self, now: Instant) -> Option<Step> {
        self.hidden = true;
        self.still_since = now;
        Some(Step::Park)
    }
}

#[cfg(windows)]
pub fn spawn(app: AppHandle) {
    use tauri::{Emitter, Manager};

    use crate::state::{AppState, Mode};

    let Some(hwnd) = app.get_webview_window("main").and_then(|w| w.hwnd().ok()) else {
        return;
    };
    // HWND isn't Send; the handle itself stays valid for the launcher's lifetime.
    let hwnd = hwnd.0 as isize;
    let _ = std::thread::Builder::new().name("cursor".into()).spawn(move || {
        let mut tracker = Tracker::new(Instant::now());
        loop {
            std::thread::sleep(POLL);
            let state = app.state::<AppState>();
            let enabled = state.config.read().unwrap().settings.hide_cursor;
            // The player's window may hold the foreground; the launcher is in front of it anyway.
            let active = enabled && (state.mode() == Mode::Player || (state.mode() == Mode::Launcher && win::is_foreground(hwnd)));
            match tracker.update(active, win::cursor_pos(), Instant::now()) {
                Some(Step::Park) => {
                    win::park(hwnd);
                    tracker.parked_at(win::cursor_pos());
                    let _ = app.emit("cursor", true);
                }
                Some(Step::Show) => {
                    let _ = app.emit("cursor", false);
                }
                None => {}
            }
        }
    });
}

#[cfg(not(windows))]
pub fn spawn(_app: AppHandle) {}

/// For the browser about to open over the launcher: a pointer on the top edge (where it's
/// parked) brings down the title bar of a full-screen browser window, so it goes halfway down
/// the right edge instead.
#[cfg(windows)]
pub fn off_top(app: &AppHandle) {
    use tauri::Manager;

    if let Some(hwnd) = app.get_webview_window("main").and_then(|w| w.hwnd().ok()) {
        win::off_top(hwnd.0 as isize);
    }
}

#[cfg(not(windows))]
pub fn off_top(_app: &AppHandle) {}

#[cfg(windows)]
mod win {
    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow, GetWindowRect, SetCursorPos};

    pub fn is_foreground(hwnd: isize) -> bool {
        // SAFETY: plain Win32 query.
        unsafe { GetForegroundWindow() }.0 as isize == hwnd
    }

    /// None on the secure desktop (lock screen, UAC), where it can't be read.
    pub fn cursor_pos() -> Option<(i32, i32)> {
        let mut p = POINT::default();
        // SAFETY: writes into the local POINT.
        unsafe { GetCursorPos(&mut p) }.ok().map(|_| (p.x, p.y))
    }

    /// Moves the pointer to the launcher window's top-right corner (the screen's, in full screen).
    pub fn park(hwnd: isize) {
        let mut r = RECT::default();
        // SAFETY: the launcher's own window handle; writes into the local RECT.
        unsafe {
            if GetWindowRect(HWND(hwnd as _), &mut r).is_ok() {
                let _ = SetCursorPos(r.right - 1, r.top);
            }
        }
    }

    /// Moves a pointer on the window's top edge to the middle of its right edge.
    pub fn off_top(hwnd: isize) {
        let mut r = RECT::default();
        // SAFETY: the launcher's own window handle; writes into the local RECT.
        unsafe {
            if GetWindowRect(HWND(hwnd as _), &mut r).is_err() {
                return;
            }
            if cursor_pos().is_some_and(|(_, y)| y <= r.top) {
                let _ = SetCursorPos(r.right - 1, r.top + (r.bottom - r.top) / 2);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parks_on_coming_to_front_then_after_idle() {
        let t0 = Instant::now();
        let mut t = Tracker::new(t0);
        assert_eq!(t.update(false, Some((5, 5)), t0), None);

        // The launcher comes to the front: put away at once.
        assert_eq!(t.update(true, Some((5, 5)), t0), Some(Step::Park));
        t.parked_at(Some((1919, 0)));
        assert_eq!(t.update(true, Some((1919, 0)), t0 + IDLE * 2), None);

        // Moving shows it; it stays while moving, and goes once still for IDLE.
        assert_eq!(t.update(true, Some((800, 400)), t0 + IDLE * 2), Some(Step::Show));
        let moved = t0 + IDLE * 2 + Duration::from_secs(1);
        assert_eq!(t.update(true, Some((810, 400)), moved), None);
        assert_eq!(t.update(true, Some((810, 400)), moved + IDLE / 2), None);
        assert_eq!(t.update(true, Some((810, 400)), moved + IDLE), Some(Step::Park));
    }

    #[test]
    fn shows_when_leaving_the_front_and_parks_on_return() {
        let t0 = Instant::now();
        let mut t = Tracker::new(t0);
        assert_eq!(t.update(true, Some((5, 5)), t0), Some(Step::Park));
        t.parked_at(Some((1919, 0)));

        // An app opens (or the setting is turned off): the pointer is shown, then left alone.
        assert_eq!(t.update(false, Some((1919, 0)), t0), Some(Step::Show));
        assert_eq!(t.update(false, Some((300, 300)), t0 + IDLE * 2), None);

        // Back to the launcher: wherever it was, it's put away.
        assert_eq!(t.update(true, Some((300, 300)), t0 + IDLE * 2), Some(Step::Park));
    }
}
