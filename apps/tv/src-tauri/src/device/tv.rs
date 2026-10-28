//! This TV as phones and tablets see it (through the home server): what's on (the Hub's "On the
//! TV", the remote's header), the speakers' volume, the picture off, and the remote's keys.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::assistant::system;
use crate::error::{Error, Result};
use crate::input::{self, Action, Extra, Remote};
use crate::state::{AppState, Mode};

/// What the companion played, shown until the TV moves on to another app.
#[derive(Debug, Clone, Deserialize)]
pub struct Showing {
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub live: bool,
}

#[derive(Debug, Clone)]
pub struct Shown {
    /// The app it opened in.
    app: String,
    show: Showing,
}

#[derive(Debug, Clone, Serialize)]
pub struct TvNow {
    pub mode: Mode,
    /// The app in front, from apps.toml.
    pub app: Option<String>,
    pub app_name: Option<String>,
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub image: Option<String>,
    pub live: bool,
    /// The speakers, 0–100; None when Windows won't say.
    pub volume: Option<u8>,
    pub muted: bool,
}

pub fn now(app: &AppHandle) -> TvNow {
    let st = app.state::<AppState>();
    let mode = st.mode();
    let running = st.running.lock().unwrap().as_ref().map(|r| r.app_id.clone());
    let app_name = running.as_ref().and_then(|id| {
        let cfg = st.config.read().unwrap();
        cfg.apps.iter().find(|a| &a.id == id).map(|a| a.name.clone())
    });
    let (volume, muted) = match system::volume(|_| {}) {
        Ok(v) => (Some(v.level), v.muted),
        Err(_) => (None, false),
    };
    let mut tv = TvNow { mode, app: running.clone(), app_name, title: None, subtitle: None, image: None, live: false, volume, muted };
    if mode == Mode::Launcher {
        return tv;
    }
    if let Some(w) = crate::watch::playing(app) {
        tv.title = Some(w.title);
        tv.subtitle = match (w.media_type.as_str(), &tv.app_name) {
            ("tv", Some(name)) if w.season > 0 => Some(format!("S{} E{} · {name}", w.season, w.episode)),
            _ => tv.app_name.clone(),
        };
        tv.image = w.backdrop;
        return tv;
    }
    let shown = st.shown.lock().unwrap().clone();
    if let Some(s) = shown.filter(|s| running.as_deref() == Some(s.app.as_str())) {
        tv.title = Some(s.show.title);
        tv.subtitle = s.show.subtitle.or(tv.app_name.clone());
        tv.image = s.show.image;
        tv.live = s.show.live;
    }
    tv
}

/// Tells the screens following this TV what's on now (the agent passes "tv" on).
pub fn emit(app: &AppHandle) {
    let _ = app.emit("tv", now(app));
}

/// Keeps `show` as what's on the TV, once the command that played it returned.
pub fn showing(app: &AppHandle, show: Showing) {
    let st = app.state::<AppState>();
    let running = st.running.lock().unwrap().as_ref().map(|r| r.app_id.clone());
    *st.shown.lock().unwrap() = running.map(|app| Shown { app, show });
    emit(app);
}

/// Sets the speakers' level (which unmutes) and/or mute.
pub fn volume(app: &AppHandle, level: Option<u8>, muted: Option<bool>) -> Result<TvNow> {
    system::volume(|v| {
        if let Some(l) = level {
            v.level = l.min(100);
            v.muted = false;
        }
        if let Some(m) = muted {
            v.muted = m;
        }
    })?;
    let tv = now(app);
    let _ = app.emit("tv", &tv);
    Ok(tv)
}

/// The remote's keys, as the companion names them.
#[derive(Debug, Clone, Copy, Deserialize)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Select,
    Back,
    Home,
    /// Y: the focused item's options.
    Menu,
    PlayPause,
    ChannelUp,
    ChannelDown,
}

/// Presses `key` as the 8BitDo would. Play/pause is X in a web app; on the launcher's own
/// screens (where X opens the assistant) it's the media key, for the music.
pub fn key(app: &AppHandle, key: Key) -> Result<()> {
    let mode = app.state::<AppState>().mode();
    let r = match key {
        Key::Up => Remote::Action(Action::Up),
        Key::Down => Remote::Action(Action::Down),
        Key::Left => Remote::Action(Action::Left),
        Key::Right => Remote::Action(Action::Right),
        Key::Select => Remote::Action(Action::Select),
        Key::Back => Remote::Action(Action::Back),
        Key::Home => Remote::Action(Action::Home),
        Key::Menu => Remote::Extra(Extra::Y),
        Key::ChannelUp => Remote::Extra(Extra::Rb),
        Key::ChannelDown => Remote::Extra(Extra::Lb),
        Key::PlayPause if mode == Mode::Streaming => Remote::Extra(Extra::X),
        Key::PlayPause => return system::media_key(system::MediaKey::PlayPause),
    };
    if input::remote(r) {
        Ok(())
    } else {
        Err(Error::msg("The launcher isn't reading input right now"))
    }
}

/// Types `text` wherever the keyboard is on the TV: a search box, a site's field.
pub fn text(text: &str) -> Result<()> {
    use enigo::{Enigo, Keyboard};
    let mut enigo = Enigo::new(&enigo::Settings::default()).map_err(|e| Error::msg(e.to_string()))?;
    enigo.text(text).map_err(|e| Error::msg(e.to_string()))
}

/// Turns the picture off: the displays sleep until the next mouse or keyboard input (the
/// remote's keys count).
#[cfg(windows)]
pub fn off() -> Result<()> {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, HWND_BROADCAST, SC_MONITORPOWER, WM_SYSCOMMAND};
    // SAFETY: a broadcast window message; 2 = power off.
    unsafe { PostMessageW(Some(HWND_BROADCAST), WM_SYSCOMMAND, WPARAM(SC_MONITORPOWER as usize), LPARAM(2)) }
        .map_err(|e| Error::msg(format!("TV off: {}", e.message())))
}

#[cfg(not(windows))]
pub fn off() -> Result<()> {
    Err(Error::msg("Turning the TV off works on Windows only"))
}
