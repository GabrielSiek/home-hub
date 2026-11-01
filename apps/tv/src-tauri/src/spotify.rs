//! Spotify lives on the home server. What's left here is what only this PC can do: start the
//! Spotify desktop app, which is the player the server tells Spotify to use, and the alarm's way
//! to the music, through the server.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::error::{Error, Result};
use crate::focus_win;
use crate::state::{AppState, Mode};

/// What the alarm asks of the music.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Control {
    Play,
    Pause,
    Volume(u8),
}

/// The music, when it's on this PC's Spotify app.
#[derive(Debug, Clone, Copy)]
pub struct Here {
    pub playing: bool,
    pub volume: Option<u8>,
}

fn latest() -> &'static Mutex<Option<Here>> {
    static LATEST: OnceLock<Mutex<Option<Here>>> = OnceLock::new();
    LATEST.get_or_init(Mutex::default)
}

/// The name of this PC's Connect device: the Spotify app names it after the computer.
pub fn device_name() -> Option<String> {
    std::env::var("COMPUTERNAME").ok()
}

/// The server's `spotify` event: while music plays one comes every few seconds.
pub fn heard(snapshot: &Value) {
    let player = &snapshot["player"];
    let device = &player["device"];
    let local = device["name"].as_str().is_some_and(|n| device_name().is_some_and(|pc| pc.eq_ignore_ascii_case(n.trim())));
    *latest().lock().unwrap() = local.then(|| Here {
        playing: player["playing"].as_bool().unwrap_or(false),
        volume: device["volume"].as_u64().map(|v| v.min(100) as u8),
    });
}

pub fn here() -> Option<Here> {
    *latest().lock().unwrap()
}

pub async fn control(app: &AppHandle, control: Control) -> Result<()> {
    let args = json!({ "control": control });
    crate::agent::server_call(app, "spotify_control", args).await.map(|_| ()).map_err(Error::msg)
}

/// Plays `uri` on this PC.
pub async fn play(app: &AppHandle, uri: &str) -> Result<()> {
    let args = json!({ "uri": uri });
    crate::agent::server_call(app, "spotify_play", args).await.map(|_| ()).map_err(Error::msg)
}

/// Starts the Spotify desktop app: the classic installer's, else the Microsoft Store one
/// (through its command alias), else whatever handles spotify: links.
pub fn start_desktop(app: &AppHandle) -> Result<()> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let candidates = [
        env("APPDATA").map(|d| d.join(r"Spotify\Spotify.exe")),
        env("LOCALAPPDATA").map(|d| d.join(r"Microsoft\WindowsApps\Spotify.exe")),
    ];
    // symlink_metadata: the Store alias is a reparse point that plain metadata can't open.
    for exe in candidates.into_iter().flatten().filter(|p| std::fs::symlink_metadata(p).is_ok()) {
        if std::process::Command::new(&exe).arg("--minimized").spawn().is_ok() {
            return Ok(());
        }
    }
    app.opener()
        .open_url("spotify:", None::<&str>)
        .map_err(|_| Error::msg("The Spotify app isn't installed on this PC"))
}

/// The desktop app showed up: the launcher comes back in front of it.
pub fn started(app: &AppHandle) {
    if app.state::<AppState>().mode() == Mode::Launcher {
        focus_win::bring_to_front(app);
    }
}
