//! Devices on the home network, driven with their own keys and no cloud: Positivo (Tuya) bulbs
//! and IR hubs over the Tuya LAN protocol, Sonoff switches in eWeLink's LAN mode. The keys and
//! the IR code sets were taken from the vendors' apps once, into smarthome-local.json.
//!
//! An air conditioner behind an IR hub can't report back, so its state is the last one sent,
//! kept in smarthome-ir.json.

pub mod ewelink_lan;
pub mod tuya_lan;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use parking_lot::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio::sync::Notify;

use super::{emit, from_pct, num, to_pct, Ac, AcFan, AcMode, ColorMode, Command, Device, Hs, Kind, Link, Provider};
use tvl_kit::{Error, Result};
use tvl_kit::time::now_ms;

use crate::ctx::Ctx;

const FILE: &str = "smarthome-local.json";
const IR_FILE: &str = "smarthome-ir.json";
pub const PREFIX: &str = "local:";
const POLL_WATCHING: Duration = Duration::from_secs(8);
const POLL_IDLE: Duration = Duration::from_secs(60);
/// Look again for devices that moved to another address at most this often.
const DISCOVER_EVERY: Duration = Duration::from_secs(120);
/// How long to listen for the bulbs' broadcasts, and for the Sonoffs' mDNS answers.
const TUYA_LISTEN: Duration = Duration::from_secs(6);
const MDNS_LISTEN: Duration = Duration::from_secs(3);
/// A bulb or hub is offline after this many calls in a row went unanswered: the older ones
/// doze on Wi-Fi and take one client at a time, so a single miss means little.
const OFFLINE_AFTER: u8 = 3;
/// A Sonoff that hasn't answered mDNS for this long is offline.
const EWE_STALE: Duration = Duration::from_secs(180);
/// Between the "power on" code and the one with the mode and temperature.
const AC_SETTLE: Duration = Duration::from_millis(900);
/// Before trying an IR code again on a hub that dropped the connection.
const IR_RETRY_AFTER: Duration = Duration::from_millis(600);
/// Tuya's IR library numbers an AC's modes M0…M4 and its fan speeds S0…S3 in this order.
const AC_MODES: [AcMode; 5] = [AcMode::Cool, AcMode::Heat, AcMode::Auto, AcMode::Fan, AcMode::Dry];
const AC_FANS: [AcFan; 4] = [AcFan::Auto, AcFan::Low, AcFan::Mid, AcFan::High];
const SONOFF_PORT: u16 = 8081;

// ---------- The stored keys and codes ----------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Store {
    tuya: Vec<TuyaEntry>,
    ir: Vec<Hub>,
    ewelink: Vec<EweEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TuyaEntry {
    id: String,
    name: String,
    key: String,
    ip: String,
    version: String,
    #[serde(default)]
    mac: Option<String>,
    #[serde(default)]
    category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Hub {
    id: String,
    name: String,
    key: String,
    ip: String,
    version: String,
    #[serde(default)]
    remotes: Vec<Remote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Remote {
    id: String,
    name: String,
    /// "ac", "tv" or "remote".
    kind: String,
    #[serde(default)]
    brand: Option<String>,
    /// Button name ("power on", "M0_T22_S0") → its IR code.
    #[serde(default)]
    codes: BTreeMap<String, Code>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Code {
    head: String,
    pulse: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EweEntry {
    id: String,
    name: String,
    key: String,
    uiid: u32,
    #[serde(default)]
    model: Option<String>,
    #[serde(default = "one")]
    channels: u8,
    /// Where it was last found; mDNS finds it again when it moves.
    #[serde(default)]
    ip: Option<String>,
}

fn one() -> u8 {
    1
}

/// What an IR air conditioner was last told.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AcState {
    pub on: bool,
    pub mode: AcMode,
    pub target: u8,
    pub fan: AcFan,
    #[serde(default)]
    pub sent_at: Option<i64>,
}

impl Default for AcState {
    fn default() -> Self {
        Self {
            on: false,
            mode: AcMode::Cool,
            target: 22,
            fan: AcFan::Auto,
            sent_at: None,
        }
    }
}

struct Seen {
    at: Instant,
    ip: String,
    port: u16,
    params: Value,
}

#[derive(Default)]
struct Inner {
    store: Store,
    link: Link,
    /// Bulb id → its data points as last read.
    dps: HashMap<String, Map<String, Value>>,
    /// Bulb or hub id → calls in a row it didn't answer. Unknown counts as online.
    misses: HashMap<String, u8>,
    ewe: HashMap<String, Seen>,
    acs: HashMap<String, AcState>,
    discovered: Option<Instant>,
}

impl Inner {
    fn online(&self, id: &str) -> bool {
        self.misses.get(id).is_none_or(|m| *m < OFFLINE_AFTER)
    }

    /// Counts a call to `id` that worked or didn't.
    fn answered(&mut self, id: &str, ok: bool) {
        let m = self.misses.entry(id.to_string()).or_default();
        *m = if ok { 0 } else { m.saturating_add(1) };
    }
}

pub struct State {
    path: PathBuf,
    ir_path: PathBuf,
    inner: Mutex<Inner>,
    wake: Notify,
    gen: AtomicU64,
    /// One call at a time per device: the older hubs take a single client.
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl State {
    pub fn new(data_dir: &Path) -> Self {
        let path = data_dir.join(FILE);
        let ir_path = data_dir.join(IR_FILE);
        let store = tvl_kit::store::load(&path).unwrap_or_default();
        let acs = tvl_kit::store::load(&ir_path).unwrap_or_default();
        Self {
            path,
            ir_path,
            inner: Mutex::new(Inner {
                store,
                acs,
                ..Default::default()
            }),
            wake: Notify::new(),
            gen: AtomicU64::new(0),
            locks: Mutex::new(HashMap::new()),
        }
    }

    pub fn configured(&self) -> bool {
        let inner = self.inner.lock();
        let s = &inner.store;
        !(s.tuya.is_empty() && s.ir.is_empty() && s.ewelink.is_empty())
    }

    pub fn link(&self) -> Link {
        self.inner.lock().link.clone()
    }

    /// The vendors' own ids of every device here, so the cloud copies of them can be left out.
    pub fn vendor_ids(&self) -> Vec<String> {
        let inner = self.inner.lock();
        let s = &inner.store;
        s.tuya
            .iter()
            .map(|t| t.id.clone())
            .chain(s.ir.iter().flat_map(|h| h.remotes.iter().map(|r| r.id.clone())))
            .chain(s.ewelink.iter().map(|e| e.id.clone()))
            .collect()
    }

    pub fn devices(&self) -> Vec<Device> {
        let inner = self.inner.lock();
        let mut out = Vec::new();
        for t in &inner.store.tuya {
            out.push(bulb_device(t, inner.dps.get(&t.id), inner.online(&t.id)));
        }
        for h in &inner.store.ir {
            let ok = inner.online(&h.id);
            // A remote without codes can't do anything yet.
            for r in h.remotes.iter().filter(|r| r.kind == "ac" && !r.codes.is_empty()) {
                out.push(ac_device(h, r, inner.acs.get(&r.id).copied().unwrap_or_default(), ok));
            }
        }
        for e in &inner.store.ewelink {
            out.extend(ewe_devices(e, inner.ewe.get(&e.id)));
        }
        out
    }

    /// Poll now instead of waiting.
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    fn lock_for(&self, id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.locks.lock().entry(id.to_string()).or_default().clone()
    }

    fn save_store(&self, store: &Store) {
        if let Err(e) = tvl_kit::store::save(&self.path, store, true) {
            tracing::error!("can't save the local devices: {e}");
        }
    }

    fn save_acs(&self, acs: &HashMap<String, AcState>) {
        if let Err(e) = tvl_kit::store::save(&self.ir_path, acs, true) {
            tracing::error!("can't save the air conditioners' last state: {e}");
        }
    }
}

fn state(app: &Ctx) -> &State {
    &app.smarthome.local
}

/// Starts (or restarts) the polling task.
pub fn restart(app: &Ctx) {
    let gen = state(app).gen.fetch_add(1, Ordering::SeqCst) + 1;
    let ctx = app.clone();
    tvl_kit::task::spawn_supervised("smart home LAN", app.shutdown.clone(), move || run(ctx.clone(), gen));
}

fn alive(app: &Ctx, gen: u64) -> bool {
    state(app).gen.load(Ordering::SeqCst) == gen
}

async fn run(app: Ctx, gen: u64) {
    loop {
        if !alive(&app, gen) {
            return;
        }
        if !state(&app).configured() {
            state(&app).inner.lock().link = Link::default();
            emit(&app);
            return;
        }
        let before = (state(&app).devices(), state(&app).link());
        let link = poll(&app).await;
        if !alive(&app, gen) {
            return;
        }
        state(&app).inner.lock().link = link;
        // Only a change goes out: every screen gets the whole picture, and an old iPad redraws it.
        if (state(&app).devices(), state(&app).link()) != before {
            emit(&app);
        }
        let wait = if super::watching(&app) { POLL_WATCHING } else { POLL_IDLE };
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = state(&app).wake.notified() => {}
        }
    }
}

fn tuya_target(id: &str, ip: &str, key: &str, version: &str) -> tuya_lan::Target {
    tuya_lan::Target {
        id: id.to_string(),
        ip: ip.to_string(),
        key: key.to_string(),
        version: if version.starts_with("3.5") { tuya_lan::Version::V35 } else { tuya_lan::Version::V33 },
    }
}

/// Reads every bulb and hub, asks the Sonoffs over mDNS, and looks for moved devices when one
/// didn't answer.
async fn poll(app: &Ctx) -> Link {
    let st = state(app);
    let store = st.inner.lock().store.clone();

    let bulbs = store.tuya.iter().map(|t| async move {
        let lock = st.lock_for(&t.id);
        let _held = lock.lock().await;
        (t.id.clone(), tuya_lan::status(&tuya_target(&t.id, &t.ip, &t.key, &t.version)).await)
    });
    let hubs = store.ir.iter().map(|h| async move {
        let lock = st.lock_for(&h.id);
        let _held = lock.lock().await;
        (h.id.clone(), tuya_lan::status(&tuya_target(&h.id, &h.ip, &h.key, &h.version)).await)
    });
    let sonoffs = async {
        if store.ewelink.is_empty() {
            return Ok(Vec::new());
        }
        ewelink_lan::discover(MDNS_LISTEN).await
    };
    let (bulbs, hubs, found) = tokio::join!(
        futures_util::future::join_all(bulbs),
        futures_util::future::join_all(hubs),
        sonoffs
    );

    let mut answered = 0;
    let mut missing = false;
    {
        let mut inner = st.inner.lock();
        for (id, r) in bulbs {
            let ok = r.is_ok();
            answered += usize::from(ok);
            missing |= !ok;
            if let Ok(dps) = r {
                // A reply can carry only some data points; keep the rest.
                inner.dps.entry(id.clone()).or_default().extend(dps);
            }
            inner.answered(&id, ok);
        }
        for (id, r) in hubs {
            answered += usize::from(r.is_ok());
            missing |= r.is_err();
            inner.answered(&id, r.is_ok());
        }
        if let Ok(found) = &found {
            let keys: HashMap<String, String> = inner.store.ewelink.iter().map(|e| (e.id.clone(), e.key.clone())).collect();
            for f in found {
                let Some(key) = keys.get(&f.deviceid) else { continue };
                let params = ewelink_lan::decrypt_params(key, f).unwrap_or(Value::Null);
                answered += 1;
                inner.ewe.insert(
                    f.deviceid.clone(),
                    Seen {
                        at: Instant::now(),
                        ip: f.ip.clone(),
                        port: f.port,
                        params,
                    },
                );
            }
            // Remember where each was found, for the first command after a restart.
            let mut moved = false;
            for e in &mut inner.store.ewelink {
                if let Some(s) = found.iter().find(|f| f.deviceid == e.id) {
                    if e.ip.as_deref() != Some(s.ip.as_str()) {
                        e.ip = Some(s.ip.clone());
                        moved = true;
                    }
                }
            }
            if moved {
                st.save_store(&inner.store);
            }
        }
    }

    let due = st.inner.lock().discovered.is_none_or(|t| t.elapsed() > DISCOVER_EVERY);
    if missing && due {
        st.inner.lock().discovered = Some(Instant::now());
        if let Ok(seen) = tuya_lan::discover(TUYA_LISTEN).await {
            let mut inner = st.inner.lock();
            let mut moved = false;
            for s in &seen {
                // A firmware update can change the protocol version, too.
                for t in inner.store.tuya.iter_mut().filter(|t| t.id == s.id && (t.ip != s.ip || t.version != s.version)) {
                    t.ip = s.ip.clone();
                    t.version = s.version.clone();
                    moved = true;
                }
                for h in inner.store.ir.iter_mut().filter(|h| h.id == s.id && h.ip != s.ip) {
                    h.ip = s.ip.clone();
                    moved = true;
                }
            }
            if moved {
                st.save_store(&inner.store);
                st.wake();
            }
        }
    }

    Link {
        configured: true,
        signed_in: true,
        connected: answered > 0,
        error: (answered == 0).then(|| "No device answered on the home network".to_string()),
    }
}

// ---------- Bulbs ----------

/// Tuya's colour data point: hue 0–360, saturation and value 0–1000, as 4 hex digits each.
fn parse_hsv(s: &str) -> Option<(Hs, u8)> {
    if s.len() != 12 {
        return None;
    }
    let part = |i: usize| u16::from_str_radix(s.get(i..i + 4)?, 16).ok();
    let (h, sat, v) = (part(0)?, part(4)?, part(8)?);
    Some((
        Hs {
            h: h % 360,
            s: (sat.min(1000) / 10) as u8,
        },
        to_pct(f64::from(v), 0.0, 1000.0).max(1),
    ))
}

fn hsv_hex(c: Hs, v_pct: u8) -> String {
    let v = (u16::from(v_pct.clamp(1, 100)) * 10).max(10);
    format!("{:04x}{:04x}{:04x}", c.h % 360, u16::from(c.s.min(100)) * 10, v)
}

fn bulb_device(t: &TuyaEntry, dps: Option<&Map<String, Value>>, answered: bool) -> Device {
    let mut d = Device::new(format!("{PREFIX}tuya:{}", t.id), Provider::Tuya, t.name.clone(), Kind::Light);
    d.via = Some("Positivo bulb · Tuya local".into());
    d.online = answered;
    d.on = Some(false);
    let Some(dps) = dps else { return d };
    d.on = dps.get("20").and_then(Value::as_bool).or(Some(false));
    let colour = dps.get("21").and_then(Value::as_str) == Some("colour");
    let white = num(dps.get("22")).map(|v| to_pct(v, 10.0, 1000.0).max(1));
    d.color_temp = num(dps.get("23")).map(|v| to_pct(v, 0.0, 1000.0));
    let hsv = dps.get("24").and_then(Value::as_str).and_then(parse_hsv);
    if let Some((c, v)) = hsv {
        d.color = Some(c);
        d.color_mode = Some(if colour { ColorMode::Color } else { ColorMode::White });
        d.brightness = if colour { Some(v) } else { white };
    } else {
        d.brightness = white;
    }
    d
}

/// The data points that carry out `cmd` on a bulb showing `dps`. Anything but switching off
/// also switches it on.
fn bulb_dps(dps: Option<&Map<String, Value>>, cmd: Command) -> Result<Map<String, Value>> {
    let colour = dps.and_then(|m| m.get("21")).and_then(Value::as_str) == Some("colour");
    let hsv = dps.and_then(|m| m.get("24")).and_then(Value::as_str).and_then(parse_hsv);
    let white = dps.and_then(|m| num(m.get("22"))).map(|v| to_pct(v, 10.0, 1000.0).max(1));
    let mut out = Map::new();
    match cmd {
        Command::Power(on) => {
            out.insert("20".into(), json!(on));
            return Ok(out);
        }
        Command::Brightness(p) if colour => {
            let c = hsv.map_or(Hs { h: 30, s: 100 }, |(c, _)| c);
            out.insert("24".into(), json!(hsv_hex(c, p)));
        }
        Command::Brightness(p) => {
            out.insert("22".into(), json!(from_pct(p.max(1), 10.0, 1000.0) as i64));
        }
        Command::ColorTemp(p) => {
            out.insert("21".into(), json!("white"));
            out.insert("23".into(), json!(from_pct(p, 0.0, 1000.0) as i64));
        }
        Command::Color(c) => {
            // Keep the brightness it had, in white or in colour.
            let v = if colour { hsv.map(|(_, v)| v) } else { white };
            out.insert("21".into(), json!("colour"));
            out.insert("24".into(), json!(hsv_hex(c, v.unwrap_or(100))));
        }
        _ => return Err(Error::msg("A lamp can't do that")),
    }
    out.insert("20".into(), json!(true));
    Ok(out)
}

async fn control_bulb(app: &Ctx, id: &str, cmd: Command) -> Result<()> {
    let st = state(app);
    let (t, dps) = {
        let inner = st.inner.lock();
        let t = inner.store.tuya.iter().find(|t| t.id == id).cloned();
        (t, inner.dps.get(id).cloned())
    };
    let t = t.ok_or_else(|| Error::msg("That lamp is no longer set up"))?;
    let set = bulb_dps(dps.as_ref(), cmd)?;
    let lock = st.lock_for(id);
    let held = lock.lock().await;
    let r = tuya_lan::set(&tuya_target(&t.id, &t.ip, &t.key, &t.version), set.clone()).await;
    drop(held);
    let mut inner = st.inner.lock();
    inner.answered(id, r.is_ok());
    if r.is_ok() {
        inner.dps.entry(id.to_string()).or_default().extend(set);
    }
    drop(inner);
    emit(app);
    r.map_err(|e| Error::msg(format!("{}: {e}", t.name)))
}

// ---------- IR air conditioners ----------

fn mode_index(m: AcMode) -> usize {
    AC_MODES.iter().position(|x| *x == m).unwrap_or(0)
}

fn fan_index(f: AcFan) -> usize {
    AC_FANS.iter().position(|x| *x == f).unwrap_or(0)
}

/// The temperatures the remote has codes for in `mode`; none for modes without one (fan, dry…).
fn temp_range(r: &Remote, mode: usize) -> Option<(u8, u8)> {
    let prefix = format!("M{mode}_T");
    let temps = r
        .codes
        .keys()
        .filter_map(|k| k.strip_prefix(&prefix)?.split('_').next()?.parse::<u8>().ok());
    temps.fold(None, |acc, t| Some(acc.map_or((t, t), |(lo, hi): (u8, u8)| (lo.min(t), hi.max(t)))))
}

/// The button for a state: its mode, temperature and fan speed, or the nearest fan speed the
/// remote has for that mode (fan-only has no "auto", dry often only "auto").
fn state_key(r: &Remote, s: &AcState) -> Option<String> {
    let m = mode_index(s.mode);
    let want = fan_index(s.fan) as i32;
    let mut fans: Vec<i32> = (0..AC_FANS.len() as i32).collect();
    fans.sort_by_key(|f| (f - want).abs());
    let t = temp_range(r, m).map(|(lo, hi)| s.target.clamp(lo, hi));
    fans.into_iter().find_map(|f| {
        let key = match t {
            Some(t) => format!("M{m}_T{t}_S{f}"),
            None => format!("M{m}_S{f}"),
        };
        r.codes.contains_key(&key).then_some(key)
    })
}

fn ac_device(h: &Hub, r: &Remote, s: AcState, hub_ok: bool) -> Device {
    let mut d = Device::new(format!("{PREFIX}ir:{}", r.id), Provider::Tuya, r.name.clone(), Kind::Ac);
    d.via = Some(format!("{} · IR", h.name));
    d.online = hub_ok;
    d.on = Some(s.on);
    let modes: Vec<AcMode> = AC_MODES
        .iter()
        .enumerate()
        .filter(|(i, _)| r.codes.keys().any(|k| k.starts_with(&format!("M{i}_"))))
        .map(|(_, m)| *m)
        .collect();
    let fans: Vec<AcFan> = AC_FANS
        .iter()
        .enumerate()
        .filter(|(i, _)| r.codes.keys().any(|k| k.ends_with(&format!("_S{i}"))))
        .map(|(_, f)| *f)
        .collect();
    let (min, max) = temp_range(r, 0).unwrap_or((16, 30));
    d.ac = Some(Ac {
        mode: s.mode,
        target: f32::from(s.target),
        min: f32::from(min),
        max: f32::from(max),
        fan: Some(s.fan),
        modes,
        fans,
        ir: true,
        sent_at: s.sent_at,
    });
    d
}

fn find_remote(store: &Store, id: &str) -> Option<(Hub, Remote)> {
    store
        .ir
        .iter()
        .find_map(|h| h.remotes.iter().find(|r| r.id == id).map(|r| (h.clone(), r.clone())))
}

async fn send_ir(app: &Ctx, hub: &Hub, code: &Code) -> Result<()> {
    let mut dps = Map::new();
    dps.insert("1".into(), json!("send_ir"));
    dps.insert("3".into(), json!(code.head));
    dps.insert("4".into(), json!(code.pulse));
    dps.insert("10".into(), json!(300));
    dps.insert("13".into(), json!(0));
    let lock = state(app).lock_for(&hub.id);
    let _held = lock.lock().await;
    let target = tuya_target(&hub.id, &hub.ip, &hub.key, &hub.version);
    // The older hubs drop a connection now and then. Once more is harmless: every code is a
    // whole state (or "power on"/"power off"), not a toggle.
    match tuya_lan::set(&target, dps.clone()).await {
        Err(_) => {
            tokio::time::sleep(IR_RETRY_AFTER).await;
            tuya_lan::set(&target, dps).await
        }
        ok => ok,
    }
}

/// Takes the AC to `want`: "power off", or "power on" (when it was off, or asked to switch on)
/// and then the code for the mode, temperature and fan.
pub async fn set_ac(app: &Ctx, remote_id: &str, mut want: AcState, power_asked: bool) -> Result<()> {
    let st = state(app);
    let (hub, remote, was) = {
        let inner = st.inner.lock();
        let (h, r) = find_remote(&inner.store, remote_id).ok_or_else(|| Error::msg("That remote is no longer set up"))?;
        let was = inner.acs.get(remote_id).copied().unwrap_or_default();
        (h, r, was)
    };
    if let Some((lo, hi)) = temp_range(&remote, mode_index(want.mode)) {
        want.target = want.target.clamp(lo, hi);
    }
    let mut keys: Vec<String> = Vec::new();
    if !want.on {
        if was.on || power_asked {
            keys.push("power off".into());
        }
    } else {
        if (!was.on || power_asked) && remote.codes.contains_key("power on") {
            keys.push("power on".into());
        }
        keys.push(state_key(&remote, &want).ok_or_else(|| Error::msg("The remote has no code for that setting"))?);
    }
    for (i, k) in keys.iter().enumerate() {
        let code = remote.codes.get(k).ok_or_else(|| Error::msg(format!("The remote has no \"{k}\" button")))?;
        if i > 0 {
            tokio::time::sleep(AC_SETTLE).await;
        }
        if let Err(e) = send_ir(app, &hub, code).await {
            st.inner.lock().answered(&hub.id, false);
            emit(app);
            return Err(Error::msg(format!("{}: {e}", hub.name)));
        }
    }
    let mut inner = st.inner.lock();
    inner.answered(&hub.id, true);
    if !keys.is_empty() {
        want.sent_at = Some(now_ms());
    } else {
        want.sent_at = was.sent_at;
    }
    inner.acs.insert(remote_id.to_string(), want);
    st.save_acs(&inner.acs);
    drop(inner);
    emit(app);
    Ok(())
}

async fn control_ac(app: &Ctx, remote_id: &str, cmd: Command) -> Result<()> {
    let mut s = state(app).inner.lock().acs.get(remote_id).copied().unwrap_or_default();
    let mut power = false;
    match cmd {
        Command::Power(on) => {
            s.on = on;
            power = true;
        }
        // A new temperature switches it on; mode and fan are kept for later while it's off.
        Command::AcTarget(t) => {
            s.target = t.round().clamp(0.0, 40.0) as u8;
            s.on = true;
        }
        Command::AcMode(m) => s.mode = m,
        Command::AcFan(f) => s.fan = f,
        _ => return Err(Error::msg("An air conditioner can't do that")),
    }
    set_ac(app, remote_id, s, power).await
}

// ---------- Sonoff ----------

fn ewe_kind(uiid: u32) -> Kind {
    match uiid {
        22 | 33 | 59 | 137 | 173 => Kind::Strip,
        _ => Kind::Switch,
    }
}

fn ewe_via(e: &EweEntry) -> String {
    let model = match e.model.as_deref() {
        Some(m) if m.contains("MSW") => "SwitchMan M5",
        _ if ewe_kind(e.uiid) == Kind::Strip => "LED strip",
        _ => "switch",
    };
    format!("Sonoff {model} · LAN")
}

fn switch_on(v: &Value) -> Option<bool> {
    v.as_str().map(|s| s == "on")
}

fn ewe_devices(e: &EweEntry, seen: Option<&Seen>) -> Vec<Device> {
    let online = seen.is_some_and(|s| s.at.elapsed() < EWE_STALE);
    let params = seen.map(|s| &s.params);
    let make = |id: String, name: String, on: Option<bool>| {
        let mut d = Device::new(id, Provider::Ewelink, name, ewe_kind(e.uiid));
        d.via = Some(ewe_via(e));
        d.online = online;
        d.on = on.or(Some(false));
        d
    };
    if e.channels > 1 {
        (0..e.channels)
            .map(|ch| {
                let on = params
                    .and_then(|p| p.get("switches")?.as_array()?.iter().find(|s| s.get("outlet").and_then(Value::as_u64) == Some(u64::from(ch))).cloned())
                    .and_then(|s| s.get("switch").and_then(switch_on));
                make(format!("{PREFIX}ewelink:{}:{ch}", e.id), format!("{} {}", e.name, ch + 1), on)
            })
            .collect()
    } else {
        let on = params.and_then(|p| p.get("switch")).and_then(switch_on);
        vec![make(format!("{PREFIX}ewelink:{}", e.id), e.name.clone(), on)]
    }
}

async fn control_ewe(app: &Ctx, id: &str, channel: Option<u8>, cmd: Command) -> Result<()> {
    let Command::Power(on) = cmd else {
        return Err(Error::msg("This can only be switched on and off for now"));
    };
    let st = state(app);
    let (e, addr) = {
        let inner = st.inner.lock();
        let e = inner.store.ewelink.iter().find(|e| e.id == id).cloned().ok_or_else(|| Error::msg("That switch is no longer set up"))?;
        let addr = inner.ewe.get(id).map(|s| (s.ip.clone(), s.port)).or_else(|| e.ip.clone().map(|ip| (ip, SONOFF_PORT)));
        (e, addr)
    };
    let (ip, port) = addr.ok_or_else(|| Error::msg(format!("{} wasn't found on the network", e.name)))?;
    let word = if on { "on" } else { "off" };
    let (command, params) = match channel {
        Some(ch) => ("switches", json!({ "switches": [{ "switch": word, "outlet": ch }] })),
        None => ("switch", json!({ "switch": word })),
    };
    let lock = st.lock_for(id);
    let held = lock.lock().await;
    let r = ewelink_lan::send(&ip, port, &e.id, &e.key, command, params).await;
    drop(held);
    if r.is_ok() {
        // The new state shows at once; the next mDNS answer confirms it.
        let mut inner = st.inner.lock();
        let seen = inner.ewe.entry(id.to_string()).or_insert_with(|| Seen {
            at: Instant::now(),
            ip: ip.clone(),
            port,
            params: json!({}),
        });
        seen.at = Instant::now();
        match channel {
            Some(ch) => {
                if !seen.params.get("switches").is_some_and(Value::is_array) {
                    seen.params["switches"] = json!([]);
                }
                if let Some(list) = seen.params["switches"].as_array_mut() {
                    list.retain(|s| s.get("outlet").and_then(Value::as_u64) != Some(u64::from(ch)));
                    list.push(json!({ "switch": word, "outlet": ch }));
                }
            }
            None => seen.params["switch"] = json!(word),
        }
    }
    emit(app);
    r.map(|_| ()).map_err(|err| Error::msg(format!("{}: {err}", e.name)))
}

/// Carries out `cmd` on a "local:…" device.
pub async fn control(app: &Ctx, id: &str, cmd: Command) -> Result<()> {
    let rest = id.strip_prefix(PREFIX).unwrap_or(id);
    if let Some(t) = rest.strip_prefix("tuya:") {
        control_bulb(app, t, cmd).await
    } else if let Some(r) = rest.strip_prefix("ir:") {
        control_ac(app, r, cmd).await
    } else if let Some(e) = rest.strip_prefix("ewelink:") {
        match e.split_once(':') {
            Some((dev, ch)) => control_ewe(app, dev, ch.parse().ok(), cmd).await,
            None => control_ewe(app, e, None, cmd).await,
        }
    } else {
        Err(Error::msg(format!("unknown device '{id}'")))
    }
}

/// The remote id of a "local:ir:…" device.
pub fn ir_remote(id: &str) -> Option<&str> {
    id.strip_prefix(PREFIX)?.strip_prefix("ir:")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(keys: &[&str]) -> Remote {
        Remote {
            id: "r".into(),
            name: "AC".into(),
            kind: "ac".into(),
            brand: None,
            codes: keys
                .iter()
                .map(|k| (k.to_string(), Code { head: String::new(), pulse: String::new() }))
                .collect(),
        }
    }

    #[test]
    fn hsv_round_trip() {
        let (c, v) = parse_hsv("00f003e803e8").unwrap();
        assert_eq!(c, Hs { h: 240, s: 100 });
        assert_eq!(v, 100);
        assert_eq!(hsv_hex(c, v), "00f003e803e8");
        assert_eq!(hsv_hex(Hs { h: 30, s: 50 }, 1), "001e01f4000a");
        assert!(parse_hsv("000e0d0000000000000000c80000").is_none());
    }

    #[test]
    fn bulb_state_from_dps() {
        let t = TuyaEntry { id: "b".into(), name: "Abajur".into(), key: String::new(), ip: String::new(), version: "3.3".into(), mac: None, category: "dj".into() };
        let dps: Map<String, Value> = serde_json::from_value(json!({"20": true, "21": "white", "22": 1000, "23": 0, "24": "000003e803e8"})).unwrap();
        let d = bulb_device(&t, Some(&dps), true);
        assert_eq!(d.on, Some(true));
        assert_eq!(d.brightness, Some(100));
        assert_eq!(d.color_temp, Some(0));
        assert_eq!(d.color_mode, Some(ColorMode::White));
        let dps: Map<String, Value> = serde_json::from_value(json!({"20": true, "21": "colour", "22": 1000, "24": "007801f40190"})).unwrap();
        let d = bulb_device(&t, Some(&dps), true);
        assert_eq!(d.color, Some(Hs { h: 120, s: 50 }));
        assert_eq!(d.brightness, Some(40));
        assert_eq!(d.color_mode, Some(ColorMode::Color));
    }

    #[test]
    fn bulb_commands() {
        let white: Map<String, Value> = serde_json::from_value(json!({"21": "white", "22": 500, "24": "000003e803e8"})).unwrap();
        let m = bulb_dps(Some(&white), Command::Brightness(100)).unwrap();
        assert_eq!(m.get("22"), Some(&json!(1000)));
        assert_eq!(m.get("20"), Some(&json!(true)));
        let m = bulb_dps(Some(&white), Command::Color(Hs { h: 120, s: 100 })).unwrap();
        assert_eq!(m.get("21"), Some(&json!("colour")));
        // White at about half brightness (49%) carries over into the colour.
        assert_eq!(m.get("24"), Some(&json!("007803e801ea")));
        let colour: Map<String, Value> = serde_json::from_value(json!({"21": "colour", "24": "007803e801f4"})).unwrap();
        let m = bulb_dps(Some(&colour), Command::Brightness(20)).unwrap();
        assert_eq!(m.get("24"), Some(&json!("007803e800c8")));
        let m = bulb_dps(None, Command::Power(false)).unwrap();
        assert_eq!(m.len(), 1);
        assert!(bulb_dps(None, Command::AcMode(AcMode::Cool)).is_err());
    }

    #[test]
    fn ac_keys() {
        let r = remote(&["power on", "power off", "M0_T18_S0", "M0_T22_S0", "M0_T30_S3", "M3_S1", "M3_S3", "M4_S0"]);
        let s = |mode, target, fan| AcState { on: true, mode, target, fan, sent_at: None };
        assert_eq!(state_key(&r, &s(AcMode::Cool, 22, AcFan::Auto)).as_deref(), Some("M0_T22_S0"));
        // Out of range is clamped to the codes there are.
        assert_eq!(temp_range(&r, 0), Some((18, 30)));
        assert_eq!(state_key(&r, &s(AcMode::Cool, 16, AcFan::Auto)).as_deref(), Some("M0_T18_S0"));
        // Fan mode has no "auto" speed: the nearest one.
        assert_eq!(state_key(&r, &s(AcMode::Fan, 22, AcFan::Auto)).as_deref(), Some("M3_S1"));
        assert_eq!(state_key(&r, &s(AcMode::Dry, 22, AcFan::High)).as_deref(), Some("M4_S0"));
        assert_eq!(state_key(&r, &s(AcMode::Heat, 22, AcFan::Auto)), None);
        let hub = Hub { id: "h".into(), name: "Controle".into(), key: String::new(), ip: String::new(), version: "3.3".into(), remotes: vec![] };
        let d = ac_device(&hub, &r, AcState::default(), true);
        let ac = d.ac.unwrap();
        assert_eq!(ac.modes, vec![AcMode::Cool, AcMode::Fan, AcMode::Dry]);
        assert_eq!((ac.min, ac.max), (18.0, 30.0));
    }

    #[test]
    fn sonoff_channels() {
        let e = EweEntry { id: "s".into(), name: "Interruptor".into(), key: String::new(), uiid: 162, model: Some("ESP32D0-MSW-T5".into()), channels: 3, ip: None };
        let seen = Seen {
            at: Instant::now(),
            ip: "1.2.3.4".into(),
            port: 8081,
            params: json!({"switches": [{"switch": "on", "outlet": 0}, {"switch": "off", "outlet": 1}, {"switch": "off", "outlet": 2}]}),
        };
        let ds = ewe_devices(&e, Some(&seen));
        assert_eq!(ds.len(), 3);
        assert_eq!(ds[0].id, "local:ewelink:s:0");
        assert_eq!(ds[0].name, "Interruptor 1");
        assert_eq!(ds[0].on, Some(true));
        assert_eq!(ds[1].on, Some(false));
        assert!(ds[0].online);
        assert_eq!(ds[0].via.as_deref(), Some("Sonoff SwitchMan M5 · LAN"));
        let off = ewe_devices(&e, None);
        assert!(!off[0].online);
    }

    /// Reads every bulb and hub in the smarthome-local.json of the data folder `TVL_DATA` names
    /// (read-only) and shows how each one would appear. Talks to the real devices, so it's never
    /// part of a normal run, and the keys never go in the repo:
    /// `$env:TVL_DATA="C:\ProgramData\TVLauncher\server"; cargo test real_store -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn real_store_devices() {
        let Some(dir) = std::env::var_os("TVL_DATA") else {
            return eprintln!("set TVL_DATA to a data folder with {FILE}");
        };
        let path = std::path::Path::new(&dir).join(FILE);
        let store: Store = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for t in &store.tuya {
            let r = tuya_lan::status(&tuya_target(&t.id, &t.ip, &t.key, &t.version)).await;
            let d = bulb_device(t, r.as_ref().ok(), r.is_ok());
            println!(
                "{:<18} online={} on={:?} bri={:?} temp={:?} mode={:?} {}",
                d.name,
                d.online,
                d.on,
                d.brightness,
                d.color_temp,
                d.color_mode,
                r.err().map(|e| e.to_string()).unwrap_or_default()
            );
        }
        for h in &store.ir {
            let r = tuya_lan::status(&tuya_target(&h.id, &h.ip, &h.key, &h.version)).await;
            for rm in h.remotes.iter().filter(|r| !r.codes.is_empty()) {
                let d = ac_device(h, rm, AcState::default(), r.is_ok());
                let ac = d.ac.unwrap();
                let err = r.as_ref().err().map(|e| e.to_string()).unwrap_or_default();
                println!("{:<18} hub ok={} modes={:?} {}-{}° {err}", d.name, d.online, ac.modes, ac.min, ac.max);
            }
        }
    }

    #[test]
    fn store_file_shape() {
        let s: Store = serde_json::from_value(json!({
            "tuya": [{"id": "a", "name": "Abajur", "key": "k", "ip": "1.1.1.1", "version": "3.5"}],
            "ir": [{"id": "h", "name": "Hub", "key": "k", "ip": "1.1.1.2", "version": "3.3",
                    "remotes": [{"id": "r", "name": "AC", "kind": "ac", "codes": {"power on": {"head": "01", "pulse": "x"}}}]}],
            "ewelink": [{"id": "s", "name": "Sw", "key": "k", "uiid": 162, "channels": 3}]
        }))
        .unwrap();
        assert_eq!(s.tuya[0].version, "3.5");
        assert_eq!(s.ir[0].remotes[0].codes.len(), 1);
        assert_eq!(s.ewelink[0].ip, None);
    }
}
