//! The launcher's own video player. mpv plays what a browser can't (MKV, HEVC, DTS, image-based
//! subtitles) and decodes on this PC's GPU; it runs behind the launcher's window, which turns
//! transparent while it plays (Player.tsx draws the controls there). So the pad, the mouse, the
//! assistant and the ring screen work over a video as they do on any other screen.
//!
//! This module only runs mpv: it starts it, tells it what to do over its JSON pipe, and keeps what
//! it reports. What's being played, and how far it got, is watch/library.rs's.

mod install;
mod ipc;

use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

use crate::error::{Error, Result};
use crate::focus_win;
use crate::launcher;
use crate::state::AppState;

pub use install::installed;

/// The screen hears how it's going this often, at most.
const REPORT: Duration = Duration::from_millis(250);

/// What mpv reports, as Player.tsx shows it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PlayerState {
    pub title: String,
    /// "S1 · E3": what's playing of a series.
    pub subtitle: Option<String>,
    pub position: f64,
    pub duration: f64,
    pub paused: bool,
    /// Waiting for the network.
    pub buffering: bool,
    /// Seconds already fetched ahead of the position.
    pub cached: f64,
    pub volume: f64,
    pub ended: bool,
    pub audio: Vec<Track>,
    pub subtitles: Vec<Track>,
    /// "S1 · E4", when the library has the next episode.
    pub next: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Track {
    pub id: i64,
    pub name: String,
    pub on: bool,
}

/// What the screen asks of the player.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Control {
    Toggle,
    /// Seconds from here.
    Seek { by: f64 },
    SeekTo { at: f64 },
    Volume { by: f64 },
    Audio { id: i64 },
    /// None: no subtitles.
    Subtitle { id: Option<i64> },
    Next,
    Close,
}

#[derive(Default)]
pub struct State {
    /// Commands for the running mpv, by which run it is: one that just ended mustn't take the
    /// next one's away.
    tx: Mutex<Option<(u64, mpsc::UnboundedSender<String>)>>,
    runs: AtomicU64,
    now: Mutex<PlayerState>,
}

/// What to play.
pub struct Opening {
    pub url: String,
    /// Seconds to start from.
    pub start: f64,
    /// Subtitle files: address and language.
    pub subtitles: Vec<(String, String)>,
    pub title: String,
    pub subtitle: Option<String>,
    pub next: Option<String>,
    pub language: String,
}

fn command(args: Value) -> String {
    format!("{}\n", json!({ "command": args }))
}

fn send(app: &AppHandle, args: Value) -> bool {
    let st = app.state::<AppState>();
    let tx = st.player.tx.lock().unwrap();
    tx.as_ref().is_some_and(|(_, t)| t.send(command(args)).is_ok())
}

pub fn running(app: &AppHandle) -> bool {
    app.state::<AppState>().player.tx.lock().unwrap().is_some()
}

pub fn snapshot(app: &AppHandle) -> PlayerState {
    app.state::<AppState>().player.now.lock().unwrap().clone()
}

pub fn control(app: &AppHandle, c: Control) {
    match c {
        Control::Toggle => send(app, json!(["cycle", "pause"])),
        Control::Seek { by } => send(app, json!(["seek", by, "relative"])),
        Control::SeekTo { at } => send(app, json!(["seek", at, "absolute"])),
        Control::Volume { by } => send(app, json!(["add", "volume", by])),
        Control::Audio { id } => send(app, json!(["set_property", "aid", id])),
        Control::Subtitle { id: Some(id) } => send(app, json!(["set_property", "sid", id])),
        Control::Subtitle { id: None } => send(app, json!(["set_property", "sid", "no"])),
        Control::Next => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = crate::watch::library::play_next(&app).await {
                    let _ = app.emit("player-error", e.to_string());
                }
            });
            true
        }
        Control::Close => {
            crate::watch::close_soon(app);
            true
        }
    };
}

/// Pauses the video for a ring; true when it was playing, so it's to be started again after.
pub fn pause_for_ring(app: &AppHandle) -> bool {
    let playing = running(app) && !snapshot(app).paused;
    if playing {
        send(app, json!(["set_property", "pause", true]));
    }
    playing
}

pub fn resume_after_ring(app: &AppHandle) {
    send(app, json!(["set_property", "pause", false]));
}

/// "pt-BR" → what a track of that language may call itself, then English.
fn languages(language: &str) -> String {
    let base = language.split('-').next().unwrap_or(language).to_ascii_lowercase();
    let mut all = vec![language.to_string()];
    if base == "pt" {
        all.extend(["pob", "pb", "pt", "por"].map(String::from));
    } else {
        all.push(base);
    }
    all.extend(["en", "eng"].map(String::from));
    all.join(",")
}

/// mpv's options: everything of its own (keys, on-screen controls, the user's config) off, since
/// the launcher is its interface. A long read-ahead rides out the Wi-Fi.
fn args(o: &Opening, pipe: &str, log: &std::path::Path) -> Vec<String> {
    let mut out: Vec<String> = [
        "--no-config",
        "--fullscreen",
        "--force-window=yes",
        "--no-terminal",
        "--no-osc",
        "--no-osd-bar",
        "--osd-level=0",
        "--no-input-default-bindings",
        "--input-vo-keyboard=no",
        "--no-input-cursor",
        "--cursor-autohide=always",
        "--keep-open=yes",
        "--no-ytdl",
        "--load-scripts=no",
        "--audio-display=no",
        "--hwdec=auto-safe",
        "--vo=gpu-next",
        "--volume=100",
        "--volume-max=100",
        "--cache=yes",
        "--cache-secs=120",
        "--demuxer-max-bytes=512MiB",
        "--demuxer-max-back-bytes=128MiB",
        "--network-timeout=30",
    ]
    .map(String::from)
    .into();
    out.push(format!("--slang={}", languages(&o.language)));
    out.push(format!("--input-ipc-server={pipe}"));
    out.push(format!("--log-file={}", log.display()));
    out.push(format!("--start={}", o.start.max(0.0)));
    out.push(o.url.clone());
    out
}

/// Whether a sidecar of language `lang` is the one to show, for a house speaking `language`.
fn wanted_subtitle(lang: &str, language: &str) -> bool {
    let base = |l: &str| l.split(['-', '.']).next().unwrap_or(l).to_ascii_lowercase();
    !lang.is_empty() && base(lang) == base(language)
}

/// Starts mpv on `o`, behind the launcher, which stays in front of it. Whatever ran before stops.
pub async fn start(app: &AppHandle, o: Opening) -> Result<()> {
    let exe = install::ensure(app).await?;
    let st = app.state::<AppState>();
    let pipe = format!(r"\\.\pipe\tvl-mpv-{}", tvl_kit::oauth::random_string(12));
    let log_dir = st.data_dir.join("logs");
    std::fs::create_dir_all(&log_dir)?;
    let mut cmd = Command::new(exe);
    cmd.args(args(&o, &pipe, &log_dir.join("mpv.log")));

    let (tx, rx) = mpsc::unbounded_channel();
    // Picked from the sidecars before anything's loaded: mpv only takes them once the file is.
    let mut first = true;
    let extra: Vec<String> = o
        .subtitles
        .iter()
        .map(|(url, lang)| {
            let pick = first && wanted_subtitle(lang, &o.language);
            first &= !pick;
            command(json!(["sub-add", url, if pick { "select" } else { "auto" }, lang, lang]))
        })
        .collect();
    *st.player.now.lock().unwrap() = PlayerState {
        title: o.title,
        subtitle: o.subtitle,
        next: o.next,
        volume: 100.0,
        buffering: true,
        ..Default::default()
    };

    focus_win::cover(app);
    let pid = launcher::spawn_player(app, cmd).inspect_err(|_| focus_win::uncover(app))?;
    focus_win::stay_over(app, pid);

    let reader = match ipc::connect(&pipe).await {
        Ok(p) => p,
        Err(e) => {
            launcher::close_if(app, crate::watch::library::SERVICE);
            return Err(Error::msg(format!("The player didn't start: {e}")));
        }
    };
    let id = st.player.runs.fetch_add(1, Ordering::SeqCst);
    *st.player.tx.lock().unwrap() = Some((id, tx));
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        run(&app, reader, rx, extra).await;
        let st = app.state::<AppState>();
        let mut tx = st.player.tx.lock().unwrap();
        if tx.as_ref().is_some_and(|(run, _)| *run == id) {
            *tx = None;
        }
    });
    Ok(())
}

/// Follows mpv until its pipe closes.
async fn run(app: &AppHandle, pipe: ipc::Pipe, mut rx: mpsc::UnboundedReceiver<String>, extra: Vec<String>) {
    let (mut lines, mut out) = pipe.split();
    let mut report = tokio::time::interval(REPORT);
    report.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut said = PlayerState::default();
    let mut extra = Some(extra);
    for (i, name) in ipc::WATCHED.iter().enumerate() {
        let _ = out.send(&command(json!(["observe_property", i + 1, name]))).await;
    }
    loop {
        tokio::select! {
            line = lines.next() => {
                let Some(line) = line else { break };
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                if v["event"] == "file-loaded" {
                    for c in extra.take().into_iter().flatten() {
                        let _ = out.send(&c).await;
                    }
                }
                if v["event"] == "end-file" && v["reason"] == "error" {
                    let _ = app.emit("player-error", "That file wouldn't play");
                    crate::watch::close_soon(app);
                }
                let st = app.state::<AppState>();
                ipc::apply(&mut st.player.now.lock().unwrap(), &v);
            }
            c = rx.recv() => {
                let Some(c) = c else { break };
                if out.send(&c).await.is_err() {
                    break;
                }
            }
            _ = report.tick() => {
                let now = snapshot(app);
                if now != said {
                    let _ = app.emit("player", &now);
                    crate::watch::library::on_state(app, &now);
                    said = now;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opening() -> Opening {
        Opening {
            url: "http://server.local:47960/api/media/x".into(),
            start: 90.5,
            subtitles: vec![],
            title: "Dune".into(),
            subtitle: None,
            next: None,
            language: "pt-BR".into(),
        }
    }

    #[test]
    fn mpv_gets_the_address_last_and_nothing_of_its_own_interface() {
        let a = args(&opening(), r"\\.\pipe\x", std::path::Path::new("mpv.log"));
        assert_eq!(a.last().unwrap(), "http://server.local:47960/api/media/x");
        for off in ["--no-config", "--no-osc", "--no-input-default-bindings", "--input-vo-keyboard=no"] {
            assert!(a.iter().any(|x| x == off), "{off}");
        }
        assert!(a.iter().any(|x| x == "--start=90.5"));
        assert!(a.iter().any(|x| x == r"--input-ipc-server=\\.\pipe\x"));
    }

    #[test]
    fn subtitles_prefer_the_houses_language_in_every_spelling() {
        assert_eq!(languages("pt-BR"), "pt-BR,pob,pb,pt,por,en,eng");
        assert_eq!(languages("es"), "es,es,en,eng");
    }

    #[test]
    fn a_sidecar_in_the_houses_language_is_the_one_picked() {
        assert!(wanted_subtitle("pt-BR", "pt-BR"));
        assert!(wanted_subtitle("pt", "pt-BR"));
        assert!(wanted_subtitle("pt.hi", "pt-BR"));
        assert!(!wanted_subtitle("en", "pt-BR"));
        assert!(!wanted_subtitle("", "pt-BR"));
    }

    #[test]
    fn commands_are_one_json_line() {
        assert_eq!(command(json!(["cycle", "pause"])), "{\"command\":[\"cycle\",\"pause\"]}\n");
    }

    #[test]
    fn the_screen_says_what_it_wants_by_kind() {
        let c: Control = serde_json::from_value(json!({ "kind": "seek_to", "at": 12.5 })).unwrap();
        assert!(matches!(c, Control::SeekTo { at } if at == 12.5));
        let c: Control = serde_json::from_value(json!({ "kind": "subtitle", "id": null })).unwrap();
        assert!(matches!(c, Control::Subtitle { id: None }));
    }
}
