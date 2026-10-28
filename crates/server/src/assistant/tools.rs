//! What the assistant can do: each tool's JSON schema for the model, and running a call. From the
//! old launcher's `assistant/tools.rs`. A tool answers the model in JSON and may add a card to the
//! reply, a poster, or a step for the UI.
//!
//! The model sees every tool. Those about the house (devices, scenes, tasks, timers, alarms,
//! weather, news) run here; those about a TV (opening apps and games, titles, channels, volume,
//! media keys) run on the TV PC of the screen that was spoken to (`assistant.tool`), which sends
//! back its cards and posters.

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use tvl_kit::text::score;
use tvl_kit::time::now_ms;
use tvl_kit::{Error, Result};

use super::{news, weather, Card, Place};
use crate::clock;
use crate::ctx::Ctx;
use crate::smarthome::{self, scenes, AcMode, Command, Device, Hs};

/// What show_screen goes to.
const SCREENS: [&str; 9] = ["home", "games", "sports", "music", "live", "smarthome", "settings", "clock", "calendar"];

/// The tools a TV PC runs: they act on that TV.
pub const ON_THE_TV: &[&str] = &[
    "open_app",
    "find_titles",
    "trending_titles",
    "find_games",
    "show_title",
    "close_app",
    "watch_channel",
    "add_news_channel",
    "remove_news_channel",
    "set_volume",
    "media",
];

/// Cards, posters and UI steps gathered while the tools run. Posters and UI steps are JSON: most
/// come from a TV PC, which knows its titles and screens.
#[derive(Default)]
pub struct Outcome {
    pub cards: Vec<Card>,
    pub media: Vec<Value>,
    pub ui: Vec<Value>,
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": { "type": "object", "properties": properties, "required": required },
        }
    })
}

pub fn definitions() -> Vec<Value> {
    vec![
        tool(
            "open_app",
            "Open a streaming app or any program installed on the PC, by name. For a game, it shows the game \
selected on the Games screen, where the user starts (or installs) it.",
            json!({ "name": { "type": "string", "description": "App, game or program name, e.g. Netflix, Hades II, Spotify." } }),
            &["name"],
        ),
        tool(
            "find_titles",
            "Look up movies and series by title: year, synopsis and which of the user's streaming apps have them. \
Their posters are shown to the user, who can pick one to watch. Use it for every title you mention or recommend.",
            json!({ "titles": { "type": "array", "items": { "type": "string" }, "description": "One or more titles." } }),
            &["titles"],
        ),
        tool(
            "trending_titles",
            "The movies and series trending this week, shown as posters.",
            json!({}),
            &[],
        ),
        tool(
            "find_games",
            "The user's games (Steam and Epic): installed or not, play time. Shown as covers the user can pick \
to play or install.",
            json!({ "query": { "type": "string", "description": "Game name. Omit for the most recently played." } }),
            &[],
        ),
        tool(
            "show_title",
            "The user wants to watch a movie or series: shows its search result, where they pick the app to watch in.",
            json!({ "title": { "type": "string" } }),
            &["title"],
        ),
        tool(
            "show_search",
            "Show the launcher's search screen with results for a movie or series, for the user to browse.",
            json!({ "query": { "type": "string" } }),
            &["query"],
        ),
        tool(
            "show_screen",
            "Go to a screen of the launcher. \"clock\" is alarms and timers; \"live\" is live TV and streams.",
            json!({ "screen": { "type": "string", "enum": SCREENS } }),
            &["screen"],
        ),
        tool(
            "watch_channel",
            "Tune in to a live TV channel: Globoplay's (TV Globo, GloboNews, sportv, Premiere, Multishow…), ESPN 1–6, \
CazéTV, or a news channel on YouTube (CNN Brasil, GZH, Band Jornalismo, Jovem Pan News…).",
            json!({ "channel": { "type": "string", "description": "Channel name, e.g. GloboNews, CNN Brasil, sportv 2." } }),
            &["channel"],
        ),
        tool(
            "add_news_channel",
            "Add a YouTube channel to the Live tab's news channels, by name (e.g. Rádio Guaíba). The launcher finds \
its live streams from then on.",
            json!({ "name": { "type": "string" } }),
            &["name"],
        ),
        tool(
            "remove_news_channel",
            "Take a channel off the Live tab's news channels.",
            json!({ "name": { "type": "string" } }),
            &["name"],
        ),
        tool("close_app", "Close the streaming app that is open and come back to the launcher.", json!({}), &[]),
        tool(
            "control_devices",
            "Control smart home devices. Give only the settings to change. Turning off ignores the rest.",
            json!({
                "devices": { "type": "array", "items": { "type": "string" }, "description": "Device ids from the list." },
                "on": { "type": "boolean" },
                "brightness": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Percent." },
                "color_temp": { "type": "integer", "minimum": 0, "maximum": 100, "description": "0 warm white to 100 cool white." },
                "color": {
                    "type": "object",
                    "properties": { "h": { "type": "integer", "minimum": 0, "maximum": 359 }, "s": { "type": "integer", "minimum": 0, "maximum": 100 } },
                    "required": ["h", "s"],
                    "description": "Hue and saturation, for color lights.",
                },
                "fan_speed": { "type": "integer", "minimum": 1 },
                "ac_target": { "type": "number", "description": "Air conditioner target, °C." },
                "ac_mode": { "type": "string", "enum": ["cool", "heat", "auto", "fan", "dry"] },
            }),
            &["devices"],
        ),
        tool(
            "run_scene",
            "Run a smart home scene.",
            json!({ "scene": { "type": "string", "description": "Scene id from the list." } }),
            &["scene"],
        ),
        tool(
            "add_task",
            "Put a task (a chore, something to buy or remember) on a room's list, shown on every screen in that room.",
            json!({
                "title": { "type": "string", "description": "Short, e.g. Water the plants." },
                "room": { "type": "string", "description": "A room from the list; the one the request is about when none is named. Omit for the whole house." },
            }),
            &["title"],
        ),
        tool(
            "complete_task",
            "Tick a task off the rooms' lists (or put it back).",
            json!({
                "task": { "type": "string", "description": "Task id from the list." },
                "done": { "type": "boolean", "description": "False puts it back. Default true." },
            }),
            &["task"],
        ),
        tool(
            "set_volume",
            "Change the PC's (and TV's) volume.",
            json!({
                "level": { "type": "integer", "minimum": 0, "maximum": 100, "description": "Absolute volume, percent." },
                "change": { "type": "integer", "minimum": -100, "maximum": 100, "description": "Relative change in points, e.g. 10 or -10." },
                "mute": { "type": "boolean" },
            }),
            &[],
        ),
        tool(
            "media",
            "Media keys for whatever is playing: play/pause, next, previous.",
            json!({ "action": { "type": "string", "enum": ["play_pause", "next", "previous"] } }),
            &["action"],
        ),
        tool(
            "set_timer",
            "Start a timer. Give its length in any mix of hours, minutes and seconds.",
            json!({
                "hours": { "type": "number" },
                "minutes": { "type": "number" },
                "seconds": { "type": "number" },
                "label": { "type": "string", "description": "What it's for, e.g. Pizza. Omit when not said." },
            }),
            &[],
        ),
        tool(
            "set_alarm",
            "Set an alarm on the TV. Without days it rings once.",
            json!({
                "time": { "type": "string", "description": "24-hour HH:MM." },
                "days": {
                    "type": "array",
                    "items": { "type": "string", "enum": ["mon", "tue", "wed", "thu", "fri", "sat", "sun"] },
                    "description": "Days it repeats on.",
                },
                "date": { "type": "string", "description": "Once, on this day (YYYY-MM-DD). Omit for the next time the clock reads the time." },
                "label": { "type": "string", "description": "e.g. Wake up. Omit when not said." },
            }),
            &["time"],
        ),
        tool(
            "change_timer",
            "Cancel, pause, resume or add time to timers.",
            json!({
                "timer": { "type": "string", "description": "A timer's id or name from the list, or \"all\"." },
                "action": { "type": "string", "enum": ["cancel", "pause", "resume", "add"] },
                "minutes": { "type": "number", "description": "For add: minutes more (negative: fewer)." },
            }),
            &["timer", "action"],
        ),
        tool(
            "change_alarm",
            "Turn alarms on or off, or delete them.",
            json!({
                "alarm": { "type": "string", "description": "An alarm's id, HH:MM time or name from the list, or \"all\"." },
                "action": { "type": "string", "enum": ["on", "off", "delete"] },
            }),
            &["alarm", "action"],
        ),
        tool(
            "get_weather",
            "Current weather and the forecast.",
            json!({
                "location": { "type": "string", "description": "City. Omit for the user's own." },
                "days": { "type": "integer", "minimum": 1, "maximum": 7, "description": "Days of forecast, today included. Default 3." },
            }),
            &[],
        ),
        tool(
            "get_news",
            "Latest news headlines.",
            json!({ "topic": { "type": "string", "description": "Omit for top stories." } }),
            &[],
        ),
    ]
}


/// Runs one call; the result (or error) is JSON for the model.
pub async fn run(app: &Ctx, place: &Place, name: &str, args: &str, out: &mut Outcome) -> String {
    let result = dispatch(app, place, name, args, out).await;
    match result {
        Ok(v) => v.to_string(),
        Err(e) => json!({ "error": e.to_string() }).to_string(),
    }
}

fn parse<T: DeserializeOwned>(args: &str) -> Result<T> {
    let args = if args.trim().is_empty() { "{}" } else { args };
    serde_json::from_str(args).map_err(|e| Error::msg(format!("bad arguments: {e}")))
}

async fn dispatch(app: &Ctx, place: &Place, name: &str, args: &str, out: &mut Outcome) -> Result<Value> {
    if ON_THE_TV.contains(&name) {
        return on_the_tv(app, place, name, args, out).await;
    }
    match name {
        "show_search" => {
            let a: Query = parse(args)?;
            out.ui.push(json!({ "type": "navigate", "screen": "search", "query": a.query, "pick": null }));
            Ok(json!({ "ok": true }))
        }
        "show_screen" => {
            let a: ScreenArg = parse(args)?;
            if !SCREENS.contains(&a.screen.as_str()) {
                return Err(Error::msg(format!("no screen '{}'", a.screen)));
            }
            out.ui.push(json!({ "type": "navigate", "screen": a.screen, "query": null, "pick": null }));
            Ok(json!({ "ok": true }))
        }
        "control_devices" => control_devices(app, parse(args)?, out).await,
        "run_scene" => run_scene(app, parse(args)?, out).await,
        "add_task" => add_task(app, parse(args)?, out),
        "complete_task" => complete_task(app, parse(args)?, out),
        "set_timer" => set_timer(app, place, parse(args)?, out),
        "set_alarm" => set_alarm(app, place, parse(args)?, out),
        "change_timer" => change_timer(app, parse(args)?, out),
        "change_alarm" => change_alarm(app, parse(args)?, out),
        "get_weather" => get_weather(app, parse(args)?, out).await,
        "get_news" => get_news(app, parse(args)?, out).await,
        _ => Err(Error::msg(format!("unknown tool '{name}'"))),
    }
}

/// A tool about a TV, run by the TV PC of the screen that was spoken to: its answer for the
/// model, and the cards, posters and UI steps it made.
async fn on_the_tv(app: &Ctx, place: &Place, name: &str, args: &str, out: &mut Outcome) -> Result<Value> {
    let Some(device) = place.device.as_deref() else {
        return Err(Error::msg("no TV is on here to do that"));
    };
    let call = json!({ "name": name, "args": args });
    let reply = app
        .agents
        .call(device, "assistant.tool", call, place.screen.as_deref(), 60_000, false)
        .await
        .map_err(|e| Error::msg(format!("the TV didn't do it: {e:?}")))?;
    if let Some(cards) = reply.get("cards").cloned() {
        out.cards.extend(serde_json::from_value::<Vec<Card>>(cards).unwrap_or_default());
    }
    for m in reply.get("media").and_then(Value::as_array).into_iter().flatten() {
        if !out.media.iter().any(|x| x["id"] == m["id"]) {
            out.media.push(m.clone());
        }
    }
    out.ui.extend(reply.get("ui").and_then(Value::as_array).into_iter().flatten().cloned());
    Ok(reply.get("result").cloned().unwrap_or(Value::Null))
}

#[derive(Deserialize)]
struct Query {
    query: String,
}

#[derive(Deserialize)]
struct ScreenArg {
    screen: String,
}

/// The best scoring item, ties going to the earlier one.
fn best<'a, T>(query: &str, items: &'a [T], name: impl Fn(&T) -> &str) -> Option<(u8, &'a T)> {
    let mut found: Option<(u8, &T)> = None;
    for it in items {
        let s = score(query, name(it));
        if s > 0 && found.is_none_or(|(b, _)| s > b) {
            found = Some((s, it));
        }
    }
    found
}


// ---------- Smart home ----------

#[derive(Deserialize)]
struct Control {
    devices: Vec<String>,
    on: Option<bool>,
    brightness: Option<u8>,
    color_temp: Option<u8>,
    color: Option<Hs>,
    fan_speed: Option<u8>,
    ac_target: Option<f32>,
    ac_mode: Option<AcMode>,
}

impl Control {
    /// Off is just off; otherwise power first, then the rest.
    fn commands(&self) -> Vec<Command> {
        if self.on == Some(false) {
            return vec![Command::Power(false)];
        }
        let mut c = Vec::new();
        if self.on == Some(true) {
            c.push(Command::Power(true));
        }
        c.extend(self.brightness.map(|v| Command::Brightness(v.clamp(1, 100))));
        c.extend(self.color_temp.map(|v| Command::ColorTemp(v.min(100))));
        c.extend(self.color.map(Command::Color));
        c.extend(self.fan_speed.map(Command::FanSpeed));
        c.extend(self.ac_target.map(Command::AcTarget));
        c.extend(self.ac_mode.map(Command::AcMode));
        c
    }

    fn summary(&self) -> String {
        if self.on == Some(false) {
            return "Off".into();
        }
        let mut parts = Vec::new();
        if let Some(b) = self.brightness {
            parts.push(format!("{b}%"));
        }
        if self.color.is_some() {
            parts.push("color set".into());
        }
        if self.color_temp.is_some() {
            parts.push("white set".into());
        }
        if let Some(t) = self.ac_target {
            parts.push(format!("{t}°C"));
        }
        if let Some(m) = self.ac_mode {
            parts.push(format!("{m:?}").to_lowercase());
        }
        if let Some(f) = self.fan_speed {
            parts.push(format!("speed {f}"));
        }
        if parts.is_empty() {
            "On".into()
        } else {
            parts.join(" · ")
        }
    }
}

async fn control_devices(app: &Ctx, a: Control, out: &mut Outcome) -> Result<Value> {
    let home = smarthome::snapshot(app);
    let commands = a.commands();
    if commands.is_empty() {
        return Err(Error::msg("nothing to change was given"));
    }
    let mut done: Vec<&Device> = Vec::new();
    let mut failed = Vec::new();
    for id in &a.devices {
        let Some(d) = home.devices.iter().find(|d| &d.id == id) else {
            failed.push(json!({ "device": id, "error": "unknown device id" }));
            continue;
        };
        let mut result = Ok(());
        for c in &commands {
            result = smarthome::control(app, id, *c).await;
            if result.is_err() {
                break;
            }
        }
        match result {
            Ok(()) => done.push(d),
            Err(e) => failed.push(json!({ "device": d.name, "error": e.to_string() })),
        }
    }
    if let Some(first) = done.first() {
        let title = if done.len() == 1 {
            first.name.clone()
        } else if let Some(room) = first.room.as_ref().filter(|r| done.iter().all(|d| d.room.as_ref() == Some(*r))) {
            format!("{room} · {} devices", done.len())
        } else {
            format!("{} devices", done.len())
        };
        let icon = if done.iter().all(|d| d.kind == first.kind) { kind_icon(first) } else { "bulb" };
        out.cards.push(Card::new(icon, &title, &a.summary()).accent_if(a.on != Some(false)));
    }
    let names: Vec<_> = done.iter().map(|d| d.name.clone()).collect();
    Ok(json!({ "done": names, "failed": failed }))
}

fn kind_icon(d: &Device) -> &'static str {
    use smarthome::Kind::*;
    match d.kind {
        Light => "bulb",
        Strip => "strip",
        Switch => "switch",
        Plug => "plug",
        Fan => "fan",
        Ac => "ac",
        Sensor => "sensor",
    }
}

#[derive(Deserialize)]
struct SceneArg {
    scene: String,
}

async fn run_scene(app: &Ctx, a: SceneArg, out: &mut Outcome) -> Result<Value> {
    let list = smarthome::snapshot(app).scenes;
    let scene = list
        .iter()
        .find(|s| s.id == a.scene)
        .or_else(|| best(&a.scene, &list, |s| s.name.as_str()).map(|(_, s)| s));
    let (id, name, icon) = match scene {
        Some(s) => (s.id.clone(), s.name.clone(), s.icon.clone()),
        None if a.scene == scenes::ALL_OFF => (a.scene.clone(), "All off".to_string(), "power".to_string()),
        None => return Err(Error::msg(format!("no scene '{}'", a.scene))),
    };
    scenes::run(app, &id).await?;
    out.cards.push(Card::new(&icon, &name, "Scene on").accent());
    Ok(json!({ "ran": name }))
}

#[derive(Deserialize)]
struct TaskArg {
    title: String,
    room: Option<String>,
}

fn add_task(app: &Ctx, a: TaskArg, out: &mut Outcome) -> Result<Value> {
    let home = smarthome::snapshot(app);
    let room = match a.room.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
        None => None,
        Some(r) => {
            let rooms = smarthome::rooms::names(&home.layout, &home.devices);
            let found = rooms.iter().find(|x| x.eq_ignore_ascii_case(r)).cloned().or_else(|| best(r, &rooms, |x| x.as_str()).map(|(_, x)| x.clone()));
            Some(found.ok_or_else(|| Error::msg(format!("no room '{r}'")))?)
        }
    };
    smarthome::rooms::put_task(app, None, room.as_deref(), &a.title)?;
    out.cards.push(Card::new("check", &a.title, &format!("Added to the {}", room.as_deref().unwrap_or("house's list"))));
    Ok(json!({ "added": a.title, "room": room }))
}

#[derive(Deserialize)]
struct DoneArg {
    task: String,
    done: Option<bool>,
}

fn complete_task(app: &Ctx, a: DoneArg, out: &mut Outcome) -> Result<Value> {
    let tasks = smarthome::snapshot(app).tasks;
    let t = tasks
        .iter()
        .find(|t| t.id == a.task)
        .or_else(|| best(&a.task, &tasks, |t| t.title.as_str()).map(|(_, t)| t))
        .ok_or_else(|| Error::msg(format!("no task '{}'", a.task)))?;
    let done = a.done.unwrap_or(true);
    smarthome::rooms::task_done(app, &t.id, done)?;
    out.cards.push(Card::new("check", &t.title, if done { "Done" } else { "Back on the list" }).accent_if(done));
    Ok(json!({ "task": t.title, "done": done }))
}

// ---------- Timers and alarms ----------

#[derive(Deserialize)]
struct TimerArg {
    hours: Option<f64>,
    minutes: Option<f64>,
    seconds: Option<f64>,
    label: Option<String>,
}

fn set_timer(app: &Ctx, place: &Place, a: TimerArg, out: &mut Outcome) -> Result<Value> {
    let ms = a.hours.unwrap_or(0.0) * 3_600_000.0 + a.minutes.unwrap_or(0.0) * 60_000.0 + a.seconds.unwrap_or(0.0) * 1000.0;
    let t = clock::start_timer(app, ms.round() as i64, a.label.as_deref(), place.ring_target.clone())?;
    out.cards.push(clock::timer_card(&t, now_ms()));
    Ok(json!({ "started": t.label, "id": t.id }))
}

#[derive(Deserialize)]
struct AlarmArg {
    time: String,
    #[serde(default)]
    days: Vec<String>,
    date: Option<String>,
    label: Option<String>,
}

fn set_alarm(app: &Ctx, place: &Place, a: AlarmArg, out: &mut Outcome) -> Result<Value> {
    const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
    let (hour, minute) = a
        .time
        .trim()
        .split_once(':')
        .and_then(|(h, m)| Some((h.trim().parse::<u8>().ok()?, m.trim().get(..2)?.parse::<u8>().ok()?)))
        .ok_or_else(|| Error::msg("time must be HH:MM"))?;
    let days = a
        .days
        .iter()
        .filter_map(|d| DAYS.iter().position(|x| d.to_lowercase().starts_with(x)).map(|i| i as u8))
        .collect();
    let alarm = clock::save_alarm(
        app,
        clock::AlarmEdit {
            id: None,
            hour,
            minute,
            label: a.label.unwrap_or_default(),
            days,
            date: a.date,
            sound: clock::Sound::Chime,
            scene: None,
            target: None,
        },
        place.ring_target.clone(),
    )?;
    out.cards.push(clock::alarm_card(&alarm, now_ms()));
    Ok(json!({ "set": alarm.label, "id": alarm.id, "rings_next": clock::next_text(&alarm) }))
}

#[derive(Deserialize)]
struct TimerChange {
    timer: String,
    action: String,
    minutes: Option<f64>,
}

fn change_timer(app: &Ctx, a: TimerChange, out: &mut Outcome) -> Result<Value> {
    let change = match a.action.as_str() {
        "cancel" => clock::Change::Cancel,
        "pause" => clock::Change::Pause,
        "resume" => clock::Change::Resume,
        "add" => clock::Change::Add((a.minutes.unwrap_or(1.0) * 60_000.0).round() as i64),
        other => return Err(Error::msg(format!("unknown timer action '{other}'"))),
    };
    let timers = clock::find_timers(app, &a.timer);
    if timers.is_empty() {
        return Err(Error::msg(format!("no timer '{}'", a.timer)));
    }
    let mut done = Vec::new();
    for t in timers {
        let t = clock::change_timer(app, &t.id, change)?;
        out.cards.push(match change {
            clock::Change::Cancel => Card::new("close", &t.label, "Timer cancelled"),
            _ => clock::timer_card(&t, now_ms()),
        });
        done.push(t.label);
    }
    Ok(json!({ "done": done }))
}

#[derive(Deserialize)]
struct AlarmChange {
    alarm: String,
    action: String,
}

fn change_alarm(app: &Ctx, a: AlarmChange, out: &mut Outcome) -> Result<Value> {
    let alarms = clock::find_alarms(app, &a.alarm);
    if alarms.is_empty() {
        return Err(Error::msg(format!("no alarm '{}'", a.alarm)));
    }
    let mut done = Vec::new();
    for al in alarms {
        let time = format!("{:02}:{:02}", al.hour, al.minute);
        match a.action.as_str() {
            "on" => out.cards.push(clock::alarm_card(&clock::set_alarm_on(app, &al.id, true)?, now_ms())),
            "off" => {
                clock::set_alarm_on(app, &al.id, false)?;
                out.cards.push(Card::new("bell", &time, "Alarm off"));
            }
            "delete" => {
                clock::delete_alarm(app, &al.id)?;
                out.cards.push(Card::new("close", &time, "Alarm deleted"));
            }
            other => return Err(Error::msg(format!("unknown alarm action '{other}'"))),
        }
        done.push(time);
    }
    Ok(json!({ "done": done }))
}

// ---------- Weather and news ----------

#[derive(Deserialize)]
struct WeatherArg {
    location: Option<String>,
    days: Option<u8>,
}

async fn get_weather(app: &Ctx, a: WeatherArg, out: &mut Outcome) -> Result<Value> {
    let http = tvl_kit::http::api();
    let settings = app.settings.get();
    let place = a
        .location
        .filter(|l| !l.trim().is_empty())
        .or_else(|| Some(settings.assistant_location.clone()).filter(|l| !l.trim().is_empty()));
    let f = weather::forecast(&http, place.as_deref(), &settings.language, a.days.unwrap_or(3)).await?;
    let c = &f.current;
    out.cards.push(Card::new(
        weather::icon(c.code),
        &format!("{} · {:.0}°", f.place, c.temperature_c),
        &capitalize(c.conditions),
    ));
    Ok(serde_json::to_value(&f)?)
}

#[derive(Deserialize)]
struct NewsArg {
    topic: Option<String>,
}

async fn get_news(app: &Ctx, a: NewsArg, out: &mut Outcome) -> Result<Value> {
    let http = tvl_kit::http::api();
    let settings = app.settings.get();
    let items = news::headlines(&http, a.topic.as_deref(), &settings.language, &settings.region).await?;
    let title = a.topic.as_deref().filter(|t| !t.trim().is_empty()).map_or("Top stories".to_string(), capitalize);
    out.cards.push(Card::new("news", &title, &format!("{} headlines", items.len())));
    Ok(json!({ "headlines": items }))
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_loosely() {
        assert_eq!(score("Netflix", "Netflix"), 100);
        assert_eq!(score("hbo max", "HBO Max"), 100);
        assert_eq!(score("hbomax", "HBO Max"), 95);
        assert_eq!(score("hades", "Hades II"), 80);
        assert_eq!(score("Pokémon", "pokemon"), 100);
        assert_eq!(score("ring", "Elden Ring"), 65);
        assert_eq!(score("open spotify please", "Spotify"), 55);
        assert_eq!(score("ring elden", "Elden Ring"), 45);
        assert_eq!(score("xyz", "Netflix"), 0);
        assert_eq!(score("", "Netflix"), 0);
    }

    #[test]
    fn best_prefers_higher_then_earlier() {
        let items = ["Hades", "Hades II", "Hollow Knight"];
        assert_eq!(best("hades", &items, |s| s).map(|(_, s)| *s), Some("Hades"));
        assert_eq!(best("hades ii", &items, |s| s).map(|(_, s)| *s), Some("Hades II"));
        assert_eq!(best("celeste", &items, |s| s), None);
    }

    #[test]
    fn turning_off_ignores_the_rest() {
        let c: Control = serde_json::from_str(r#"{"devices":["a"],"on":false,"brightness":50}"#).unwrap();
        assert_eq!(c.commands(), vec![Command::Power(false)]);
        assert_eq!(c.summary(), "Off");
        let c: Control =
            serde_json::from_str(r#"{"devices":["a"],"on":true,"brightness":140,"color":{"h":200,"s":80}}"#).unwrap();
        assert_eq!(
            c.commands(),
            vec![Command::Power(true), Command::Brightness(100), Command::Color(Hs { h: 200, s: 80 })]
        );
        let c: Control = serde_json::from_str(r#"{"devices":["ac"],"ac_target":22,"ac_mode":"cool"}"#).unwrap();
        assert_eq!(c.summary(), "22°C · cool");
    }

    #[test]
    fn every_tool_has_a_name_and_object_parameters() {
        let defs = definitions();
        assert!(defs.len() >= 10);
        for d in defs {
            assert_eq!(d["type"], "function");
            assert!(d["function"]["name"].as_str().is_some_and(|n| !n.is_empty()));
            assert_eq!(d["function"]["parameters"]["type"], "object");
        }
    }
}
