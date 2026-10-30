use std::path::PathBuf;
use std::sync::{Mutex, RwLock};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::assistant::AssistantState;
use crate::cdp::Link;
use crate::config::Config;
use crate::games::GamesState;
use crate::history::History;
use crate::livetv;
use crate::photos;
use crate::recs::RecsState;
use crate::retro;
use crate::spotify;
use crate::sports;
use crate::twitch;
use crate::watch::WatchState;
use crate::youtube;

/// Who owns the controller right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// The launcher UI is in front and receives every action.
    Launcher,
    /// A launched app (browser) is in front; the pad is bridged to keyboard/mouse.
    Streaming,
    /// A game is running; only Home is acted on.
    Game,
    /// The launcher's own player has a video on, under the launcher's window. The pad works as on
    /// any screen; Home closes it.
    Player,
}

pub struct Running {
    pub pid: u32,
    pub app_id: String,
    /// Signalled when the process exits.
    pub exited: std::sync::mpsc::Receiver<()>,
}

pub struct AppState {
    pub data_dir: PathBuf,
    pub config: RwLock<Config>,
    pub history: Mutex<History>,
    pub mode: Mutex<Mode>,
    pub running: Mutex<Option<Running>>,
    /// A closed app still shutting down behind the launcher (launcher::close).
    pub stopping: Mutex<Option<std::thread::JoinHandle<()>>>,
    /// DevTools link to the running browser, while there is one.
    pub cdp: Mutex<Option<Link>>,
    pub pad: Mutex<Option<String>>,
    pub games: GamesState,
    pub retro: retro::State,
    pub spotify: spotify::State,
    pub twitch: twitch::State,
    pub assistant: AssistantState,
    pub watch: WatchState,
    pub player: crate::mpv::State,
    pub recs: RecsState,
    pub photos: photos::State,
    pub sports: sports::State,
    pub livetv: livetv::State,
    pub youtube: youtube::State,
    /// Timers and alarms ringing here (ring/).
    pub ring: crate::ring::State,
    /// What a phone played on this TV, shown until the TV moves on (device/tv.rs).
    pub shown: Mutex<Option<crate::device::tv::Shown>>,
    pub http: reqwest::Client,
}

impl AppState {
    pub fn new(data_dir: PathBuf, config: Config) -> Self {
        Self {
            history: Mutex::new(History::load(&data_dir)),
            games: GamesState::new(&data_dir),
            retro: retro::State::new(&data_dir),
            spotify: spotify::State::new(&data_dir),
            twitch: twitch::State::new(&data_dir),
            assistant: AssistantState::default(),
            watch: WatchState::new(&data_dir),
            player: crate::mpv::State::default(),
            recs: RecsState::new(&data_dir),
            photos: photos::State::new(&data_dir),
            sports: sports::State::new(&data_dir),
            livetv: livetv::State::new(&data_dir),
            youtube: youtube::State::new(&data_dir),
            ring: crate::ring::State::default(),
            shown: Mutex::new(None),
            data_dir,
            config: RwLock::new(config),
            mode: Mutex::new(Mode::Launcher),
            running: Mutex::new(None),
            stopping: Mutex::new(None),
            cdp: Mutex::new(None),
            pad: Mutex::new(None),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn mode(&self) -> Mode {
        *self.mode.lock().unwrap()
    }
}

pub fn set_mode(app: &AppHandle, mode: Mode) {
    *app.state::<AppState>().mode.lock().unwrap() = mode;
    let _ = app.emit("mode", mode);
}
