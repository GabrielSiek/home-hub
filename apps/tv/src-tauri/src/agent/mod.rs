//! This TV PC as part of the house: its link to the home server (crates/agent-link) and what goes
//! over it.
//!
//! - The server's calls run here: what a phone or tablet asks of this TV (device/remote_api.rs),
//!   what rings here (`ring.set`), the calendar's reminder card, a sign-in page to open, the
//!   assistant's tools that act on this TV, and the pictures a page shows from this PC.
//! - The server's events reach the TV UI under the same names, with this TV's own entries marked
//!   (`this`) and only its own rings.
//! - What this TV says about itself (`mode`, `tv`, `watch`, `watch-sync`, `spotify`, `mic-ready`)
//!   goes to the screens following it; what the server needs (`device.context`, `feed.sports`) to
//!   the server.
//! - The TV UI's server-owned commands go to the server (invoke.rs).

pub mod invoke;
pub mod settings;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Listener, Manager};
use tvl_agent_link::{Host, Identity, Link, Reply, Status};
use tvl_protocol::clock::RingSet;
use tvl_protocol::{topics, Caps};

use crate::error::{Error, Result};
use crate::state::AppState;

/// Where TV PCs look for the server: the service's probe port, and in a debug build first the
/// one of `tvl-server run --dev`.
fn probe_ports() -> Vec<u16> {
    if cfg!(debug_assertions) {
        vec![47972, 47962]
    } else {
        vec![47962]
    }
}

/// The running-app id of a sign-in page the server asked to open.
const SIGN_IN_APP: &str = "server-signin";
/// Sign-in pages the server may open here.
const SIGN_IN_PAGES: &[&str] = &["https://accounts.google.com/", "https://accounts.spotify.com/", "https://c2ccdn.coolkit.cc/"];
/// The largest picture a page may load from this PC.
const MOST_FILE: u64 = 5 * 1024 * 1024;

/// This TV PC's lasting identity (agent.json): made on first run.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Kept {
    device: String,
    name: String,
}

/// This TV PC's id and room, as the house last listed it.
type Me = Option<(String, Option<String>)>;

pub struct Agent {
    link: Link,
    /// Who it is in the house: the ring fail-safe rings what's aimed at it.
    me: Mutex<Me>,
}

fn agent(app: &AppHandle) -> Option<tauri::State<'_, Agent>> {
    app.try_state::<Agent>()
}

/// Runs `cmd` on the server.
pub async fn server_call(app: &AppHandle, cmd: &str, args: Value) -> std::result::Result<Value, String> {
    let link = agent(app).map(|a| a.link.clone()).ok_or_else(|| tvl_agent_link::OFFLINE.to_string())?;
    link.call(cmd, args).await
}

pub fn online(app: &AppHandle) -> bool {
    agent(app).is_some_and(|a| a.link.is_online())
}

/// This TV PC's id and room, once the server has said.
pub fn me(app: &AppHandle) -> Me {
    me_slot(app)?.lock().unwrap().clone()
}

fn me_slot(app: &AppHandle) -> Option<&Mutex<Me>> {
    app.try_state::<Agent>().map(|s| &s.inner().me)
}

pub fn status(app: &AppHandle) -> Option<Status> {
    Some(agent(app)?.link.status())
}

pub fn link(app: &AppHandle) -> Option<Link> {
    Some(agent(app)?.link.clone())
}

fn identity(dir: &Path) -> Kept {
    let path = dir.join("agent.json");
    if let Some(k) = tvl_kit::store::load::<Kept>(&path) {
        return k;
    }
    let pc = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "TV".into());
    let k = Kept { device: format!("pc-{}", tvl_kit::oauth::random_string(10).to_ascii_lowercase()), name: pc };
    if let Err(e) = tvl_kit::store::save(&path, &k, true) {
        tracing::error!("can't keep this TV PC's id: {e}");
    }
    k
}

/// Starts the link to the server.
pub fn start(app: &AppHandle) {
    let st = app.state::<AppState>();
    let kept = identity(&st.data_dir);
    let caps = Caps {
        remote: true,
        volume: true,
        apps: true,
        mic: crate::assistant::voice::has_microphone(),
        voice: st.config.read().unwrap().settings.assistant_speech != crate::config::Speech::Off,
        ring: true,
        spotify_device: None,
    };
    let me = Identity { device: kept.device, name: kept.name, caps, mac: wired_mac(), app_version: env!("CARGO_PKG_VERSION").into() };
    let host = Arc::new(Tv { app: app.clone() });
    let link = Link::new(&st.data_dir, me, host, probe_ports());
    app.manage(Agent { link: link.clone(), me: Mutex::new(None) });
    tauri::async_runtime::spawn(link.run());

    // What this TV says about itself, to the screens following it.
    for name in topics::FROM_AGENT_TO_FOLLOWERS {
        let handle = app.clone();
        app.listen_any(*name, move |e| {
            let payload = serde_json::from_str(e.payload()).unwrap_or(Value::Null);
            if let Some(l) = link_of(&handle) {
                l.event(name, payload);
            }
        });
    }
    // What's running changed: the assistant on the server should know.
    let handle = app.clone();
    app.listen_any("mode", move |_| send_context(&handle));
    // The volume changed by other means (the pad, the assistant, Windows itself): the screens
    // following this TV hear it instead of asking every few seconds.
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut last = None;
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            let now = crate::assistant::system::volume(|_| {}).ok().map(|v| (v.level, v.muted));
            if now != last && last.is_some() {
                crate::device::tv::emit(&handle);
            }
            last = now;
        }
    });
    // Grêmio's matches for the server's calendar, now and then (sports moves to the server later).
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(3 * 3600)).await;
            send_sports(&handle).await;
        }
    });
}

fn link_of(app: &AppHandle) -> Option<Link> {
    agent(app).map(|a| a.link.clone())
}

/// What the assistant on the server should know about this TV: what's open, its apps, its games.
fn send_context(app: &AppHandle) {
    let Some(link) = link_of(app) else { return };
    let st = app.state::<AppState>();
    let running = st.running.lock().unwrap().as_ref().map(|r| r.app_id.clone());
    let (apps, running) = {
        let cfg = st.config.read().unwrap();
        let apps: Vec<Value> = cfg.apps.iter().map(|a| json!({ "id": a.id, "name": a.name })).collect();
        let running = running.map(|id| {
            let name = cfg.apps.iter().find(|a| a.id == id).map_or_else(|| id.clone(), |a| a.name.clone());
            json!({ "id": id, "name": name })
        });
        (apps, running)
    };
    let games: Vec<String> = crate::games::library(app).games.into_iter().filter(|g| g.installed).map(|g| g.name).collect();
    link.event("device.context", json!({ "running": running, "apps": apps, "games": games }));
}

async fn send_sports(app: &AppHandle) {
    let games = crate::sports::gremio_schedule(app).await;
    let reminders = crate::sports::reminders(app);
    if let Some(link) = link_of(app) {
        link.event("feed.sports", json!({ "games": games, "reminders": reminders, "at": crate::history::now_ms() }));
    }
}

/// Marks this TV PC's own entry in a list of the house's screens (`this`), for the TV UI.
fn mark_this(app: &AppHandle, screens: &mut Value) {
    let Some((id, _)) = me(app) else { return };
    for s in screens.as_array_mut().into_iter().flatten() {
        if s["id"] == id.as_str() {
            s["this"] = json!(true);
        }
    }
}

/// A server event or answer as the TV UI reads it.
pub fn for_this_tv(app: &AppHandle, name: &str, mut payload: Value) -> Value {
    match name {
        "devices" | "devices_get" => mark_this(app, &mut payload),
        "smarthome" | "smarthome_get" => mark_this(app, &mut payload["screens"]),
        _ => {}
    }
    payload
}

/// The TV app's side of the link.
struct Tv {
    app: AppHandle,
}

impl Host for Tv {
    fn call(&self, cmd: String, args: Value, from: Option<String>) -> Reply {
        let app = self.app.clone();
        Box::pin(async move {
            let read = tvl_protocol::spec(&cmd).is_some_and(|s| s.idempotent);
            if !read && !cmd.starts_with("ring.") {
                tracing::info!(cmd, from = from.as_deref().unwrap_or("server"), "from the server");
            }
            match cmd.as_str() {
                "ring.set" => {
                    let set: RingSet = serde_json::from_value(args).map_err(|e| e.to_string())?;
                    crate::ring::set(&app, set);
                    Ok(Value::Null)
                }
                "calendar.card" => {
                    crate::overlay::calendar_card(&app, &args);
                    Ok(Value::Null)
                }
                "oauth.loopback" => sign_in_page(&app, &args).await.map_err(|e| e.to_string()),
                "assistant.tool" => {
                    let name = args["name"].as_str().unwrap_or_default();
                    let tool_args = args["args"].as_str().unwrap_or("{}");
                    Ok(crate::assistant::tools::run(&app, name, tool_args).await)
                }
                "device.file" => picture(&app, args["path"].as_str().unwrap_or_default()).map_err(|e| e.to_string()),
                _ => match crate::device::remote_api::dispatch(&app, &cmd, args).await {
                    Ok(v) => Ok(v),
                    Err(crate::device::remote_api::Failure::Unknown) => Err(format!("This TV PC doesn't do {cmd}")),
                    Err(crate::device::remote_api::Failure::Failed(e)) => Err(e.to_string()),
                },
            }
        })
    }

    fn event(&self, name: String, payload: Value) {
        match name.as_str() {
            "clock" => crate::ring::clock(&self.app, payload),
            "settings" => settings::apply_house(&self.app, &payload),
            // Fell behind: everything again, as after reconnecting.
            "resync" => {
                let app = self.app.clone();
                tauri::async_runtime::spawn(async move { refresh(&app).await });
            }
            _ => {
                if name == "devices" {
                    self.remember_me(&payload);
                }
                let payload = for_this_tv(&self.app, &name, payload);
                let _ = self.app.emit(&name, payload);
            }
        }
    }

    fn status(&self, status: &Status) {
        if let (Status::Online { screen, .. }, Some(me)) = (status, me_slot(&self.app)) {
            *me.lock().unwrap() = Some((screen.id.clone(), screen.room.clone()));
        }
        let _ = self.app.emit("server", status);
    }

    fn connected(&self) {
        crate::ring::connected(&self.app);
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            send_context(&app);
            crate::device::tv::emit(&app);
            let mode = app.state::<AppState>().mode();
            if let Some(l) = link_of(&app) {
                l.event("mode", json!(mode));
            }
            refresh(&app).await;
            send_sports(&app).await;
        });
    }
}

impl Tv {
    /// A room change for this TV PC arrives in the devices list.
    fn remember_me(&self, devices: &Value) {
        let Some(slot) = me_slot(&self.app) else { return };
        let mut me = slot.lock().unwrap();
        let Some((id, room)) = me.as_mut() else { return };
        if let Some(s) = devices.as_array().into_iter().flatten().find(|s| s["id"] == id.as_str()) {
            *room = s["room"].as_str().map(str::to_string);
        }
    }
}

/// The household's state again, for the TV UI (after reconnecting or falling behind): the same
/// events the server sends as things change.
async fn refresh(app: &AppHandle) {
    for (cmd, event) in [("smarthome_get", "smarthome"), ("companion_get", "companion"), ("calendar_link", "calendar"), ("devices_get", "devices")] {
        match server_call(app, cmd, json!({})).await {
            Ok(v) => {
                let v = for_this_tv(app, event, v);
                let _ = app.emit(event, v);
            }
            Err(e) => tracing::debug!("{cmd} after reconnecting: {e}"),
        }
    }
    if let Ok(clock) = server_call(app, "clock_get", json!({})).await {
        crate::ring::clock(app, clock);
    }
}

/// The server's `oauth.loopback {url, port, path, service, state}`: its sign-in page in this
/// PC's browser, and the redirect back to 127.0.0.1 caught here. Answers the redirect's query
/// (`code`, `state` or `error`); the server checks the state and exchanges the code.
async fn sign_in_page(app: &AppHandle, args: &Value) -> Result<Value> {
    let url = args["url"].as_str().unwrap_or_default();
    if !SIGN_IN_PAGES.iter().any(|p| url.starts_with(p)) {
        return Err(Error::msg("Not a sign-in page this TV opens"));
    }
    let port = args["port"].as_u64().and_then(|p| u16::try_from(p).ok()).ok_or_else(|| Error::msg("No port"))?;
    let path = args["path"].as_str().unwrap_or("/");
    let service = args["service"].as_str().unwrap_or("the");
    let expected = args["state"].as_str().unwrap_or_default().to_string();
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|e| Error::msg(format!("Port {port} for the {service} sign-in is busy: {e}")))?;
    crate::launcher::open_page(app, url, SIGN_IN_APP)?;
    let result = tvl_kit::oauth::loopback::wait_for_redirect(
        &listener,
        path,
        service,
        || crate::launcher::is_running(app, SIGN_IN_APP),
        |q| q.get("state") == Some(&expected) && (q.contains_key("code") || q.contains_key("error")),
    )
    .await;
    crate::launcher::close_if(app, SIGN_IN_APP);
    Ok(json!(result?))
}

/// A picture from this PC for a page (the server's `device.file {path}`): an app tile, art or a
/// photo from this app's own folders, as base64. Nothing else on the PC.
fn picture(app: &AppHandle, path: &str) -> Result<Value> {
    let dir = app.state::<AppState>().data_dir.clone();
    let wanted = std::fs::canonicalize(path).map_err(|_| Error::msg("No such picture"))?;
    let allowed = ["icons", "art", "photos", "retro-art"]
        .iter()
        .filter_map(|d| std::fs::canonicalize(dir.join(d)).ok())
        .any(|d| wanted.starts_with(d));
    let image = wanted
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["png", "jpg", "jpeg", "webp", "gif", "avif"].contains(&e.to_ascii_lowercase().as_str()));
    if !allowed || !image {
        return Err(Error::msg("Not a picture a page may see"));
    }
    if std::fs::metadata(&wanted)?.len() > MOST_FILE {
        return Err(Error::msg("That picture is too big"));
    }
    Ok(json!(base64::engine::general_purpose::STANDARD.encode(std::fs::read(&wanted)?)))
}

/// The network address of this PC's wired connection, for Wake-on-LAN. None on Wi-Fi only, where
/// waking doesn't work anyway.
#[cfg(windows)]
fn wired_mac() -> Option<String> {
    use windows::Win32::NetworkManagement::IpHelper::{GetAdaptersInfo, IP_ADAPTER_INFO};
    const ETHERNET: u32 = 6;
    let mut size = 0u32;
    // SAFETY: asks only for the size the list needs.
    unsafe { GetAdaptersInfo(None, &mut size) };
    if size == 0 {
        return None;
    }
    // u64s, so the list is aligned for IP_ADAPTER_INFO.
    let mut buf = vec![0u64; (size as usize).div_ceil(8)];
    let first = buf.as_mut_ptr().cast::<IP_ADAPTER_INFO>();
    // SAFETY: the buffer is `size` bytes, as asked.
    if unsafe { GetAdaptersInfo(Some(first), &mut size) } != 0 {
        return None;
    }
    let mut next = first.cast_const();
    while !next.is_null() {
        // SAFETY: a list GetAdaptersInfo wrote into `buf`, which outlives the loop.
        let a = unsafe { &*next };
        let gateway: Vec<u8> = a.GatewayList.IpAddress.String.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
        let has_gateway = !gateway.is_empty() && gateway != b"0.0.0.0";
        if a.Type == ETHERNET && a.AddressLength == 6 && has_gateway {
            return Some(a.Address[..6].iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":"));
        }
        next = a.Next.cast_const();
    }
    None
}

#[cfg(not(windows))]
fn wired_mac() -> Option<String> {
    None
}
