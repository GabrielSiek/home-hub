//! Timers and alarms, on the server: from the old launcher's `clock/`, which also rang them on its
//! own speakers. Now the server keeps them and decides where each one rings (its room's TV PCs,
//! else every TV PC that's on), and each TV PC does the ringing: the chime, the speakers turned up,
//! Spotify paused or the alarm's playlist, the ring shown over the app in front. A TV PC gets its
//! whole ringing state (`ring.set`) on every change and whenever it reconnects.
//!
//! The assistant's quick path lives here too: clear timer and alarm requests in Portuguese or
//! English are done without asking the model (`quick`).

pub mod parse;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tvl_kit::store::JsonStore;
use tvl_kit::text::score;
use tvl_kit::time::{local, now_ms, Day, DAY, HOUR, MIN};
use tvl_kit::{Error, Result};

use crate::assistant::Card;
use crate::ctx::Ctx;
use crate::smarthome::{self, scenes};
use parse::{Intent, Lang, Target, When};
pub use tvl_protocol::clock::{Chip, Ring, RingKind as Kind, RingSet, RingTarget, Sound};

const FILE: &str = "clock.json";
/// Unanswered, a ring stops by itself after this long.
const RING_FOR: i64 = 10 * MIN;
/// Something due while the server was off still rings if it's this recent.
const MISSED: i64 = 10 * MIN;
pub const SNOOZE: i64 = 10 * MIN;
const LONGEST: i64 = 24 * HOUR;
const ALARM_COLOR: &str = "#ffd36b";
const TICK: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timer {
    pub id: String,
    pub label: String,
    /// Named by the user, rather than after its length ("12-minute timer").
    #[serde(default)]
    pub named: bool,
    pub emoji: String,
    pub color: String,
    /// Its length, with the time added since.
    pub total_ms: i64,
    /// Running: when it goes off.
    pub end_at: Option<i64>,
    /// Paused: what was left.
    pub left_ms: Option<i64>,
    /// Where it rings; None: every TV PC that's on (timers from the old launcher).
    #[serde(default)]
    pub target: Option<RingTarget>,
}

impl Timer {
    fn left(&self, now: i64) -> i64 {
        self.left_ms.unwrap_or_else(|| self.end_at.map_or(0, |e| (e - now).max(0)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alarm {
    pub id: String,
    pub hour: u8,
    pub minute: u8,
    pub label: String,
    /// Monday 0 … Sunday 6; none: once.
    #[serde(default)]
    pub days: Vec<u8>,
    /// Once, on this day ("2026-10-27"); without it, on the first day the time comes.
    #[serde(default)]
    pub date: Option<String>,
    pub enabled: bool,
    #[serde(default)]
    pub sound: Sound,
    /// A smart home scene run as it goes off.
    #[serde(default)]
    pub scene: Option<String>,
    /// When it goes off next. Kept, so one due while the server was off still rings.
    #[serde(default)]
    pub next_at: Option<i64>,
    /// Snoozed: rings again then.
    #[serde(default)]
    pub snoozed: Option<i64>,
    /// Where it rings; None: every TV PC that's on.
    #[serde(default)]
    pub target: Option<RingTarget>,
}

impl Alarm {
    fn next(&self, after: i64) -> Option<i64> {
        if !self.enabled {
            return None;
        }
        let date = self.date.as_deref().and_then(Day::parse_iso);
        local::next_at(self.hour, self.minute, &self.days, date, after)
    }

    fn time(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }
}

/// Everything the screens show.
#[derive(Debug, Clone, Serialize)]
pub struct Clock {
    pub timers: Vec<Timer>,
    pub alarms: Vec<Alarm>,
    /// The first one is on screen; the rest follow as each is answered.
    pub ringing: Vec<Ring>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    timers: Vec<Timer>,
    #[serde(default)]
    alarms: Vec<Alarm>,
}

/// A ring in progress, with what only the server needs.
#[derive(Debug, Clone)]
struct Ringing {
    ring: Ring,
    /// When it started ringing: later than `ring.since` for one missed while the server was off.
    started: i64,
    target: Option<RingTarget>,
}

#[derive(Default)]
struct Inner {
    timers: Vec<Timer>,
    alarms: Vec<Alarm>,
    ringing: Vec<Ringing>,
    /// The minute the alarms' next times were last worked out in.
    planned: i64,
}

pub struct State {
    saved: JsonStore<Saved>,
    inner: Mutex<Inner>,
    /// What each TV PC was last told is ringing on it, to tell it again only on a change.
    sent: Mutex<HashMap<String, String>>,
    generation: AtomicU64,
}

impl State {
    pub fn new(data_dir: &std::path::Path) -> Self {
        let saved: JsonStore<Saved> = JsonStore::open(data_dir.join(FILE));
        let s = saved.get();
        Self {
            inner: Mutex::new(Inner { timers: s.timers, alarms: s.alarms, ..Default::default() }),
            saved,
            sent: Mutex::default(),
            generation: AtomicU64::new(0),
        }
    }

    fn save(&self, inner: &Inner) -> Result<()> {
        self.saved.set(Saved { timers: inner.timers.clone(), alarms: inner.alarms.clone() })
    }
}

fn state(app: &Ctx) -> &State {
    &app.clock
}

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn new_id(prefix: &str) -> String {
    format!("{prefix}{}{}", now_ms(), NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

// ---------- Where things ring ----------

/// Where something set from `screen` rings: that screen's room; for a phone in no room, its TV
/// PC's room (or that TV PC); None (everywhere) when there's nothing to go by.
pub fn target_for(app: &Ctx, screen: Option<&str>) -> Option<RingTarget> {
    let screen = screen?;
    if let Some(room) = app.devices.get(screen).and_then(|s| s.room) {
        return Some(RingTarget::Room(room));
    }
    let tv = app.devices.get(&app.devices.followed(Some(screen))?)?;
    Some(match tv.room {
        Some(room) => RingTarget::Room(room),
        None => RingTarget::Device(tv.id),
    })
}

/// The TV PCs that ring for `target` now, and a note when it isn't where it was meant to.
fn ringers(app: &Ctx, target: &Option<RingTarget>) -> (Vec<String>, String) {
    let on: Vec<tvl_protocol::Screen> =
        app.devices.tvs().into_iter().filter(|t| t.online && t.caps.as_ref().is_none_or(|c| c.ring)).collect();
    let everywhere = |why: String| {
        let names: Vec<String> = on.iter().map(|t| t.name.clone()).collect();
        let note = if names.is_empty() { String::new() } else { format!("{why}: ringing on {}", names.join(", ")) };
        (on.iter().map(|t| t.id.clone()).collect::<Vec<_>>(), note)
    };
    match target {
        Some(RingTarget::Room(room)) => {
            let here: Vec<String> = on.iter().filter(|t| t.room.as_deref() == Some(room)).map(|t| t.id.clone()).collect();
            if here.is_empty() {
                everywhere(format!("No TV is on in {room}"))
            } else {
                (here, String::new())
            }
        }
        Some(RingTarget::Device(id)) => {
            if on.iter().any(|t| &t.id == id) {
                (vec![id.clone()], String::new())
            } else {
                let name = app.devices.get(id).map_or_else(|| "That TV".to_string(), |t| t.name);
                everywhere(format!("{name} is off"))
            }
        }
        Some(RingTarget::Everywhere) | None => (on.iter().map(|t| t.id.clone()).collect(), String::new()),
    }
}

fn with_targets(app: &Ctx, r: &Ringing) -> Ring {
    let (targets, note) = ringers(app, &r.target);
    Ring { targets, note, ..r.ring.clone() }
}

pub fn snapshot(app: &Ctx) -> Clock {
    let (timers, alarms, ringing) = {
        let inner = state(app).inner.lock();
        (inner.timers.clone(), inner.alarms.clone(), inner.ringing.clone())
    };
    Clock { timers, alarms, ringing: ringing.iter().map(|r| with_targets(app, r)).collect() }
}

/// Tells each TV PC what rings on it, when that changed (or always, with `force`). Nothing rings
/// in shadow mode: the old launcher rings its own copies.
fn push_rings(app: &Ctx, force: Option<&str>) {
    if app.config.shadow {
        return;
    }
    let rings = snapshot(app).ringing;
    for tv in app.devices.tvs().into_iter().filter(|t| t.online) {
        let mine: Vec<Ring> = rings.iter().filter(|r| r.targets.contains(&tv.id)).cloned().collect();
        let signature = serde_json::to_string(&mine).unwrap_or_default();
        let changed = state(app).sent.lock().insert(tv.id.clone(), signature.clone()).as_ref() != Some(&signature);
        if !changed && force != Some(tv.id.as_str()) {
            continue;
        }
        let set = RingSet { generation: state(app).generation.fetch_add(1, Ordering::Relaxed) + 1, rings: mine };
        let (ctx, device) = (app.clone(), tv.id.clone());
        tokio::spawn(async move {
            let args = serde_json::to_value(&set).unwrap_or_default();
            if let Err(e) = ctx.agents.call(&device, "ring.set", args, None, 5_000, false).await {
                tracing::debug!(device, "ring.set not delivered: {e:?}");
            }
        });
    }
}

/// A TV PC (re)connected: it gets its whole ringing state, even nothing.
pub fn resend(app: &Ctx, device: &str) {
    state(app).sent.lock().remove(device);
    push_rings(app, Some(device));
}

/// Changes the timers and alarms, saves them, catches up with what rings and tells the screens.
fn edit<T>(app: &Ctx, f: impl FnOnce(&mut Inner, i64) -> Result<T>) -> Result<T> {
    let st = state(app);
    let out = {
        let mut inner = st.inner.lock();
        let out = f(&mut inner, now_ms())?;
        inner.planned = 0;
        st.save(&inner)?;
        out
    };
    tick(app);
    push_rings(app, None);
    emit(app);
    Ok(out)
}

fn emit(app: &Ctx) {
    app.bus.emit("clock", &snapshot(app));
}

// ---------- Timers ----------

pub fn start_timer(app: &Ctx, ms: i64, label: Option<&str>, target: Option<RingTarget>) -> Result<Timer> {
    if !(1000..=LONGEST).contains(&ms) {
        return Err(Error::msg("A timer can run from 1 second to 24 hours"));
    }
    let label = label.map(str::trim).filter(|l| !l.is_empty()).map(capitalize);
    let named = label.is_some();
    let label = label.unwrap_or_else(|| length_name(ms));
    let (emoji, color) = look(&label);
    edit(app, |inner, now| {
        let t = Timer {
            id: new_id("t"),
            label,
            named,
            emoji: emoji.into(),
            color: color.into(),
            total_ms: ms,
            end_at: Some(now + ms),
            left_ms: None,
            target,
        };
        inner.timers.push(t.clone());
        Ok(t)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", content = "ms", rename_all = "snake_case")]
pub enum Change {
    Pause,
    Resume,
    /// Milliseconds more (or fewer); a timer that went off starts again with them.
    Add(i64),
    Cancel,
}

pub fn change_timer(app: &Ctx, id: &str, c: Change) -> Result<Timer> {
    edit(app, |inner, now| {
        let i = inner.timers.iter().position(|t| t.id == id).ok_or_else(|| Error::msg("That timer is gone"))?;
        let ringing = inner.ringing.iter().any(|r| r.ring.id == id);
        let t = &mut inner.timers[i];
        match c {
            Change::Cancel => {
                let t = inner.timers.remove(i);
                inner.ringing.retain(|r| r.ring.id != id);
                return Ok(t);
            }
            Change::Add(ms) if ringing => {
                let ms = ms.clamp(1000, LONGEST);
                (t.end_at, t.left_ms, t.total_ms) = (Some(now + ms), None, t.total_ms + ms);
                inner.ringing.retain(|r| r.ring.id != id);
            }
            Change::Add(ms) => {
                let left = (t.left(now) + ms).clamp(1000, LONGEST);
                t.total_ms = (t.total_ms + left - t.left(now)).max(left);
                match t.left_ms {
                    Some(_) => t.left_ms = Some(left),
                    None => t.end_at = Some(now + left),
                }
            }
            Change::Pause if !ringing => {
                if let Some(end) = t.end_at.take() {
                    t.left_ms = Some((end - now).max(1000));
                }
            }
            Change::Resume => {
                if let Some(left) = t.left_ms.take() {
                    t.end_at = Some(now + left);
                }
            }
            Change::Pause => {}
        }
        Ok(inner.timers[i].clone())
    })
}

/// "12-minute timer", "1h 30m timer".
fn length_name(ms: i64) -> String {
    let s = (ms + 500) / 1000;
    match (s / 3600, s / 60 % 60, s % 60) {
        (0, 0, s) => format!("{s}-second timer"),
        (0, m, 0) => format!("{m}-minute timer"),
        (h, 0, 0) => format!("{h}-hour timer"),
        (0, m, s) => format!("{m}:{s:02} timer"),
        (h, m, 0) => format!("{h}h {m}m timer"),
        (h, m, s) => format!("{h}:{m:02}:{s:02} timer"),
    }
}

/// An emoji and a colour for a timer, from its name.
fn look(label: &str) -> (&'static str, &'static str) {
    const FOOD: &str = "#E8590C";
    const LOOKS: &[(&[&str], &str, &str)] = &[
        (&["pizza"], "🍕", FOOD),
        (&["cha", "tea"], "🍵", "#20c997"),
        (&["cafe", "coffee"], "☕", "#a47148"),
        (&["bolo", "cake", "torta", "pie", "brownie"], "🍰", "#e64980"),
        (&["pao", "bread", "baguete"], "🍞", FOOD),
        (&["ovo", "ovos", "egg", "eggs"], "🥚", "#f2c94c"),
        (&["arroz", "rice"], "🍚", FOOD),
        (&["macarrao", "massa", "pasta", "miojo", "lasanha", "lasagna", "spaghetti", "espaguete"], "🍝", FOOD),
        (&["pipoca", "popcorn"], "🍿", FOOD),
        (&["carne", "meat", "steak", "churrasco", "picanha", "bbq"], "🥩", "#c92a2a"),
        (&["frango", "chicken"], "🍗", FOOD),
        (&["batata", "batatas", "potato", "potatoes", "fries"], "🍟", FOOD),
        (&["sopa", "soup", "feijao", "beans"], "🍲", FOOD),
        (&["forno", "oven", "assado", "roast"], "🔥", FOOD),
        (&["roupa", "roupas", "laundry", "maquina", "washing"], "👕", "#339af0"),
        (&["banho", "shower", "bath"], "🚿", "#339af0"),
        (&["treino", "workout", "exercicio", "exercise", "alongamento"], "💪", "#7950f2"),
        (&["jogo", "game", "partida"], "🎮", "#7950f2"),
        (&["soneca", "cochilo", "nap", "sleep", "dormir"], "😴", "#7950f2"),
        (&["remedio", "medicine", "pill", "pills"], "💊", "#e64980"),
        (&["planta", "plantas", "plants", "regar"], "🪴", "#40c057"),
        (&["cachorro", "dog", "passear", "walk"], "🐕", "#40c057"),
    ];
    let words = parse::plain(label);
    LOOKS
        .iter()
        .find(|(keys, ..)| words.iter().any(|w| keys.contains(&w.as_str())))
        .map_or(("⏱️", "#4c8dff"), |(_, e, c)| (*e, *c))
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

// ---------- Alarms ----------

/// An alarm as the Clock screen or the assistant sets it: new without an id.
#[derive(Debug, Clone, Deserialize)]
pub struct AlarmEdit {
    pub id: Option<String>,
    pub hour: u8,
    pub minute: u8,
    pub label: String,
    #[serde(default)]
    pub days: Vec<u8>,
    /// Once, on this day.
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub sound: Sound,
    #[serde(default)]
    pub scene: Option<String>,
    /// Where it rings; left out, a new alarm rings where it was set from and an edited one keeps
    /// its place.
    #[serde(default)]
    pub target: Option<RingTarget>,
}

/// Saves alarm `e`; a new one without a target rings at `default_target`.
pub fn save_alarm(app: &Ctx, e: AlarmEdit, default_target: Option<RingTarget>) -> Result<Alarm> {
    if e.hour > 23 || e.minute > 59 || e.days.iter().any(|d| *d > 6) {
        return Err(Error::msg("That's not a time on the clock"));
    }
    let mut days = e.days.clone();
    days.sort_unstable();
    days.dedup();
    let day = e.date.as_deref().and_then(Day::parse_iso).filter(|_| days.is_empty());
    let date = day.map(Day::iso);
    let label = Some(e.label.trim()).filter(|l| !l.is_empty()).map_or_else(|| "Alarm".to_string(), capitalize);
    edit(app, |inner, now| {
        if local::next_at(e.hour, e.minute, &days, day, now).is_none() {
            return Err(Error::msg("That day has gone by"));
        }
        let i = match e.id.as_ref().and_then(|id| inner.alarms.iter().position(|a| &a.id == id)) {
            Some(i) => i,
            None => {
                inner.alarms.push(Alarm {
                    id: new_id("a"),
                    hour: 0,
                    minute: 0,
                    label: String::new(),
                    days: Vec::new(),
                    date: None,
                    enabled: true,
                    sound: Sound::Chime,
                    scene: None,
                    next_at: None,
                    snoozed: None,
                    target: default_target,
                });
                inner.alarms.len() - 1
            }
        };
        let a = &mut inner.alarms[i];
        (a.hour, a.minute, a.label, a.days, a.date) = (e.hour, e.minute, label, days, date);
        (a.sound, a.scene, a.enabled, a.snoozed) = (e.sound.clone(), e.scene.clone(), true, None);
        if e.target.is_some() {
            a.target = e.target.clone();
        }
        a.next_at = a.next(now);
        let id = a.id.clone();
        inner.ringing.retain(|r| r.ring.id != id);
        Ok(inner.alarms[i].clone())
    })
}

pub fn set_alarm_on(app: &Ctx, id: &str, on: bool) -> Result<Alarm> {
    edit(app, |inner, now| {
        let a = inner.alarms.iter_mut().find(|a| a.id == id).ok_or_else(|| Error::msg("That alarm is gone"))?;
        a.enabled = on;
        a.snoozed = None;
        // A one-off alarm turned back on rings at its time's next coming.
        if on && a.next(now).is_none() {
            a.date = None;
        }
        a.next_at = a.next(now);
        let a = a.clone();
        inner.ringing.retain(|r| r.ring.id != a.id);
        Ok(a)
    })
}

pub fn delete_alarm(app: &Ctx, id: &str) -> Result<()> {
    edit(app, |inner, _| {
        inner.alarms.retain(|a| a.id != id);
        inner.ringing.retain(|r| r.ring.id != id);
        Ok(())
    })
}

/// A room renamed (Some) or deleted (None): what rings there follows.
pub fn move_room(app: &Ctx, from: &str, to: Option<&str>) -> Result<()> {
    let moved = |t: &mut Option<RingTarget>| {
        if *t == Some(RingTarget::Room(from.to_string())) {
            *t = to.map(|r| RingTarget::Room(r.to_string()));
        }
    };
    edit(app, |inner, _| {
        inner.timers.iter_mut().for_each(|t| moved(&mut t.target));
        inner.alarms.iter_mut().for_each(|a| moved(&mut a.target));
        inner.ringing.iter_mut().for_each(|r| moved(&mut r.target));
        Ok(())
    })
}

// ---------- Ringing ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", content = "ms", rename_all = "snake_case")]
pub enum Answer {
    Stop,
    Snooze,
    /// A timer: starts it again with this long.
    Add(i64),
}

/// Anyone may answer, from any screen; a second answer to the same ring does nothing.
pub fn respond(app: &Ctx, id: &str, a: Answer) -> Result<()> {
    edit(app, |inner, now| {
        let Some(r) = inner.ringing.iter().find(|r| r.ring.id == id).cloned() else {
            return Ok(());
        };
        inner.ringing.retain(|x| x.ring.id != id);
        match (r.ring.kind, a) {
            (Kind::Timer, Answer::Add(ms)) => {
                if let Some(t) = inner.timers.iter_mut().find(|t| t.id == id) {
                    let ms = ms.clamp(1000, LONGEST);
                    (t.end_at, t.left_ms, t.total_ms) = (Some(now + ms), None, t.total_ms + ms);
                }
            }
            (Kind::Timer, _) => inner.timers.retain(|t| t.id != id),
            (Kind::Alarm, Answer::Snooze) => {
                if let Some(a) = inner.alarms.iter_mut().find(|a| a.id == id) {
                    a.snoozed = Some(now + SNOOZE);
                }
            }
            // A one-off alarm was turned off as it went off.
            (Kind::Alarm, _) => {}
        }
        Ok(())
    })
}

/// A TV PC rang something on its own while the server was out of reach (its fail-safe): it's
/// done, so the server's catch-up doesn't ring it again.
pub fn rang_locally(app: &Ctx, id: &str, at: i64) -> Result<()> {
    edit(app, |inner, now| {
        inner.ringing.retain(|r| r.ring.id != id);
        if let Some(a) = inner.alarms.iter_mut().find(|a| a.id == id) {
            if a.next_at.is_some_and(|n| n <= at + MISSED) {
                if a.days.is_empty() {
                    (a.enabled, a.date) = (false, None);
                }
                a.next_at = a.next(now.max(at + 1));
            }
        }
        inner.timers.retain(|t| !(t.id == id && t.end_at.is_some_and(|e| e <= at + MISSED)));
        Ok(())
    })
}

fn timer_ring(t: &Timer, since: i64, now: i64) -> Ringing {
    Ringing {
        ring: Ring {
            id: t.id.clone(),
            kind: Kind::Timer,
            label: t.label.clone(),
            emoji: t.emoji.clone(),
            color: t.color.clone(),
            since,
            total_ms: t.total_ms,
            chips: Vec::new(),
            note: String::new(),
            sound: Sound::Chime,
            targets: Vec::new(),
        },
        started: now,
        target: t.target.clone(),
    }
}

fn alarm_ring(a: &Alarm, since: i64, now: i64) -> Ringing {
    Ringing {
        ring: Ring {
            id: a.id.clone(),
            kind: Kind::Alarm,
            label: a.label.clone(),
            emoji: String::new(),
            color: ALARM_COLOR.into(),
            since,
            total_ms: 0,
            chips: Vec::new(),
            note: String::new(),
            sound: a.sound.clone(),
            targets: Vec::new(),
        },
        started: now,
        target: a.target.clone(),
    }
}

/// Rings what's due, plans the alarms' next times and ends rings left unanswered. True when
/// anything changed.
fn tick(app: &Ctx) -> bool {
    let st = state(app);
    let now = now_ms();
    let mut rang = Vec::new();
    let changed = {
        let mut guard = st.inner.lock();
        let inner = &mut *guard;
        let mut changed = false;

        let mut gone = Vec::new();
        for t in &inner.timers {
            let Some(end) = t.end_at.filter(|e| *e <= now) else { continue };
            if inner.ringing.iter().any(|r| r.ring.id == t.id) {
                continue;
            }
            if now - end <= MISSED {
                rang.push(timer_ring(t, end, now));
            } else {
                gone.push(t.id.clone());
            }
        }
        if !gone.is_empty() {
            inner.timers.retain(|t| !gone.contains(&t.id));
            changed = true;
        }

        let replan = now / MIN != inner.planned;
        inner.planned = now / MIN;
        for a in &mut inner.alarms {
            if let Some(s) = a.snoozed.filter(|s| *s <= now) {
                a.snoozed = None;
                changed = true;
                if now - s <= MISSED {
                    rang.push(alarm_ring(a, s, now));
                }
            }
            match a.next_at {
                Some(at) if at <= now => {
                    changed = true;
                    if now - at <= MISSED {
                        rang.push(alarm_ring(a, at, now));
                    }
                    if a.days.is_empty() {
                        (a.enabled, a.date) = (false, None);
                    }
                    a.next_at = a.next(now);
                }
                // Once a minute: the zone or the clock may have changed.
                _ if replan => {
                    let next = a.next(now);
                    if next != a.next_at {
                        a.next_at = next;
                        changed = true;
                    }
                    // A one-off whose day went by while the server was off.
                    if a.enabled && next.is_none() {
                        (a.enabled, a.date) = (false, None);
                        changed = true;
                    }
                }
                _ => {}
            }
        }

        let unanswered: Vec<String> =
            inner.ringing.iter().filter(|r| now - r.started > RING_FOR).map(|r| r.ring.id.clone()).collect();
        for id in &unanswered {
            let timer = inner.ringing.iter().any(|r| &r.ring.id == id && r.ring.kind == Kind::Timer);
            inner.ringing.retain(|x| &x.ring.id != id);
            if timer {
                inner.timers.retain(|t| &t.id != id);
            }
            changed = true;
        }
        for r in &rang {
            if !inner.ringing.iter().any(|x| x.ring.id == r.ring.id) {
                inner.ringing.push(r.clone());
                changed = true;
            }
        }
        if changed {
            if let Err(e) = st.save(inner) {
                tracing::error!("can't save the timers and alarms: {e}");
            }
        }
        changed
    };
    for r in rang {
        went_off(app, r.ring);
    }
    changed
}

/// Rings what's due every half second while the server runs, and follows the TV PCs coming and
/// going (what rings where changes with them).
pub fn start(app: &Ctx) {
    let ctx = app.clone();
    tvl_kit::task::spawn_supervised("clock", app.shutdown.clone(), move || {
        let ctx = ctx.clone();
        async move {
            loop {
                if tick(&ctx) {
                    push_rings(&ctx, None);
                    emit(&ctx);
                }
                tokio::time::sleep(TICK).await;
            }
        }
    });
    let ctx = app.clone();
    tvl_kit::task::spawn_supervised("clock follows the TV PCs", app.shutdown.clone(), move || {
        let ctx = ctx.clone();
        async move {
            let mut rx = ctx.bus.subscribe();
            loop {
                match rx.recv().await {
                    Ok(e) if &*e.event == "devices" && !state(&ctx).inner.lock().ringing.is_empty() => {
                        push_rings(&ctx, None);
                        emit(&ctx);
                    }
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
        }
    });
}

/// An alarm's scene, and the chips on its screen.
fn went_off(app: &Ctx, r: Ring) {
    tracing::info!("{} went off", r.label);
    if r.kind != Kind::Alarm {
        return;
    }
    let scene = state(app).inner.lock().alarms.iter().find(|a| a.id == r.id).and_then(|a| a.scene.clone());
    let app = app.clone();
    tokio::spawn(async move {
        let mut chips = Vec::new();
        if let Some(id) = scene.filter(|_| !app.config.shadow) {
            let name = smarthome::snapshot(&app).scenes.into_iter().find(|s| s.id == id).map(|s| s.name);
            match scenes::run(&app, &id).await {
                Ok(()) => chips.push(Chip { color: ALARM_COLOR.into(), text: format!("{} scene on", name.unwrap_or(id)) }),
                Err(e) => tracing::warn!("the alarm's scene: {e}"),
            }
        }
        chips.extend(today(&app).await);
        if chips.is_empty() {
            return;
        }
        if let Some(x) = state(&app).inner.lock().ringing.iter_mut().find(|x| x.ring.id == r.id) {
            x.ring.chips = chips;
        }
        push_rings(&app, None);
        emit(&app);
    });
}

/// The day's first event and Grêmio's match, if there's one today, for an alarm's screen.
async fn today(app: &Ctx) -> Vec<Chip> {
    let now = now_ms();
    let midnight = local::utc(local::day(now).plus(1).days() * DAY);
    let events = crate::calendar::rest_of_day(app, now, midnight).await;
    let mut chips = Vec::new();
    if let Some(e) = events.iter().find(|e| e.game.is_none()) {
        chips.push(Chip { color: e.color.clone(), text: format!("First up · {} {}", local::hm(e.start), e.title) });
    }
    if let Some(e) = events.iter().find(|e| e.game.is_some()) {
        let evening = local::wall(e.start).rem_euclid(DAY) >= 18 * HOUR;
        let when = if evening { "Tonight" } else { "Today" };
        chips.push(Chip { color: e.color.clone(), text: format!("{when} · {}, {}", e.title, local::hm(e.start)) });
    }
    chips
}

// ---------- The assistant ----------

/// The timers and alarms in a few lines, for the model.
pub fn describe(app: &Ctx) -> String {
    let now = now_ms();
    let inner = state(app).inner.lock();
    let mut out = String::from("\nTimers (id: name, state):\n");
    if inner.timers.is_empty() {
        out.push_str("- none\n");
    }
    for t in &inner.timers {
        let what = if inner.ringing.iter().any(|r| r.ring.id == t.id) {
            "ringing now".to_string()
        } else if t.left_ms.is_some() {
            format!("paused, {} left", clock_text(t.left(now)))
        } else {
            format!("{} left, ends at {}", clock_text(t.left(now)), local::hm(t.end_at.unwrap_or(now)))
        };
        out.push_str(&format!("- {}: {} ({} long), {what}\n", t.id, t.label, clock_text(t.total_ms)));
    }
    out.push_str("Alarms (id: time, name, days, state):\n");
    if inner.alarms.is_empty() {
        out.push_str("- none\n");
    }
    for a in &inner.alarms {
        let state = match (a.enabled, a.next_at, a.snoozed) {
            (_, _, Some(s)) => format!("snoozed until {}", local::hm(s)),
            (true, Some(n), _) => format!("on, next {} {}", local::weekday_name(local::weekday(local::day(n)), false), local::hm(n)),
            _ => "off".into(),
        };
        out.push_str(&format!("- {}: {}, {}, {}, {state}\n", a.id, a.time(), a.label, days_phrase(&a.days, false)));
    }
    out
}

/// When an alarm rings next, for the model: "Monday 07:00".
pub fn next_text(a: &Alarm) -> Option<String> {
    let n = a.snoozed.or(a.next_at)?;
    Some(format!("{} {}", local::weekday_name(local::weekday(local::day(n)), false), local::hm(n)))
}

/// "8:42", "1:02:03".
fn clock_text(ms: i64) -> String {
    let s = (ms + 999) / 1000;
    match s / 3600 {
        0 => format!("{}:{:02}", s / 60, s % 60),
        h => format!("{h}:{:02}:{:02}", s / 60 % 60, s % 60),
    }
}

/// A card under the reply that shows the timer counting down, or the alarm.
pub fn timer_card(t: &Timer, now: i64) -> Card {
    let sub = match t.end_at {
        Some(end) => format!("Timer · ends at {}", local::hm(end)),
        None => "Timer · paused".into(),
    };
    Card::new("clock", &format!("{} · {}", t.label, clock_text(t.left(now))), &sub).accent().clock(&format!("timer:{}", t.id))
}

pub fn alarm_card(a: &Alarm, now: i64) -> Card {
    let when = match a.next_at {
        Some(n) if a.days.is_empty() => day_word(n, now, false),
        _ => days_phrase(&a.days, false),
    };
    Card::new("bell", &format!("{} · {}", a.time(), capitalize(&when)), "Alarm").accent().clock(&format!("alarm:{}", a.id))
}

/// The timers a name, an id or "all" picks.
pub fn find_timers(app: &Ctx, query: &str) -> Vec<Timer> {
    let inner = state(app).inner.lock();
    let q = query.trim();
    if q.is_empty() || matches!(q.to_lowercase().as_str(), "all" | "every" | "todos") {
        return inner.timers.clone();
    }
    if let Some(t) = inner.timers.iter().find(|t| t.id == q) {
        return vec![t.clone()];
    }
    best_timer(&inner.timers, q).into_iter().cloned().collect()
}

fn best_timer<'a>(timers: &'a [Timer], name: &str) -> Option<&'a Timer> {
    timers.iter().map(|t| (score(name, &t.label), t)).filter(|(s, _)| *s > 0).max_by_key(|(s, _)| *s).map(|(_, t)| t)
}

/// The alarms an id, a time ("07:00"), a name or "all" picks.
pub fn find_alarms(app: &Ctx, query: &str) -> Vec<Alarm> {
    let inner = state(app).inner.lock();
    let q = query.trim();
    if q.is_empty() || matches!(q.to_lowercase().as_str(), "all" | "every" | "todos") {
        return inner.alarms.clone();
    }
    if let Some(a) = inner.alarms.iter().find(|a| a.id == q) {
        return vec![a.clone()];
    }
    let at: Vec<Alarm> = inner.alarms.iter().filter(|a| a.time() == normalize_time(q)).cloned().collect();
    if !at.is_empty() {
        return at;
    }
    let found = inner.alarms.iter().map(|a| (score(q, &a.label), a)).filter(|(s, _)| *s > 0).max_by_key(|(s, _)| *s);
    found.map(|(_, a)| a.clone()).into_iter().collect()
}

/// "7:00" → "07:00".
fn normalize_time(q: &str) -> String {
    match q.split_once(':').map(|(h, m)| (h.trim().parse::<u8>(), m.trim().parse::<u8>())) {
        Some((Ok(h), Ok(m))) => format!("{h:02}:{m:02}"),
        _ => q.to_string(),
    }
}

/// The days an alarm rings on, in words: "on weekdays", "às segundas e quartas".
fn days_phrase(days: &[u8], pt: bool) -> String {
    const PLURAL_PT: [&str; 7] = ["segundas", "terças", "quartas", "quintas", "sextas", "sábados", "domingos"];
    match (days, pt) {
        ([], false) => "once".into(),
        ([], true) => "uma vez".into(),
        ([0, 1, 2, 3, 4, 5, 6], false) => "every day".into(),
        ([0, 1, 2, 3, 4, 5, 6], true) => "todos os dias".into(),
        ([0, 1, 2, 3, 4], false) => "on weekdays".into(),
        ([0, 1, 2, 3, 4], true) => "de segunda a sexta".into(),
        ([5, 6], false) => "on weekends".into(),
        ([5, 6], true) => "nos fins de semana".into(),
        (list, false) => format!("on {}", and_list(list.iter().map(|d| format!("{}s", local::weekday_name(*d, false))), false)),
        (list, true) => format!("às {}", and_list(list.iter().map(|d| PLURAL_PT[usize::from(*d).min(6)].to_string()), true)),
    }
}

fn and_list(items: impl Iterator<Item = String>, pt: bool) -> String {
    let items: Vec<String> = items.collect();
    match items.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} {} {last}", rest.join(", "), if pt { "e" } else { "and" }),
    }
}

/// "today", "tomorrow, Monday", "on Wednesday" for a one-off time.
fn day_word(at: i64, now: i64, pt: bool) -> String {
    let (d, today) = (local::day(at), local::day(now));
    let name = local::weekday_name(local::weekday(d), pt);
    match (d.days() - today.days(), pt) {
        (0, false) => "today".into(),
        (0, true) => "hoje".into(),
        (1, false) => format!("tomorrow, {name}"),
        (1, true) => format!("amanhã, {name}"),
        (_, false) => format!("on {name}"),
        (_, true) => format!("{} {name}", if local::weekday(d) >= 5 { "no" } else { "na" }),
    }
}

/// A length read out: "12 minutes", "1 hora e 30 minutos". Seconds only under ten minutes.
fn spoken(ms: i64, pt: bool) -> String {
    let mut s = (ms + 999) / 1000;
    if s >= 600 {
        s = (s + 30) / 60 * 60;
    }
    let (h, m, sec) = (s / 3600, s / 60 % 60, s % 60);
    let unit = |n: i64, en: &str, pt_word: &str| {
        let word = if pt { pt_word } else { en };
        format!("{n} {word}{}", if n == 1 { "" } else { "s" })
    };
    let parts: Vec<String> = [(h, "hour", "hora"), (m, "minute", "minuto"), (sec, "second", "segundo")]
        .into_iter()
        .filter(|(n, ..)| *n > 0)
        .map(|(n, en, p)| unit(n, en, p))
        .collect();
    if parts.is_empty() {
        return unit(0, "second", "segundo");
    }
    and_list(parts.into_iter(), pt)
}

/// A request for a timer or an alarm the assistant understood without the model, done.
pub struct Quick {
    pub text: String,
    pub cards: Vec<Card>,
}

/// One part of a request, with what it acts on found.
enum Step {
    Timer { ms: i64, label: Option<String> },
    Alarm { hour: u8, minute: u8, days: Vec<u8>, date: Option<Day>, label: Option<String> },
    Cancel(Vec<Timer>),
    AlarmsOff(Vec<Alarm>),
    Add(Timer, i64),
    Pause(Timer),
    Resume(Timer),
    Left(Timer),
    Say(&'static str, &'static str),
}

/// Does what `text` asks when it's clearly about timers or alarms; None leaves it to the model.
/// What it sets rings at `target` (where it was said).
pub fn quick(app: &Ctx, text: &str, target: Option<RingTarget>) -> Option<Quick> {
    let pt_first = app.settings.get().language.starts_with("pt");
    let parsed = parse::parse(text, if pt_first { Lang::Pt } else { Lang::En })?;
    let pt = parsed.lang == Lang::Pt;
    // Every part has to be clear before any is done.
    let steps = parsed.intents.into_iter().map(|i| plan(app, i)).collect::<Option<Vec<_>>>()?;
    let mut said = Vec::new();
    let mut cards = Vec::new();
    for s in steps {
        match run(app, s, pt, &target, &mut cards) {
            Ok(line) => said.push(line),
            Err(e) => said.push(e.to_string()),
        }
    }
    Some(Quick { text: said.join(" "), cards })
}

/// Finds what a part of the request acts on. None when it isn't clear (which timer?).
fn plan(app: &Ctx, i: Intent) -> Option<Step> {
    let (timers, alarms, ringing) = {
        let inner = state(app).inner.lock();
        let ringing: Vec<String> = inner.ringing.iter().map(|r| r.ring.id.clone()).collect();
        (inner.timers.clone(), inner.alarms.clone(), ringing)
    };
    // The timer named, or the only one there could be.
    let pick = |label: &Option<String>, fits: &dyn Fn(&Timer) -> bool| -> Option<Timer> {
        if let Some(l) = label {
            return best_timer(&timers, l).cloned();
        }
        let fitting: Vec<&Timer> = timers.iter().filter(|t| fits(t)).collect();
        match fitting.as_slice() {
            [one] => Some((*one).clone()),
            _ => timers.iter().find(|t| ringing.contains(&t.id)).cloned(),
        }
    };
    Some(match i {
        Intent::Timer { ms, label } => Step::Timer { ms, label },
        Intent::Alarm { hour, minute, days, when, label } => {
            let now = now_ms();
            let today = local::day(now);
            let date = match when {
                Some(When::Tomorrow) => Some(today.plus(1)),
                Some(When::Weekday(d)) => (0..8)
                    .map(|k| today.plus(k))
                    .find(|day| local::weekday(*day) == d && local::next_at(hour, minute, &[], Some(*day), now).is_some()),
                Some(When::Today) | None => None,
            };
            Step::Alarm { hour, minute, days, date, label }
        }
        Intent::Cancel { target: Target::Timer, label, all, .. } => {
            if timers.is_empty() {
                return Some(Step::Say("No timers are running.", "Nenhum timer rodando."));
            }
            match (all, &label) {
                (true, None) => Step::Cancel(timers.clone()),
                _ => Step::Cancel(vec![pick(&label, &|_| true)?]),
            }
        }
        Intent::Cancel { target: Target::Alarm, time, all, .. } => {
            let on: Vec<Alarm> = alarms.into_iter().filter(|a| a.enabled || a.snoozed.is_some()).collect();
            if on.is_empty() {
                return Some(Step::Say("No alarms are on.", "Nenhum alarme ligado."));
            }
            let chosen: Vec<Alarm> = match time {
                Some((h, m)) => on.into_iter().filter(|a| (a.hour, a.minute) == (h, m)).collect(),
                None if all || on.len() == 1 => on,
                None => return None,
            };
            if chosen.is_empty() {
                return None;
            }
            Step::AlarmsOff(chosen)
        }
        Intent::Add { ms, label } => Step::Add(pick(&label, &|_| true)?, ms),
        Intent::Pause { label } => match pick(&label, &|t| t.end_at.is_some()) {
            Some(t) => Step::Pause(t),
            None if label.is_none() && timers.is_empty() => Step::Say("No timers are running.", "Nenhum timer rodando."),
            None => return None,
        },
        Intent::Resume { label } => match pick(&label, &|t| t.left_ms.is_some()) {
            Some(t) => Step::Resume(t),
            None if label.is_none() && timers.is_empty() => Step::Say("No timers are running.", "Nenhum timer rodando."),
            None => return None,
        },
        Intent::Left { label } => Step::Left(pick(&label, &|_| true)?),
    })
}

/// "the pizza timer", "timer de pizza", "the 12-minute timer".
fn timer_words(t: &Timer, pt: bool) -> String {
    let name = t.label.to_lowercase();
    match (pt, t.named) {
        (false, _) if name.ends_with("timer") => name,
        (false, _) => format!("{name} timer"),
        (true, true) => format!("timer de {name}"),
        (true, false) => format!("timer de {}", spoken(t.total_ms, true)),
    }
}

fn run(app: &Ctx, s: Step, pt: bool, target: &Option<RingTarget>, cards: &mut Vec<Card>) -> Result<String> {
    let now = now_ms();
    let time = |h: u8, m: u8| format!("{h}:{m:02}");
    Ok(match s {
        Step::Timer { ms, label } => {
            let t = start_timer(app, ms, label.as_deref(), target.clone())?;
            cards.push(timer_card(&t, now));
            let length = spoken(ms, pt);
            match (pt, t.named) {
                (false, true) => format!("{} running, {length}.", capitalize(&timer_words(&t, false))),
                (false, false) => format!("Timer running, {length}."),
                (true, true) => format!("Timer de {length} para {} começou.", t.label.to_lowercase()),
                (true, false) => format!("Timer de {length} começou."),
            }
        }
        Step::Alarm { hour, minute, days, date, label } => {
            let edit = AlarmEdit {
                id: None,
                hour,
                minute,
                label: label.unwrap_or_default(),
                days: days.clone(),
                date: date.map(Day::iso),
                sound: Sound::Chime,
                scene: None,
                target: None,
            };
            let a = save_alarm(app, edit, target.clone())?;
            cards.push(alarm_card(&a, now));
            let when = match (a.next_at, days.is_empty()) {
                (Some(n), true) => day_word(n, now, pt),
                _ => days_phrase(&a.days, pt),
            };
            if pt {
                format!("Alarme marcado: {when}, às {}.", time(hour, minute))
            } else {
                format!("Your alarm is set for {} {when}.", time(hour, minute))
            }
        }
        Step::Cancel(list) => {
            for t in &list {
                change_timer(app, &t.id, Change::Cancel)?;
            }
            match (list.as_slice(), pt) {
                ([t], false) => format!("{} cancelled.", capitalize(&timer_words(t, false))),
                ([t], true) => format!("{} cancelado.", capitalize(&timer_words(t, true))),
                (_, false) => format!("{} timers cancelled.", list.len()),
                (_, true) => format!("{} timers cancelados.", list.len()),
            }
        }
        Step::AlarmsOff(list) => {
            for a in &list {
                if a.days.is_empty() {
                    delete_alarm(app, &a.id)?;
                } else {
                    set_alarm_on(app, &a.id, false)?;
                }
            }
            match (list.as_slice(), pt) {
                ([a], false) => format!("Your {} alarm is off.", time(a.hour, a.minute)),
                ([a], true) => format!("Alarme das {} desligado.", time(a.hour, a.minute)),
                (_, false) => format!("{} alarms turned off.", list.len()),
                (_, true) => format!("{} alarmes desligados.", list.len()),
            }
        }
        Step::Add(t, ms) => {
            let t = if state(app).inner.lock().ringing.iter().any(|r| r.ring.id == t.id) {
                respond(app, &t.id, Answer::Add(ms))?;
                find_timers(app, &t.id).into_iter().next().unwrap_or(t)
            } else {
                change_timer(app, &t.id, Change::Add(ms))?
            };
            cards.push(timer_card(&t, now));
            let (added, left) = (spoken(ms, pt), spoken(t.left(now), pt));
            if pt {
                format!("Mais {added} no {}, faltam {left}.", timer_words(&t, true))
            } else {
                format!("Added {added} to the {}, {left} left.", timer_words(&t, false))
            }
        }
        Step::Pause(t) => {
            let t = change_timer(app, &t.id, Change::Pause)?;
            cards.push(timer_card(&t, now));
            let left = spoken(t.left(now), pt);
            if pt {
                format!("{} pausado, faltam {left}.", capitalize(&timer_words(&t, true)))
            } else {
                format!("{} paused, {left} left.", capitalize(&timer_words(&t, false)))
            }
        }
        Step::Resume(t) => {
            let t = change_timer(app, &t.id, Change::Resume)?;
            cards.push(timer_card(&t, now));
            let left = spoken(t.left(now), pt);
            if pt {
                format!("{} de volta, faltam {left}.", capitalize(&timer_words(&t, true)))
            } else {
                format!("{} running again, {left} left.", capitalize(&timer_words(&t, false)))
            }
        }
        Step::Left(t) => {
            let done = t.end_at.is_some_and(|e| e <= now);
            if !done {
                cards.push(timer_card(&t, now));
            }
            let left = spoken(t.left(now), pt);
            match (done, pt) {
                (true, false) => format!("The {} is done.", timer_words(&t, false)),
                (true, true) => format!("O {} acabou.", timer_words(&t, true)),
                (false, false) => format!("{left} left on the {}.", timer_words(&t, false)),
                (false, true) => format!("Faltam {left} no {}.", timer_words(&t, true)),
            }
        }
        Step::Say(en, pt_text) => (if pt { pt_text } else { en }).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_names_and_looks() {
        assert_eq!(length_name(12 * MIN), "12-minute timer");
        assert_eq!(length_name(90_000), "1:30 timer");
        assert_eq!(length_name(45_000), "45-second timer");
        assert_eq!(length_name(2 * HOUR), "2-hour timer");
        assert_eq!(length_name(90 * MIN), "1h 30m timer");
        assert_eq!(look("Pizza"), ("🍕", "#E8590C"));
        assert_eq!(look("Chá verde").0, "🍵");
        assert_eq!(look("10-minute timer").0, "⏱️");
    }

    #[test]
    fn read_out() {
        assert_eq!(spoken(12 * MIN, false), "12 minutes");
        assert_eq!(spoken(90 * MIN, true), "1 hora e 30 minutos");
        assert_eq!(spoken(8 * MIN + 42_000, false), "8 minutes and 42 seconds");
        assert_eq!(spoken(MIN, true), "1 minuto");
        // Over ten minutes, to the minute.
        assert_eq!(spoken(23 * MIN + 40_000, false), "24 minutes");
        assert_eq!(clock_text(8 * MIN + 42_000), "8:42");
        assert_eq!(clock_text(HOUR + 2 * MIN + 3000), "1:02:03");
        assert_eq!(days_phrase(&[0, 2], false), "on Mondays and Wednesdays");
        assert_eq!(days_phrase(&[0, 2, 4], true), "às segundas, quartas e sextas");
        assert_eq!(days_phrase(&[0, 1, 2, 3, 4], true), "de segunda a sexta");
    }

    #[test]
    fn saved_alarms_without_new_fields() {
        let a: Alarm = serde_json::from_str(r#"{"id":"a1","hour":7,"minute":0,"label":"Wake up","enabled":true}"#).unwrap();
        assert_eq!((a.sound, a.days.len(), a.next_at, a.target), (Sound::Chime, 0, None, None));
        let s: Sound = serde_json::from_str(r#"{"type":"spotify","uri":"spotify:playlist:1","name":"Good morning"}"#).unwrap();
        assert_eq!(s, Sound::Spotify { uri: "spotify:playlist:1".into(), name: "Good morning".into() });
    }
}
