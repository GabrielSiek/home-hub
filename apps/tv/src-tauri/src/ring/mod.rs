//! Timers and alarms ringing on this TV PC. The home server decides what rings where and sends this
//! TV PC its whole ringing state (`ring.set`) on every change and whenever it reconnects; here they
//! make their effects, as the old launcher's clock did:
//! - The bell plays on the PC's speakers (sound.rs), unmuted for it. An alarm can play a Spotify
//!   playlist instead, from quiet to its usual volume over a minute.
//! - Spotify music playing on this PC is paused meanwhile, and so is the video of a web app.
//! - On the launcher, the ring screen takes the pad (ClockRing.tsx). Over a web app, ring.js shows
//!   the same screen in its page, and the pad's A, B, X and Y answer it (input.rs asks `press`).
//! - During a game only the bell sounds: Home brings the launcher, with the ring screen.
//!
//! Fail-safe: the timers and alarms the server last sent are kept. One aimed at this TV that comes
//! due while the server can't be reached rings here anyway, and the server is told once it's back
//! (`clock_rang_locally`), so it isn't rung twice.

mod sound;

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tvl_protocol::clock::{Ring, RingKind as Kind, RingSet, RingTarget, Sound};

use crate::assistant::system::{self as pc, Volume};
use crate::cdp::{self, Cmd};
use crate::history::now_ms;
use crate::spotify;
use crate::state::{AppState, Mode};
use sound::Tone;

const RING_SCRIPT: &str = include_str!("ring.js");
const MIN: i64 = 60_000;
/// Nothing rings for longer.
const RING_FOR: i64 = 10 * MIN;
/// The fail-safe rings what's just due, not what was due long ago.
const MISSED: i64 = 10 * MIN;
/// An alarm's playlist starts this loud and reaches its usual volume over a minute.
const PLAYLIST_FROM: u8 = 10;
/// The page over a web app is told about the ring again this often: it may have navigated.
const OVERLAY_EVERY: i64 = 2000;
const ALARM_COLOR: &str = "#ffd36b";

/// What ringing changed, undone once nothing rings.
#[derive(Default)]
struct Effects {
    on: bool,
    /// The web app whose video was paused.
    video: Option<String>,
    /// The launcher's own player was paused.
    player: bool,
    /// Spotify's music was paused.
    music: bool,
    /// The speakers before they were unmuted or turned up.
    speakers: Option<Volume>,
    /// An alarm's playlist is playing, with Spotify's volume from before.
    playlist: Option<Option<u8>>,
    /// Bumped as ringing starts and ends, so a slow start from before gives up.
    generation: u64,
    /// When the page over the app was last told.
    overlay_at: i64,
}

#[derive(Default)]
struct Inner {
    /// The newest `ring.set` taken (the server's count; it starts again with a new connection).
    generation: u64,
    /// What the server says rings here.
    rings: Vec<Ring>,
    /// Rung here by the fail-safe.
    local: Vec<Ring>,
    /// Fail-safe rings the server hasn't been told about: (id, when it was due).
    unreported: Vec<(String, i64)>,
    /// What the fail-safe already rang, so it rings each once.
    fired: HashSet<(String, i64)>,
    /// The server's last `clock` snapshot: the UI's, and the fail-safe's timetable.
    clock: Option<Value>,
    fx: Effects,
}

#[derive(Default)]
pub struct State {
    inner: Mutex<Inner>,
    bell: sound::Bell,
}

fn state(app: &AppHandle) -> &State {
    &app.state::<AppState>().inner().ring
}

/// What rings here now, the first one on screen, each with what it's doing on this TV.
pub fn mine(app: &AppHandle) -> Vec<Ring> {
    let name = playing_app(app);
    let inner = state(app).inner.lock().unwrap();
    inner
        .rings
        .iter()
        .chain(inner.local.iter())
        .map(|r| Ring { note: note(r, &inner.fx, name.as_deref()), ..r.clone() })
        .collect()
}

pub fn ringing(app: &AppHandle) -> bool {
    let inner = state(app).inner.lock().unwrap();
    !inner.rings.is_empty() || !inner.local.is_empty()
}

/// The server's note when it rings somewhere it wasn't meant to ("No TV is on in Quarto"), else
/// what ringing does on this TV.
fn note(r: &Ring, fx: &Effects, app: Option<&str>) -> String {
    if !r.note.is_empty() {
        return r.note.clone();
    }
    match (&r.kind, &r.sound) {
        (Kind::Alarm, Sound::Spotify { name, .. }) => format!("Spotify plays “{name}”, getting louder for a minute"),
        (Kind::Alarm, Sound::Chime) => "The chime gets louder for a minute".into(),
        (Kind::Timer, _) if fx.video.is_some() => {
            format!("{} is paused until you stop the timer", app.or(fx.video.as_deref()).unwrap_or("The video"))
        }
        (Kind::Timer, _) if fx.music => "Spotify is paused until you stop the timer".into(),
        (Kind::Timer, _) => "Stops by itself after 10 minutes".into(),
    }
}

/// The name of the web app in front.
fn playing_app(app: &AppHandle) -> Option<String> {
    let st = app.state::<AppState>();
    let id = st.running.lock().unwrap().as_ref().map(|r| r.app_id.clone())?;
    let name = st.config.read().unwrap().apps.iter().find(|a| a.id == id).map(|a| a.name.clone());
    Some(name.unwrap_or(id))
}

/// The server's `clock` event (or `clock_get`): kept, and passed to the UI with only what rings
/// here.
pub fn clock(app: &AppHandle, snapshot: Value) {
    state(app).inner.lock().unwrap().clock = Some(snapshot);
    emit(app);
}

/// The last clock snapshot, with this TV's rings: what `clock_get` answers while the server is
/// out of reach.
pub fn snapshot(app: &AppHandle) -> Option<Value> {
    let mut clock = state(app).inner.lock().unwrap().clock.clone()?;
    clock["ringing"] = json!(mine(app));
    Some(clock)
}

fn emit(app: &AppHandle) {
    if let Some(clock) = snapshot(app) {
        let _ = app.emit("clock", clock);
    }
}

/// The server's `ring.set`: everything that rings here now.
pub fn set(app: &AppHandle, set: RingSet) {
    {
        let mut inner = state(app).inner.lock().unwrap();
        if set.generation < inner.generation {
            return;
        }
        inner.generation = set.generation;
        let ids: Vec<&str> = set.rings.iter().map(|r| r.id.as_str()).collect();
        // The server rings it now: not twice.
        inner.local.retain(|r| !ids.contains(&r.id.as_str()));
        inner.rings = set.rings;
    }
    sync(app);
    emit(app);
}

/// Connected to the server (again): its ring.set count starts over, and what rang here meanwhile
/// is reported.
pub fn connected(app: &AppHandle) {
    let unreported = {
        let mut inner = state(app).inner.lock().unwrap();
        inner.generation = 0;
        std::mem::take(&mut inner.unreported)
    };
    if unreported.is_empty() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        for (id, at) in unreported {
            if let Err(e) = crate::agent::server_call(&app, "clock_rang_locally", json!({ "id": id, "at": at })).await {
                tracing::warn!("couldn't tell the server {id} rang here: {e}");
                state(&app).inner.lock().unwrap().unreported.push((id, at));
            }
        }
    });
}

/// The server's answer: `{"type": "stop" | "snooze"}` or `{"type": "add", "ms": …}`.
fn answer_for(kind: Kind, b: Button) -> Option<Value> {
    match (kind, b) {
        (Kind::Timer, Button::A | Button::B) => Some(json!({ "type": "stop" })),
        (Kind::Timer, Button::X) => Some(json!({ "type": "add", "ms": MIN })),
        (Kind::Timer, Button::Y) => Some(json!({ "type": "add", "ms": 5 * MIN })),
        (Kind::Alarm, Button::A) => Some(json!({ "type": "snooze" })),
        (Kind::Alarm, Button::B) => Some(json!({ "type": "stop" })),
        (Kind::Alarm, _) => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    A,
    B,
    X,
    Y,
}

/// The pad while a web app is in front: it answers the ring shown over the page (A stops a timer
/// or snoozes an alarm, B stops, X and Y add 1 and 5 minutes to a timer). The bell stops here at
/// once; the answer goes to the server. False when nothing rings.
pub fn press(app: &AppHandle, b: Button) -> bool {
    let front = mine(app).into_iter().next();
    let Some(r) = front else { return false };
    if let Some(answer) = answer_for(r.kind, b) {
        stop_here(app, &r.id);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = crate::agent::server_call(&app, "clock_respond", json!({ "id": r.id, "answer": answer })).await {
                tracing::warn!("answering {}: {e}", r.label);
            }
        });
    }
    true
}

/// Answered on this TV (its pad, its ring screen): stops ringing here now, ahead of the server's
/// word. A fail-safe ring is reported as rung.
pub fn stop_here(app: &AppHandle, id: &str) {
    {
        let mut inner = state(app).inner.lock().unwrap();
        inner.rings.retain(|r| r.id != id);
        inner.local.retain(|r| r.id != id);
    }
    sync(app);
    emit(app);
}

/// The timers and alarms the server last sent, as far as the fail-safe needs them.
#[derive(Deserialize, Default)]
struct Timetable {
    #[serde(default)]
    timers: Vec<TimerDue>,
    #[serde(default)]
    alarms: Vec<AlarmDue>,
}

#[derive(Deserialize)]
struct TimerDue {
    id: String,
    label: String,
    #[serde(default)]
    emoji: String,
    #[serde(default)]
    color: String,
    #[serde(default)]
    total_ms: i64,
    end_at: Option<i64>,
    #[serde(default)]
    target: Option<RingTarget>,
}

#[derive(Deserialize)]
struct AlarmDue {
    id: String,
    label: String,
    enabled: bool,
    #[serde(default)]
    sound: Sound,
    next_at: Option<i64>,
    snoozed: Option<i64>,
    #[serde(default)]
    target: Option<RingTarget>,
}

/// Whether something aimed at `target` rings on this TV (`me`: its id and room).
fn aimed_here(target: &Option<RingTarget>, me: &(String, Option<String>)) -> bool {
    match target {
        None | Some(RingTarget::Everywhere) => true,
        Some(RingTarget::Device(id)) => *id == me.0,
        Some(RingTarget::Room(room)) => me.1.as_deref() == Some(room),
    }
}

/// What the fail-safe rings now: due within the last ten minutes, aimed here, not rung yet.
fn due(clock: &Value, me: &(String, Option<String>), now: i64, fired: &HashSet<(String, i64)>) -> Vec<Ring> {
    let table: Timetable = serde_json::from_value(clock.clone()).unwrap_or_default();
    let fresh = |id: &str, at: i64| at <= now && now - at <= MISSED && !fired.contains(&(id.to_string(), at));
    let mut out = Vec::new();
    for t in table.timers.iter().filter(|t| aimed_here(&t.target, me)) {
        if let Some(at) = t.end_at.filter(|at| fresh(&t.id, *at)) {
            out.push(Ring {
                id: t.id.clone(),
                kind: Kind::Timer,
                label: t.label.clone(),
                emoji: t.emoji.clone(),
                color: t.color.clone(),
                since: at,
                total_ms: t.total_ms,
                chips: Vec::new(),
                note: String::new(),
                sound: Sound::Chime,
                targets: vec![me.0.clone()],
            });
        }
    }
    for a in table.alarms.iter().filter(|a| aimed_here(&a.target, me)) {
        let at = [a.snoozed, a.next_at.filter(|_| a.enabled)].into_iter().flatten().find(|at| fresh(&a.id, *at));
        if let Some(at) = at {
            out.push(Ring {
                id: a.id.clone(),
                kind: Kind::Alarm,
                label: a.label.clone(),
                emoji: String::new(),
                color: ALARM_COLOR.into(),
                since: at,
                total_ms: 0,
                chips: Vec::new(),
                note: String::new(),
                sound: a.sound.clone(),
                targets: vec![me.0.clone()],
            });
        }
    }
    out
}

/// Every half second: the fail-safe while the server is away, rings left unanswered, and the
/// overlay over a web app.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            if tick(&app) {
                sync(&app);
                emit(&app);
            } else {
                overlay(&app, false);
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });
}

fn tick(app: &AppHandle) -> bool {
    let now = now_ms();
    let away = !crate::agent::online(app);
    let me = crate::agent::me(app);
    let mut inner = state(app).inner.lock().unwrap();
    let before = inner.rings.len() + inner.local.len();
    if let (true, Some(me), Some(clock)) = (away, me, inner.clock.clone()) {
        for r in due(&clock, &me, now, &inner.fired) {
            tracing::info!("{} went off while the server is away: ringing it here", r.label);
            inner.fired.insert((r.id.clone(), r.since));
            inner.unreported.push((r.id.clone(), r.since));
            inner.local.push(r);
        }
    }
    inner.fired.retain(|(_, at)| now - at <= 2 * MISSED);
    // Unanswered for ten minutes (the server stops its own, but it may be away).
    inner.rings.retain(|r| now - r.since <= RING_FOR);
    inner.local.retain(|r| now - r.since <= RING_FOR);
    inner.rings.len() + inner.local.len() != before
}

/// Starts or stops what ringing does, to match what rings now.
fn sync(app: &AppHandle) {
    let st = state(app);
    let front = mine(app).into_iter().next();
    let (starting, ending, generation) = {
        let mut inner = st.inner.lock().unwrap();
        let starting = front.is_some() && !inner.fx.on;
        let ending = front.is_none() && inner.fx.on;
        if starting || ending {
            inner.fx.generation += 1;
        }
        inner.fx.on = front.is_some();
        (starting, ending, inner.fx.generation)
    };
    if starting {
        begin(app, front.as_ref().is_some_and(|r| matches!(r.sound, Sound::Spotify { .. })));
    }
    if ending {
        finish(app);
        return;
    }
    if let Some(r) = front {
        bell(app, &r, generation);
        overlay(app, true);
    }
}

/// Quiets what's playing and makes sure the ring is heard.
fn begin(app: &AppHandle, playlist: bool) {
    let st = state(app);
    let music = !playlist && spotify::here().is_some_and(|h| h.playing);
    if music {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = spotify::control(&app, spotify::Control::Pause).await {
                tracing::warn!("ringing: pausing Spotify: {e}");
            }
        });
    }
    let before = pc::volume(|_| {}).ok();
    let speakers = before.filter(|v| v.muted || v.level < 20);
    if speakers.is_some() {
        let _ = pc::volume(|v| {
            v.muted = false;
            v.level = v.level.max(30);
        });
    }
    // Over the launcher's own player the ring screen is the launcher's: only the video stops.
    let video = crate::mpv::pause_for_ring(app);
    let mut inner = st.inner.lock().unwrap();
    inner.fx.music = music;
    inner.fx.speakers = speakers;
    inner.fx.player = video;
    if video {
        inner.fx.video = Some("The video".into());
    }
}

/// Puts back what ringing changed.
fn finish(app: &AppHandle) {
    let st = state(app);
    st.bell.stop();
    let fx = {
        let mut inner = st.inner.lock().unwrap();
        let generation = inner.fx.generation;
        std::mem::replace(&mut inner.fx, Effects { generation, ..Default::default() })
    };
    // The page takes its overlay away and plays the video it paused.
    if fx.player {
        crate::mpv::resume_after_ring(app);
    } else if fx.video.is_some() {
        cdp::send(app, Cmd::Eval(format!("{RING_SCRIPT}(null)")));
    }
    if let Some(v) = fx.speakers {
        let _ = pc::volume(|now| *now = v);
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(before) = fx.playlist {
            let _ = spotify::control(&app, spotify::Control::Pause).await;
            if let Some(v) = before {
                let _ = spotify::control(&app, spotify::Control::Volume(v)).await;
            }
        }
        if fx.music {
            if let Err(e) = spotify::control(&app, spotify::Control::Play).await {
                tracing::warn!("ringing: resuming Spotify: {e}");
            }
        }
    });
}

/// The bell for a timer or a chime alarm; a playlist alarm starts its playlist.
fn bell(app: &AppHandle, r: &Ring, generation: u64) {
    let st = state(app);
    let uri = match (&r.kind, &r.sound) {
        (Kind::Timer, _) => return st.bell.ring(Tone::Timer),
        (Kind::Alarm, Sound::Chime) => return st.bell.ring(Tone::Alarm),
        (Kind::Alarm, Sound::Spotify { uri, .. }) => uri.clone(),
    };
    {
        let mut inner = st.inner.lock().unwrap();
        if inner.fx.playlist.is_some() {
            return;
        }
        inner.fx.playlist = Some(None);
    }
    st.bell.stop();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = playlist(&app, &uri, generation).await {
            tracing::warn!("ringing: the alarm's playlist: {e}");
            if state(&app).inner.lock().unwrap().fx.generation == generation {
                state(&app).bell.ring(Tone::Alarm);
            }
        }
    });
}

/// Plays the alarm's playlist on this PC, from quiet to its usual volume over a minute.
async fn playlist(app: &AppHandle, uri: &str, generation: u64) -> crate::error::Result<()> {
    let current = |app: &AppHandle| state(app).inner.lock().unwrap().fx.generation == generation;
    let before = spotify::here().and_then(|h| h.volume);
    {
        let mut inner = state(app).inner.lock().unwrap();
        if inner.fx.generation == generation {
            inner.fx.playlist = Some(before);
        }
    }
    // Quiet before it starts, when there's a player to tell; again once it plays.
    let _ = spotify::control(app, spotify::Control::Volume(PLAYLIST_FROM)).await;
    spotify::play(app, uri).await?;
    let _ = spotify::control(app, spotify::Control::Volume(PLAYLIST_FROM)).await;
    let target = u32::from(before.unwrap_or(70).clamp(30, 100));
    let from = u32::from(PLAYLIST_FROM);
    for step in 1..=12 {
        tokio::time::sleep(Duration::from_secs(5)).await;
        if !current(app) {
            break;
        }
        let v = from + (target - from) * step / 12;
        let _ = spotify::control(app, spotify::Control::Volume(v as u8)).await;
    }
    Ok(())
}

/// Shows the ring over the web app in front, now (`force`) or when it's been a while: the page
/// pauses its video and keeps the overlay until told otherwise.
fn overlay(app: &AppHandle, force: bool) {
    if app.state::<AppState>().mode() != Mode::Streaming {
        return;
    }
    let st = state(app);
    let now = now_ms();
    {
        let mut inner = st.inner.lock().unwrap();
        if !force && now - inner.fx.overlay_at < OVERLAY_EVERY {
            return;
        }
        inner.fx.overlay_at = now;
    }
    let Some(mut r) = mine(app).into_iter().next() else { return };
    let name = playing_app(app);
    r.note = {
        let inner = st.inner.lock().unwrap();
        let fx = Effects { video: name.clone(), music: inner.fx.music, ..Default::default() };
        note(&Ring { note: String::new(), ..r.clone() }, &fx, name.as_deref())
    };
    if cdp::send(app, Cmd::Eval(format!("{RING_SCRIPT}({})", json!(r)))) {
        let mut inner = st.inner.lock().unwrap();
        if inner.fx.on && inner.fx.video.is_none() {
            inner.fx.video = name.or_else(|| Some("The video".into()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> (String, Option<String>) {
        ("pc-quarto".into(), Some("Quarto".into()))
    }

    #[test]
    fn the_fail_safe_rings_what_is_due_here_once() {
        let now = 1_000 * MIN;
        let clock = json!({
            "timers": [
                { "id": "t1", "label": "Pasta", "emoji": "🍝", "color": "#f00", "total_ms": 600000, "end_at": now - 1000,
                  "target": { "type": "room", "id": "Quarto" } },
                { "id": "t2", "label": "Sala's", "end_at": now - 1000, "target": { "type": "room", "id": "Sala" } },
                { "id": "t3", "label": "Later", "end_at": now + MIN },
                { "id": "t4", "label": "Long gone", "end_at": now - 60 * MIN }
            ],
            "alarms": [
                { "id": "a1", "label": "Wake up", "enabled": true, "next_at": now - 500, "target": { "type": "device", "id": "pc-quarto" } },
                { "id": "a2", "label": "Off", "enabled": false, "next_at": now - 500 },
                { "id": "a3", "label": "Snoozed", "enabled": false, "snoozed": now - 200 }
            ]
        });
        let mut fired = HashSet::new();
        let rang: Vec<String> = due(&clock, &me(), now, &fired).into_iter().map(|r| r.id).collect();
        assert_eq!(rang, ["t1", "a1", "a3"]);
        fired.insert(("t1".to_string(), now - 1000));
        assert!(!due(&clock, &me(), now, &fired).iter().any(|r| r.id == "t1"), "once");
    }

    #[test]
    fn answers_follow_the_pad() {
        assert_eq!(answer_for(Kind::Timer, Button::X), Some(json!({ "type": "add", "ms": 60_000 })));
        assert_eq!(answer_for(Kind::Alarm, Button::A), Some(json!({ "type": "snooze" })));
        assert_eq!(answer_for(Kind::Alarm, Button::Y), None);
    }
}
