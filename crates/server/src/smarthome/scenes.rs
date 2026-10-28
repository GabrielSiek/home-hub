//! Scenes kept by the launcher (scenes.json): a saved state per device (on/off, brightness,
//! white or colour, an AC's mode and temperature) across every provider, and automations
//! (automations.json) that run a scene at a time of day. "All off" is built in. A scene belongs
//! to a room or to the whole house; its automations go with it (the room's routines).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use parking_lot::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::ctx::Ctx;
use super::{control, local, snapshot, AcFan, AcMode, ColorMode, Command, Device, Hs, Kind};
use tvl_kit::time::local as wall;
use tvl_kit::{Error, Result};
use tvl_kit::time::now_ms;

const FILE: &str = "scenes.json";
const AUTOMATIONS_FILE: &str = "automations.json";
pub const ALL_OFF: &str = "all-off";
/// How often the automations' clock looks at the time.
const TICK: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub id: String,
    pub name: String,
    /// Icon name the UI knows (movie, games, moon, sun, away…).
    pub icon: String,
    /// The card's colours (purple, blue, teal, warm, night, grey); the UI picks one when empty.
    #[serde(default)]
    pub theme: String,
    /// The room it's one of the routines of; None: the whole house.
    #[serde(default)]
    pub room: Option<String>,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub device: String,
    pub on: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness: Option<u8>,
    /// White at this temperature (0 warm – 100 cool)…
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_temp: Option<u8>,
    /// …or this colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Hs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ac: Option<AcAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AcAction {
    pub mode: AcMode,
    pub target: f32,
    #[serde(default)]
    pub fan: Option<AcFan>,
}

/// Runs a scene every day, or on some weekdays, at a time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Automation {
    pub id: String,
    pub scene: String,
    pub hour: u8,
    pub minute: u8,
    /// Monday 0 … Sunday 6; empty for every day.
    #[serde(default)]
    pub days: Vec<u8>,
    pub enabled: bool,
}

pub struct Store {
    path: PathBuf,
    automations_path: PathBuf,
    scenes: Mutex<Vec<Scene>>,
    automations: Mutex<Vec<Automation>>,
}

/// A file read back, or empty; one that doesn't parse is set aside (never overwritten).
fn read<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    tvl_kit::store::load(path).unwrap_or_default()
}

impl Store {
    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join(FILE);
        let automations_path = data_dir.join(AUTOMATIONS_FILE);
        Self {
            scenes: Mutex::new(read(&path)),
            automations: Mutex::new(read(&automations_path)),
            path,
            automations_path,
        }
    }

    pub fn list(&self) -> Vec<Scene> {
        self.scenes.lock().clone()
    }

    pub fn automations(&self) -> Vec<Automation> {
        self.automations.lock().clone()
    }

    fn write(&self, scenes: &[Scene]) -> Result<()> {
        tvl_kit::store::save(&self.path, scenes, true)
    }

    fn write_automations(&self, list: &[Automation]) -> Result<()> {
        tvl_kit::store::save(&self.automations_path, list, true)
    }

    /// Scenes of room `from` move to `to` (None: they become the whole house's).
    pub fn move_room(&self, from: &str, to: Option<&str>) -> Result<()> {
        let mut scenes = self.scenes.lock();
        let mut changed = false;
        for s in scenes.iter_mut().filter(|s| s.room.as_deref() == Some(from)) {
            s.room = to.map(str::to_string);
            changed = true;
        }
        if changed {
            self.write(&scenes)?;
        }
        Ok(())
    }
}

fn store(app: &Ctx) -> &Store {
    &app.smarthome.scenes
}

/// What `d` is doing now, as a scene action; None for what can't be switched.
pub fn action_of(d: &Device) -> Option<Action> {
    if d.kind == Kind::Sensor || !d.members.is_empty() {
        return None;
    }
    let on = d.on?;
    let colour = d.color_mode == Some(ColorMode::Color);
    Some(Action {
        device: d.id.clone(),
        on,
        brightness: if on { d.brightness } else { None },
        color_temp: if on && !colour { d.color_temp } else { None },
        color: if on && colour { d.color } else { None },
        ac: d.ac.as_ref().filter(|_| on).map(|a| AcAction {
            mode: a.mode,
            target: a.target,
            fan: a.fan,
        }),
    })
}

/// What a scene saved now would hold: every switchable device that answers.
pub fn capture(devices: &[Device]) -> Vec<Action> {
    devices.iter().filter(|d| d.online).filter_map(action_of).collect()
}

/// The commands that take `d` to the state in `a`; nothing when it is already there. Colour
/// or white comes before brightness, which applies to whichever is showing.
pub fn commands_for(d: &Device, a: &Action) -> Vec<Command> {
    let mut out = Vec::new();
    if d.on != Some(a.on) {
        out.push(Command::Power(a.on));
    }
    if !a.on {
        return out;
    }
    let colour = d.color_mode == Some(ColorMode::Color);
    if let (Some(c), Some(_)) = (a.color, d.color) {
        if d.color != Some(c) || !colour {
            out.push(Command::Color(c));
        }
    } else if let (Some(t), Some(_)) = (a.color_temp, d.color_temp) {
        if d.color_temp != Some(t) || colour {
            out.push(Command::ColorTemp(t));
        }
    }
    if let (Some(want), Some(_)) = (a.brightness, d.brightness) {
        if d.brightness != Some(want) || out.iter().any(|c| matches!(c, Command::Color(_))) {
            out.push(Command::Brightness(want));
        }
    }
    if let (Some(want), Some(cur)) = (a.ac, &d.ac) {
        if cur.mode != want.mode {
            out.push(Command::AcMode(want.mode));
        }
        if let Some(f) = want.fan.filter(|f| cur.fan != Some(*f)) {
            out.push(Command::AcFan(f));
        }
        if (cur.target - want.target).abs() > f32::EPSILON {
            out.push(Command::AcTarget(want.target));
        }
    }
    out
}

fn new_id(name: &str) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    format!("{}-{}", if slug.is_empty() { "scene" } else { &slug }, now_ms() % 1_000_000)
}

/// Saves (or replaces) a scene with the devices as they are now: those in `room`, or all of
/// them for the whole house. Returns its id.
pub fn save(app: &Ctx, id: &str, name: &str, icon: &str, room: Option<&str>) -> Result<String> {
    let devices = snapshot(app).devices;
    let actions = match room {
        Some(r) => capture(&super::rooms::devices_in(&devices, r).into_iter().cloned().collect::<Vec<_>>()),
        None => capture(&devices),
    };
    if actions.is_empty() {
        return Err(Error::msg(match room {
            Some(r) => format!("Nothing in the {r} to save yet"),
            None => "No devices to save in a scene yet".to_string(),
        }));
    }
    put(
        app,
        Scene {
            id: id.to_string(),
            name: name.to_string(),
            icon: icon.to_string(),
            theme: String::new(),
            room: room.map(str::to_string),
            actions,
        },
    )
}

/// Saves a scene from the editor; an empty id makes a new one. Returns its id.
pub fn put(app: &Ctx, mut scene: Scene) -> Result<String> {
    if scene.id == ALL_OFF {
        return Err(Error::msg("That scene can't be changed"));
    }
    scene.name = scene.name.trim().to_string();
    if scene.name.is_empty() {
        return Err(Error::msg("Give the scene a name"));
    }
    if scene.actions.is_empty() {
        return Err(Error::msg("Pick at least one device for the scene"));
    }
    if scene.id.trim().is_empty() {
        scene.id = new_id(&scene.name);
    }
    let store = store(app);
    let mut scenes = store.scenes.lock();
    match scenes.iter_mut().find(|s| s.id == scene.id) {
        Some(s) => *s = scene.clone(),
        None => scenes.push(scene.clone()),
    }
    store.write(&scenes)?;
    Ok(scene.id)
}

/// Deletes a scene and the automations that ran it.
pub fn delete(app: &Ctx, id: &str) -> Result<()> {
    let store = store(app);
    let mut scenes = store.scenes.lock();
    scenes.retain(|s| s.id != id);
    store.write(&scenes)?;
    let mut autos = store.automations.lock();
    if autos.iter().any(|a| a.scene == id) {
        autos.retain(|a| a.scene != id);
        store.write_automations(&autos)?;
    }
    Ok(())
}

/// Saves an automation; an empty id makes a new one.
pub fn put_automation(app: &Ctx, mut a: Automation) -> Result<()> {
    if a.hour > 23 || a.minute > 59 || a.days.iter().any(|d| *d > 6) {
        return Err(Error::msg("That isn't a time of day"));
    }
    if a.scene != ALL_OFF && !store(app).list().iter().any(|s| s.id == a.scene) {
        return Err(Error::msg("Pick a scene to run"));
    }
    a.days.sort_unstable();
    a.days.dedup();
    if a.days.len() == 7 {
        a.days.clear();
    }
    if a.id.trim().is_empty() {
        a.id = format!("auto-{}", now_ms());
    }
    let store = store(app);
    let mut list = store.automations.lock();
    match list.iter_mut().find(|x| x.id == a.id) {
        Some(x) => *x = a,
        None => list.push(a),
    }
    store.write_automations(&list)
}

pub fn delete_automation(app: &Ctx, id: &str) -> Result<()> {
    let store = store(app);
    let mut list = store.automations.lock();
    list.retain(|a| a.id != id);
    store.write_automations(&list)
}

/// Takes one device to an action's state; its name when that failed.
async fn apply(app: &Ctx, d: &Device, a: &Action) -> Option<String> {
    // An IR air conditioner gets one code with everything, not one per change.
    if let (Some(remote), Some(cur)) = (local::ir_remote(&d.id), &d.ac) {
        if !a.on && d.on == Some(false) {
            return None;
        }
        let want = local::AcState {
            on: a.on,
            mode: a.ac.map_or(cur.mode, |x| x.mode),
            target: a.ac.map_or(cur.target, |x| x.target).round() as u8,
            fan: a.ac.and_then(|x| x.fan).or(cur.fan).unwrap_or(AcFan::Auto),
            sent_at: None,
        };
        return local::set_ac(app, remote, want, true).await.err().map(|_| d.name.clone());
    }
    for c in commands_for(d, a) {
        if control(app, &d.id, c).await.is_err() {
            return Some(d.name.clone());
        }
    }
    None
}

/// Runs a scene; devices are switched in parallel. Fails with the names that didn't change.
pub async fn run(app: &Ctx, id: &str) -> Result<()> {
    let devices = snapshot(app).devices;
    let actions = if id == ALL_OFF {
        devices
            .iter()
            .filter(|d| d.on.is_some() && d.kind != Kind::Sensor && d.members.is_empty())
            .map(|d| Action {
                device: d.id.clone(),
                on: false,
                brightness: None,
                color_temp: None,
                color: None,
                ac: None,
            })
            .collect()
    } else {
        store(app)
            .list()
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| Error::msg("That scene no longer exists"))?
            .actions
    };

    let jobs = actions
        .iter()
        .filter_map(|a| devices.iter().find(|d| d.id == a.device).map(|d| apply(app, d, a)));
    let failed: Vec<String> = futures_util::future::join_all(jobs).await.into_iter().flatten().collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(Error::msg(format!("Couldn't switch {}", failed.join(", "))))
    }
}

/// Runs each enabled automation's scene when its time comes. A time missed while the PC was
/// off isn't made up for.
pub async fn run_automations(app: Ctx) {
    // Automation and its schedule → when it runs next. An edit makes a new key.
    let mut next: HashMap<String, i64> = HashMap::new();
    loop {
        let now = now_ms();
        let enabled: Vec<Automation> = store(&app).automations().into_iter().filter(|a| a.enabled).collect();
        let key = |a: &Automation| format!("{}|{}:{}|{:?}", a.id, a.hour, a.minute, a.days);
        // Schedules of automations deleted or edited since are dropped.
        next.retain(|k, _| enabled.iter().any(|a| key(a) == *k));
        for a in enabled {
            let key = key(&a);
            let at = *next
                .entry(key.clone())
                .or_insert_with(|| wall::next_at(a.hour, a.minute, &a.days, None, now).unwrap_or(i64::MAX));
            if now < at {
                continue;
            }
            next.insert(key, wall::next_at(a.hour, a.minute, &a.days, None, now).unwrap_or(i64::MAX));
            let app = app.clone();
            tokio::spawn(async move {
                if let Err(e) = run(&app, &a.scene).await {
                    tracing::warn!(automation = a.id, "{e}");
                }
            });
        }
        tokio::time::sleep(TICK).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smarthome::{Ac, Provider};

    fn light(id: &str, on: bool, br: Option<u8>) -> Device {
        let mut d = Device::new(id.into(), Provider::Tuya, id.into(), Kind::Light);
        d.on = Some(on);
        d.brightness = br;
        d
    }

    fn act(device: &str, on: bool, brightness: Option<u8>) -> Action {
        Action { device: device.into(), on, brightness, color_temp: None, color: None, ac: None }
    }

    #[test]
    fn capture_skips_sensors_offline_and_groups() {
        let mut sensor = Device::new("s".into(), Provider::Tuya, "s".into(), Kind::Sensor);
        sensor.temperature = Some(24.0);
        let mut gone = light("gone", true, None);
        gone.online = false;
        let mut group = light("group:g", true, Some(50));
        group.members = vec!["a".into()];
        let acts = capture(&[light("a", true, Some(40)), light("b", false, Some(70)), sensor, gone, group]);
        assert_eq!(acts, vec![act("a", true, Some(40)), act("b", false, None)]);
    }

    #[test]
    fn capture_keeps_white_or_colour() {
        let mut d = light("a", true, Some(60));
        d.color_temp = Some(20);
        d.color = Some(Hs { h: 200, s: 90 });
        d.color_mode = Some(ColorMode::Color);
        let a = action_of(&d).unwrap();
        assert_eq!((a.color, a.color_temp), (Some(Hs { h: 200, s: 90 }), None));
        d.color_mode = Some(ColorMode::White);
        let a = action_of(&d).unwrap();
        assert_eq!((a.color, a.color_temp), (None, Some(20)));
    }

    #[test]
    fn commands_only_for_differences() {
        let a = act("a", true, Some(40));
        assert!(commands_for(&light("a", true, Some(40)), &a).is_empty());
        assert_eq!(commands_for(&light("a", true, Some(80)), &a), vec![Command::Brightness(40)]);
        assert_eq!(
            commands_for(&light("a", false, Some(80)), &a),
            vec![Command::Power(true), Command::Brightness(40)]
        );
        assert_eq!(commands_for(&light("a", true, Some(80)), &act("a", false, None)), vec![Command::Power(false)]);
        // A plain switch has no brightness to set.
        assert_eq!(commands_for(&light("a", false, None), &a), vec![Command::Power(true)]);
    }

    #[test]
    fn colour_then_brightness() {
        let mut d = light("a", true, Some(40));
        d.color = Some(Hs { h: 0, s: 100 });
        d.color_temp = Some(10);
        d.color_mode = Some(ColorMode::White);
        let a = Action { color: Some(Hs { h: 120, s: 100 }), ..act("a", true, Some(40)) };
        assert_eq!(commands_for(&d, &a), vec![Command::Color(Hs { h: 120, s: 100 }), Command::Brightness(40)]);
        let a = Action { color_temp: Some(10), ..act("a", true, Some(40)) };
        assert!(commands_for(&d, &a).is_empty());
    }

    #[test]
    fn ac_changes() {
        let mut d = Device::new("ac".into(), Provider::Tuya, "AC".into(), Kind::Ac);
        d.on = Some(true);
        d.ac = Some(Ac {
            mode: AcMode::Heat,
            target: 24.0,
            min: 16.0,
            max: 30.0,
            fan: Some(AcFan::Auto),
            modes: vec![],
            fans: vec![],
            ir: false,
            sent_at: None,
        });
        let a = Action { ac: Some(AcAction { mode: AcMode::Cool, target: 22.0, fan: None }), ..act("ac", true, None) };
        assert_eq!(commands_for(&d, &a), vec![Command::AcMode(AcMode::Cool), Command::AcTarget(22.0)]);
    }

    #[test]
    fn scenes_json_round_trip() {
        let s = Scene {
            id: "movie".into(),
            name: "Movie night".into(),
            icon: "movie".into(),
            theme: "purple".into(),
            room: Some("Living room".into()),
            actions: vec![act("tuya:x", false, None)],
        };
        let text = serde_json::to_string(&s).unwrap();
        assert!(!text.contains("brightness"));
        assert_eq!(serde_json::from_str::<Scene>(&text).unwrap(), s);
        // Scenes saved before themes and colours still load.
        let old: Scene = serde_json::from_str(r#"{"id":"a","name":"A","icon":"sun","actions":[{"device":"d","on":true,"brightness":5}]}"#).unwrap();
        assert_eq!(old.theme, "");
        assert_eq!(old.room, None);
        assert_eq!(old.actions[0].brightness, Some(5));
    }

    #[test]
    fn ids_from_names() {
        assert!(new_id("Movie night!").starts_with("movie-night-"));
        assert!(new_id("??").starts_with("scene-"));
    }
}
