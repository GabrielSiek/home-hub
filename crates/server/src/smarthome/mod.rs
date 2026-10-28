//! Smart home, on the server: Positivo (Tuya) and Sonoff devices driven on the home network with
//! their own keys (`local`), shown as one list of devices, plus scenes, automations, the
//! dashboard's layout and the rooms' tasks. From the old launcher's `smarthome/`; the server is
//! now the only machine that talks to the devices (Tuya devices take very few LAN clients).
//!
//! The old launcher's cloud paths (eWeLink and Tuya cloud accounts) aren't ported: every device
//! in the house answers on the LAN and no cloud keys were set. Their links read "not configured".
//!
//! Each provider keeps its own raw device state and derives `Device`s from it, so a command
//! updates that raw state right away (the tile flips at once) and puts it back if the device
//! refuses.

pub mod layout;
pub mod local;
pub mod rooms;
pub mod scenes;

use std::collections::HashSet;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tvl_kit::{Error, Result};

use crate::ctx::Ctx;

pub use layout::Layout;
pub use rooms::{Heard, Task};
pub use scenes::{Automation, Scene};
pub use tvl_protocol::{Screen, ServiceLink as Link};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Ewelink,
    Tuya,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Light,
    Strip,
    Switch,
    Plug,
    Fan,
    Ac,
    Sensor,
}

/// A color as hue (0–359) and saturation (0–100); brightness is separate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hs {
    pub h: u16,
    pub s: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    White,
    Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AcMode {
    Cool,
    Heat,
    Auto,
    Fan,
    Dry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AcFan {
    Auto,
    Low,
    Mid,
    High,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Ac {
    pub mode: AcMode,
    /// Target temperature, °C.
    pub target: f32,
    pub min: f32,
    pub max: f32,
    pub fan: Option<AcFan>,
    /// Modes this unit offers.
    pub modes: Vec<AcMode>,
    /// Fan speeds this unit offers; empty when the fan can't be set.
    pub fans: Vec<AcFan>,
    /// Driven over infrared: the state is the last one sent, not read back.
    pub ir: bool,
    /// When the last IR command went out, unix ms.
    pub sent_at: Option<i64>,
}

/// One controllable thing on screen. A capability is present when its field is `Some`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Device {
    /// "local:tuya:<id>", "local:ir:<remote>", "local:ewelink:<deviceid>[:<channel>]",
    /// "group:<id>", or for the cloud "ewelink:<deviceid>[:<channel>]" / "tuya:<id>[:<code>]".
    pub id: String,
    pub provider: Provider,
    pub name: String,
    pub room: Option<String>,
    pub kind: Kind,
    pub online: bool,
    pub on: Option<bool>,
    /// 1–100 %.
    pub brightness: Option<u8>,
    /// 0 (warm) – 100 (cool).
    pub color_temp: Option<u8>,
    pub color: Option<Hs>,
    /// Which of white or color a light that does both is showing.
    pub color_mode: Option<ColorMode>,
    /// °C.
    pub temperature: Option<f32>,
    /// %.
    pub humidity: Option<f32>,
    /// Watts.
    pub power_w: Option<f32>,
    /// 1..=fan_speeds; with `fan_speeds` set the fan's speed can be picked.
    pub fan_speed: Option<u8>,
    pub fan_speeds: Option<u8>,
    pub ac: Option<Ac>,
    /// What it is and how it's reached, e.g. "Positivo bulb · Tuya local".
    pub via: Option<String>,
    /// A group's devices.
    pub members: Vec<String>,
}

impl Device {
    pub fn new(id: String, provider: Provider, name: String, kind: Kind) -> Self {
        Self {
            id,
            provider,
            name,
            room: None,
            kind,
            online: true,
            on: None,
            brightness: None,
            color_temp: None,
            color: None,
            color_mode: None,
            temperature: None,
            humidity: None,
            power_w: None,
            fan_speed: None,
            fan_speeds: None,
            ac: None,
            via: None,
            members: Vec::new(),
        }
    }
}

/// A change asked for from the UI or a scene.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Command {
    Power(bool),
    Brightness(u8),
    ColorTemp(u8),
    Color(Hs),
    FanSpeed(u8),
    AcTarget(f32),
    AcMode(AcMode),
    AcFan(AcFan),
}

/// Everything the Smart Home screen shows.
#[derive(Debug, Clone, Serialize)]
pub struct Home {
    pub devices: Vec<Device>,
    pub scenes: Vec<Scene>,
    pub automations: Vec<Automation>,
    pub layout: Layout,
    pub tasks: Vec<Task>,
    /// The TV PCs and the phones and tablets that opened the companion, each in its room.
    pub screens: Vec<Screen>,
    pub heard: Option<Heard>,
    pub local: Link,
    /// The cloud accounts, not ported: always "not configured".
    pub ewelink: Link,
    pub tuya: Link,
}

pub struct SmartHome {
    pub local: local::State,
    pub scenes: scenes::Store,
    pub layout: layout::Store,
    pub rooms: rooms::Store,
    /// TV PCs with their Smart Home screen open: poll the devices often while there are any (or a
    /// companion page is open, which shows the devices too).
    watchers: Mutex<HashSet<String>>,
}

impl SmartHome {
    pub fn new(data_dir: &std::path::Path) -> Self {
        Self {
            local: local::State::new(data_dir),
            scenes: scenes::Store::load(data_dir),
            layout: layout::Store::load(data_dir),
            rooms: rooms::Store::load(data_dir),
            watchers: Mutex::default(),
        }
    }
}

/// Someone is looking at the devices: a TV PC's Smart Home screen or a companion page.
pub fn watching(app: &Ctx) -> bool {
    !app.smarthome.watchers.lock().is_empty() || app.devices.any_page_open()
}

pub fn snapshot(app: &Ctx) -> Home {
    let sh = &app.smarthome;
    let layout = sh.layout.get();
    Home {
        devices: layout::apply(&layout, sh.local.devices()),
        scenes: sh.scenes.list(),
        automations: sh.scenes.automations(),
        layout,
        tasks: sh.rooms.tasks(),
        screens: app.devices.list(),
        heard: sh.rooms.heard(),
        local: sh.local.link(),
        ewelink: Link::default(),
        tuya: Link::default(),
    }
}

/// Sends the whole picture to every screen; called after anything changes.
pub fn emit(app: &Ctx) {
    app.bus.emit("smarthome", &snapshot(app));
}

/// Starts the devices' polling and the automations' clock. In shadow mode (next to the old
/// launcher) neither runs: the old launcher still drives the devices.
pub fn start(app: &Ctx) {
    // The screens are part of the picture (each in its room): when the house's screens change,
    // every screen gets the picture again.
    let ctx = app.clone();
    tvl_kit::task::spawn_supervised("smart home screens", app.shutdown.clone(), move || {
        let ctx = ctx.clone();
        async move {
            let mut rx = ctx.bus.subscribe();
            loop {
                match rx.recv().await {
                    Ok(e) if &*e.event == "devices" => emit(&ctx),
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
        }
    });
    if app.config.shadow {
        tracing::info!("shadow mode: the smart home is left to the old launcher");
        return;
    }
    if !app.config.force_lan && old_launcher_running(app) {
        tracing::warn!(
            "the old launcher answers on this PC (port 47960): the smart home is left to it, since the devices take very \
             few LAN clients. Close it, or start with --force-lan"
        );
        return;
    }
    local::restart(app);
    let ctx = app.clone();
    tvl_kit::task::spawn_supervised("automations", app.shutdown.clone(), move || scenes::run_automations(ctx.clone()));
}

/// The old launcher's companion answers on this PC while this server listens elsewhere (side by
/// side on the dev PC).
fn old_launcher_running(app: &Ctx) -> bool {
    const OLD_PORT: u16 = 47960;
    if app.config.http_port == OLD_PORT {
        return false;
    }
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], OLD_PORT));
    std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(300)).is_ok()
}

/// TV PC `screen`'s Smart Home screen opened or closed. Opening it fetches the devices at once.
pub fn watch(app: &Ctx, screen: &str, active: bool) {
    let was = watching(app);
    {
        let mut w = app.smarthome.watchers.lock();
        if active {
            w.insert(screen.to_string());
        } else {
            w.remove(screen);
        }
    }
    if active && !was {
        wake(app);
    }
}

/// A companion page opened while nothing watched: fetch the devices at once.
pub fn page_opened(app: &Ctx) {
    if !app.smarthome.watchers.lock().is_empty() {
        return;
    }
    wake(app);
}

/// A TV PC went away: its Smart Home screen no longer counts.
pub fn forget_watcher(app: &Ctx, screen: &str) {
    app.smarthome.watchers.lock().remove(screen);
}

fn wake(app: &Ctx) {
    app.smarthome.local.wake();
}

/// Fetches every device again (the Refresh button).
pub fn refresh(app: &Ctx) {
    app.smarthome.local.wake();
}

pub async fn control(app: &Ctx, id: &str, cmd: Command) -> Result<()> {
    crate::hub::record_use(app, id);
    if id.starts_with(layout::GROUP_PREFIX) {
        control_group(app, id, cmd).await
    } else {
        control_one(app, id, cmd).await
    }
}

/// A room's All lights: `cmd` to every light in `room` that answers and can take it. Lights
/// that are off come on for a brightness or a warmth. Fails with the names that didn't change.
pub async fn control_room(app: &Ctx, room: &str, cmd: Command) -> Result<()> {
    let devices = snapshot(app).devices;
    let lights: Vec<&Device> = rooms::lights_in(&devices, room).into_iter().filter(|d| d.online && supports(d, cmd)).collect();
    if lights.is_empty() {
        return Err(Error::msg(format!("No lights in the {room} answer")));
    }
    let lights_up = !matches!(cmd, Command::Power(_));
    let jobs = lights.iter().map(|d| async move {
        if lights_up && d.on == Some(false) && control(app, &d.id, Command::Power(true)).await.is_err() {
            return Some(d.name.clone());
        }
        control(app, &d.id, cmd).await.err().map(|_| d.name.clone())
    });
    let failed: Vec<String> = futures_util::future::join_all(jobs).await.into_iter().flatten().collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(Error::msg(format!("Couldn't change {}", failed.join(", "))))
    }
}

/// Whether `d` can carry out `cmd`, so a group only sends it to the members that can.
fn supports(d: &Device, cmd: Command) -> bool {
    match cmd {
        Command::Power(_) => d.on.is_some(),
        Command::Brightness(_) => d.brightness.is_some(),
        Command::ColorTemp(_) => d.color_temp.is_some(),
        Command::Color(_) => d.color.is_some(),
        Command::FanSpeed(_) => d.fan_speeds.is_some(),
        Command::AcTarget(_) | Command::AcMode(_) | Command::AcFan(_) => d.ac.is_some(),
    }
}

/// Sends `cmd` to every member of a group at once. Fails with the names that didn't change.
async fn control_group(app: &Ctx, id: &str, cmd: Command) -> Result<()> {
    let layout = app.smarthome.layout.get();
    let members = layout::members(&layout, id).ok_or_else(|| Error::msg("That group no longer exists"))?;
    let devices = snapshot(app).devices;
    let jobs = members.iter().filter_map(|m| {
        let d = devices.iter().find(|d| &d.id == m && d.online && supports(d, cmd))?;
        Some(async move { control_one(app, &d.id, cmd).await.err().map(|_| d.name.clone()) })
    });
    let failed: Vec<String> = futures_util::future::join_all(jobs).await.into_iter().flatten().collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(Error::msg(format!("Couldn't change {}", failed.join(", "))))
    }
}

async fn control_one(app: &Ctx, id: &str, cmd: Command) -> Result<()> {
    if id.starts_with(local::PREFIX) {
        local::control(app, id, cmd).await
    } else {
        Err(Error::msg(format!("unknown device '{id}'")))
    }
}

// ---------- Conversions shared by both providers ----------

/// Maps `v` from `lo..=hi` to a percentage (1–100 when `v` is above `lo`).
pub(crate) fn to_pct(v: f64, lo: f64, hi: f64) -> u8 {
    if hi <= lo {
        return 100;
    }
    (((v - lo) / (hi - lo)) * 100.0).round().clamp(0.0, 100.0) as u8
}

/// Maps a percentage back into `lo..=hi`.
pub(crate) fn from_pct(pct: u8, lo: f64, hi: f64) -> f64 {
    (lo + (hi - lo) * f64::from(pct.min(100)) / 100.0).round()
}

/// A JSON number or numeric string as f64 ("23.5", 235, "unavailable" → None).
pub(crate) fn num(v: Option<&serde_json::Value>) -> Option<f64> {
    match v? {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_round_trip() {
        assert_eq!(to_pct(10.0, 10.0, 1000.0), 0);
        assert_eq!(to_pct(1000.0, 10.0, 1000.0), 100);
        assert_eq!(to_pct(505.0, 10.0, 1000.0), 50);
        assert_eq!(from_pct(50, 10.0, 1000.0), 505.0);
        assert_eq!(from_pct(100, 25.0, 255.0), 255.0);
    }

    #[test]
    fn command_json_shape() {
        let c: Command = serde_json::from_str(r#"{"type":"power","value":true}"#).unwrap();
        assert_eq!(c, Command::Power(true));
        let c: Command = serde_json::from_str(r#"{"type":"color","value":{"h":200,"s":80}}"#).unwrap();
        assert_eq!(c, Command::Color(Hs { h: 200, s: 80 }));
        let c: Command = serde_json::from_str(r#"{"type":"ac_mode","value":"cool"}"#).unwrap();
        assert_eq!(c, Command::AcMode(AcMode::Cool));
    }

    #[test]
    fn numbers_from_json() {
        assert_eq!(num(Some(&serde_json::json!("23.5"))), Some(23.5));
        assert_eq!(num(Some(&serde_json::json!(40))), Some(40.0));
        assert_eq!(num(Some(&serde_json::json!("unavailable"))), None);
        assert_eq!(num(None), None);
    }
}
