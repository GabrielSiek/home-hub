//! The assistant's brain, on the server: an LLM on DeepInfra that answers questions and drives the
//! house through tools (smart home, timers and alarms, tasks, weather, news) and the TV of the
//! screen it was spoken to (apps, games, titles, channels, volume). From the old launcher's
//! `assistant/`. Timer and alarm requests it can read by itself ("timer de 12 minutos pra pizza")
//! are done without asking the model (clock::quick), DeepInfra key or not.
//!
//! Each screen has its own conversation, so follow-ups ("and tomorrow?") work there; it's
//! forgotten after a while without use. (The old launcher had one conversation for every screen.)
//! Hearing and speaking stay where the microphones and speakers are: a TV PC records and speaks
//! with its own voice; the words are made into text here (transcribe.rs).

pub mod llm;
mod news;
pub mod tools;
pub mod transcribe;
mod weather;

use std::collections::HashMap;
use std::fmt::Write;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tvl_kit::time::now_ms;
use tvl_kit::{Error, Result};
use tvl_protocol::clock::RingTarget;

use crate::ctx::Ctx;
use crate::router::Caller;
use crate::{clock, smarthome};

/// Model calls per question; tools that need another tool's answer take a few.
const MAX_ROUNDS: usize = 5;
/// Messages kept for follow-ups.
const HISTORY_LEN: usize = 24;
/// After this long without a question, a screen's next one starts a fresh conversation.
const FORGET_AFTER: Duration = Duration::from_secs(15 * 60);
const MAX_GAMES_IN_PROMPT: usize = 80;

/// Something the assistant did, shown under its reply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    /// An icon name the UI knows.
    pub icon: String,
    pub title: String,
    pub subtitle: String,
    /// Drawn in the accent color: something started or turned on.
    #[serde(default)]
    pub accent: bool,
    /// A timer or alarm the card follows live, and opens the Clock screen on: "timer:<id>",
    /// "alarm:<id>".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock: Option<String>,
}

impl Card {
    pub fn new(icon: &str, title: &str, subtitle: &str) -> Self {
        Self { icon: icon.into(), title: title.into(), subtitle: subtitle.into(), accent: false, clock: None }
    }

    pub fn clock(mut self, what: &str) -> Self {
        self.clock = Some(what.into());
        self
    }

    pub fn accent(self) -> Self {
        self.accent_if(true)
    }

    pub fn accent_if(mut self, on: bool) -> Self {
        self.accent = on;
        self
    }
}

#[derive(Debug, Serialize)]
pub struct Reply {
    pub text: String,
    pub cards: Vec<Card>,
    /// Movies, series and games the reply is about (from the TV PC that looked them up).
    pub media: Vec<Value>,
    /// Steps for the UI after the reply (navigate, close).
    pub ui: Vec<Value>,
}

struct Conversation {
    history: Vec<Value>,
    last: Instant,
}

#[derive(Default)]
pub struct Assistant {
    /// By screen.
    conversations: Mutex<HashMap<String, Conversation>>,
}

/// Where a request came from, as the screens say it.
#[derive(Debug, Default, Deserialize)]
pub struct Asked {
    /// The screen it was said to; None: the caller.
    pub screen: Option<String>,
    /// The room it's about when it names none, if not the screen's (a phone in the room it has
    /// open).
    pub room: Option<String>,
}

/// The screen a request was said to, the room "the lights" means there, the TV that "open
/// Netflix" means, and where a timer set by it rings.
pub struct Place {
    by: String,
    room: Option<String>,
    screen: Option<String>,
    /// The TV PC that runs the tools about a TV, when one is on.
    device: Option<String>,
    ring_target: Option<RingTarget>,
}

fn place(app: &Ctx, caller: &Caller, asked: &Asked) -> Place {
    let screen = asked.screen.clone().or_else(|| caller.screen.clone());
    let s = screen.as_deref().and_then(|id| app.devices.get(id));
    Place {
        by: s.as_ref().map_or_else(|| "a screen".to_string(), |s| s.name.clone()),
        room: asked.room.clone().or_else(|| s.as_ref().and_then(|s| s.room.clone())),
        device: app.devices.target(screen.as_deref(), None).ok(),
        ring_target: clock::target_for(app, screen.as_deref()),
        screen,
    }
}

pub(crate) fn api_key(app: &Ctx) -> Result<String> {
    Some(app.settings.get().deepinfra_api_key.trim().to_string())
        .filter(|k| !k.is_empty())
        .ok_or_else(|| Error::msg("Add a DeepInfra API key in Settings to use the assistant"))
}

/// Answers `text`, running whatever tools the model asks for. `now`: the asker's local date and
/// time, as its screen sees it.
pub async fn ask(app: &Ctx, caller: &Caller, text: &str, now: &str, asked: Asked) -> Result<Reply> {
    let place = place(app, caller, &asked);
    let reply = answer(app, text, now, &place).await?;
    smarthome::rooms::heard(
        app,
        smarthome::Heard {
            said: text.trim().to_string(),
            by: place.by.clone(),
            room: place.room.clone(),
            reply: reply.text.clone(),
            at: now_ms(),
        },
    );
    Ok(reply)
}

/// The screen's conversation so far (a fresh one after a while without use).
fn history_of(app: &Ctx, key: &str) -> Vec<Value> {
    let mut all = app.assistant.conversations.lock();
    let c = all.entry(key.to_string()).or_insert_with(|| Conversation { history: Vec::new(), last: Instant::now() });
    if c.last.elapsed() > FORGET_AFTER {
        c.history.clear();
    }
    c.last = Instant::now();
    c.history.clone()
}

fn keep(app: &Ctx, key: &str, history: Vec<Value>) {
    let mut all = app.assistant.conversations.lock();
    all.retain(|_, c| c.last.elapsed() < FORGET_AFTER);
    all.insert(key.to_string(), Conversation { history: trimmed(history), last: Instant::now() });
}

async fn answer(app: &Ctx, text: &str, now: &str, place: &Place) -> Result<Reply> {
    let key = place.screen.clone().unwrap_or_default();
    if let Some(done) = clock::quick(app, text, place.ring_target.clone()) {
        // Kept in the conversation, so the model can follow up on it ("make it 15").
        let mut history = history_of(app, &key);
        history.push(json!({ "role": "user", "content": text.trim() }));
        history.push(json!({ "role": "assistant", "content": done.text }));
        keep(app, &key, history);
        return Ok(Reply { text: done.text, cards: done.cards, media: Vec::new(), ui: Vec::new() });
    }
    let api_key = api_key(app)?;
    let model = Some(app.settings.get().assistant_model.trim().to_string())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| llm::DEFAULT_MODEL.to_string());

    let mut history = history_of(app, &key);
    history.push(json!({ "role": "user", "content": text.trim() }));
    let system = json!({ "role": "system", "content": system_prompt(app, now, place) });
    let definitions = tools::definitions();
    let mut out = tools::Outcome::default();
    let mut reply = String::new();
    let http = tvl_kit::http::api();
    for round in 0..=MAX_ROUNDS {
        // The last round has no tools, so the model has to say something.
        let offered: &[Value] = if round < MAX_ROUNDS { &definitions } else { &[] };
        let messages: Vec<Value> = std::iter::once(system.clone()).chain(history.iter().cloned()).collect();
        let msg = llm::chat(&http, &api_key, &model, &messages, offered).await?;
        history.push(msg.to_history());
        if msg.tool_calls().is_empty() {
            reply = msg.text();
            break;
        }
        for call in msg.tool_calls() {
            let result = tools::run(app, place, &call.function.name, &call.function.arguments, &mut out).await;
            history.push(json!({ "role": "tool", "tool_call_id": call.id, "content": result }));
        }
    }
    if reply.is_empty() {
        reply = if out.cards.is_empty() { "Sorry, I didn't get that." } else { "Done." }.to_string();
    }
    keep(app, &key, history);
    Ok(Reply { text: reply, cards: out.cards, media: out.media, ui: out.ui })
}

/// The newest messages, starting at a question so no tool answer loses its call.
fn trimmed(mut h: Vec<Value>) -> Vec<Value> {
    if h.len() > HISTORY_LEN {
        h.drain(..h.len() - HISTORY_LEN);
    }
    let start = h.iter().position(|m| m["role"] == "user").unwrap_or(h.len());
    h.drain(..start);
    h
}

fn system_prompt(app: &Ctx, now: &str, place: &Place) -> String {
    let settings = app.settings.get();
    let mut p = String::from(
        "You are the assistant of a home's TV launcher: TV PCs on the TVs, used from the couch with a game \
controller, and phones and tablets around the house. You answer questions and control the TVs, the PCs and the \
smart home.\n\
Rules:\n\
- Replies appear on the screen and may be read aloud: one to three short sentences, conversational, plain text. \
No markdown, lists, emoji, links or ids.\n\
- Answer in the language the user writes or speaks in.\n\
- Use the tools to act, and only say something was done after its tool succeeded. If a tool fails, say so briefly.\n\
- Device, scene and app ids come only from the lists below. When a request names a room or a kind of device, \
act on every matching device at once.\n\
- For weather and news, call the tool and summarize the essentials; don't read everything out.\n\
- Whenever you talk about specific movies or series, look them up with find_titles (trending_titles for what's \
popular), and games with find_games: the user sees their posters and can pick one to watch or play. \
To watch a movie or series, use show_title: it opens the title's search result, where the user picks the app. \
For a game, open_app shows it on the Games screen. Never start a title or a game yourself: the user does.\n\
- Timers and alarms: set_timer, set_alarm, change_timer and change_alarm, on the ones listed below. Times are \
24-hour; an alarm with no days rings once.\n\
- If a request is unclear, ask one short question.\n\n",
    );
    let _ = writeln!(p, "Now: {now}. Language: {}. Region: {}.", settings.language, settings.region);
    match &place.room {
        Some(room) => _ = writeln!(
            p,
            "Said to {} in the {room}. A request that names no room and doesn't say all or the whole house \
(\"turn on the lights\", \"it's cold in here\") is about the {room}'s devices only. Naming a room, or saying all \
or everywhere, reaches those.",
            place.by
        ),
        None => _ = writeln!(p, "Said to {}, which isn't in any room: when a request could mean several rooms, ask which.", place.by),
    }

    // The TV that "open Netflix" means: what it says about itself.
    let context = place.device.as_deref().and_then(|d| app.agent_latest(d, "device.context"));
    match (&place.device, &context) {
        (Some(d), Some(c)) => {
            let tv = app.devices.get(d).map_or_else(|| "the TV".to_string(), |t| t.name);
            match c["running"]["name"].as_str() {
                Some(name) => _ = writeln!(p, "On {tv} right now: {name} (close_app closes it)."),
                None => _ = writeln!(p, "{tv} shows the launcher; no app is open."),
            }
            p.push_str("\nStreaming apps (id: name):\n");
            for a in c["apps"].as_array().into_iter().flatten() {
                let _ = writeln!(p, "- {}: {}", a["id"].as_str().unwrap_or("?"), a["name"].as_str().unwrap_or("?"));
            }
            let games: Vec<&str> = c["games"].as_array().into_iter().flatten().filter_map(Value::as_str).take(MAX_GAMES_IN_PROMPT).collect();
            if !games.is_empty() {
                let _ = writeln!(p, "\nInstalled games: {}.", games.join(", "));
            }
        }
        (Some(_), None) => p.push_str("The TV here is on, but hasn't said what it has.\n"),
        (None, _) => p.push_str("No TV is on here: opening apps, playing, channels and the volume can't be done now.\n"),
    }

    let home = smarthome::snapshot(app);
    if home.devices.is_empty() {
        p.push_str("\nNo smart home devices are connected.\n");
    } else {
        p.push_str("\nSmart home devices (id | name | room | kind | state):\n");
        for d in &home.devices {
            let _ = writeln!(p, "- {} | {} | {} | {:?} | {}", d.id, d.name, d.room.as_deref().unwrap_or("-"), d.kind, device_state(d));
        }
        p.push_str("Scenes (id: name · the room they're for):\n- all-off: All off · whole house\n");
        for s in &home.scenes {
            let _ = writeln!(p, "- {}: {} · {}", s.id, s.name, s.room.as_deref().unwrap_or("whole house"));
        }
    }
    let rooms = smarthome::rooms::names(&home.layout, &home.devices);
    if !rooms.is_empty() {
        let _ = writeln!(p, "Rooms: {}.", rooms.join(", "));
    }
    let open: Vec<_> = home.tasks.iter().filter(|t| t.done_at.is_none()).collect();
    if open.is_empty() {
        p.push_str("The rooms' task lists are empty (add_task adds to one).\n");
    } else {
        p.push_str("Tasks on the rooms' lists (id | room | task):\n");
        for t in open {
            let _ = writeln!(p, "- {} | {} | {}", t.id, t.room.as_deref().unwrap_or("whole house"), t.title);
        }
    }
    p.push_str(&clock::describe(app));
    p
}

/// A device's state in a few words, for the model.
fn device_state(d: &smarthome::Device) -> String {
    if !d.online {
        return "offline".into();
    }
    let mut parts = Vec::new();
    match d.on {
        Some(true) => parts.push("on".to_string()),
        Some(false) => parts.push("off".to_string()),
        None => {}
    }
    if d.on == Some(true) {
        if let Some(b) = d.brightness {
            parts.push(format!("{b}%"));
        }
        if let Some(ac) = &d.ac {
            parts.push(format!("{:?} {}°C", ac.mode, ac.target).to_lowercase());
        }
        if let Some(f) = d.fan_speed {
            parts.push(format!("speed {f}"));
        }
    }
    if let Some(t) = d.temperature {
        parts.push(format!("{t}°C"));
    }
    if let Some(h) = d.humidity {
        parts.push(format!("{h}% humidity"));
    }
    if let Some(w) = d.power_w {
        parts.push(format!("{w} W"));
    }
    if d.color.is_some() || d.color_temp.is_some() {
        parts.push("dimmable color".into());
    }
    if parts.is_empty() {
        "-".into()
    } else {
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimming_keeps_whole_exchanges() {
        let mut h = Vec::new();
        for i in 0..20 {
            h.push(json!({ "role": "user", "content": i.to_string() }));
            h.push(json!({ "role": "assistant", "content": "", "tool_calls": [] }));
            h.push(json!({ "role": "tool", "content": "{}" }));
        }
        let t = trimmed(h);
        assert!(t.len() <= HISTORY_LEN);
        assert_eq!(t[0]["role"], "user");
        assert_eq!(t.last().unwrap()["role"], "tool");
    }
}
