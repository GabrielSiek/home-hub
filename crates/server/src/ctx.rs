//! What every handler and background loop shares: the old launcher's `AppState`, without Tauri.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use parking_lot::Mutex;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use tvl_kit::store::JsonStore;
use tvl_kit::time::Clock;
use tvl_kit::Result;

use crate::admin::Admin;
use crate::agents::Agents;
use crate::bus::Bus;
use crate::config::Config;
use crate::devices::Devices;
use crate::assistant::Assistant;
use crate::calendar;
use crate::clock;
use crate::hub::Hub;
use crate::library::Library;
use crate::settings::Settings;
use crate::smarthome::SmartHome;
use crate::http::security::Names;
use crate::tls::Authority;

pub struct Server {
    pub config: Config,
    pub names: Arc<Names>,
    pub bus: Bus,
    /// Cancelled when the server stops: every loop and socket ends.
    pub shutdown: CancellationToken,
    /// Where "now" comes from (tests move it by hand).
    pub time: Arc<dyn Clock>,
    /// Signs the HTTPS certificates; None when it couldn't be loaded or made (no HTTPS then).
    pub ca: Option<Arc<Authority>>,
    pub devices: Devices,
    pub admin: Admin,
    pub agents: Agents,
    pub smarthome: SmartHome,
    pub hub: Hub,
    pub clock: clock::State,
    pub calendar: calendar::State,
    pub assistant: Assistant,
    pub settings: Settings,
    pub library: Library,
    pub started: Instant,
    /// The HTTPS port, once it's listening (TV PCs are told it).
    pub https_port: OnceLock<u16>,
    /// The latest of each event a TV PC sends only to the server (`device.context`, …).
    agent_events: Mutex<HashMap<(String, String), Value>>,
    /// TV PCs' network addresses, for Wake-on-LAN (kept, since a PC that's off can't say it).
    macs: JsonStore<HashMap<String, String>>,
}

pub type Ctx = Arc<Server>;

impl Server {
    pub fn new(config: Config, clock: Arc<dyn Clock>) -> Result<Ctx> {
        std::fs::create_dir_all(&config.data_dir)?;
        let bus = Bus::new();
        let ca = match Authority::load(&config.data_dir, &config.names) {
            Ok(ca) => Some(Arc::new(ca)),
            Err(e) => {
                tracing::error!("no HTTPS, the certificate authority failed: {e}");
                None
            }
        };
        Ok(Arc::new(Server {
            names: Arc::new(Names::new(&config.names)),
            devices: Devices::open(&config.data_dir, clock.clone(), bus.clone()),
            admin: Admin::open(&config.data_dir, clock.clone())?,
            agents: Agents::default(),
            smarthome: SmartHome::new(&config.data_dir),
            hub: Hub::open(&config.data_dir),
            clock: clock::State::new(&config.data_dir),
            calendar: calendar::State::new(&config.data_dir),
            assistant: Assistant::default(),
            settings: Settings::open(&config.data_dir)?,
            library: Library::default(),
            macs: JsonStore::open(config.data_dir.join("wake.json")),
            agent_events: Mutex::default(),
            started: Instant::now(),
            https_port: OnceLock::new(),
            shutdown: CancellationToken::new(),
            ca,
            time: clock,
            bus,
            config,
        }))
    }

    pub fn agent_event(&self, device: &str, name: &str, payload: Value) {
        self.agent_events.lock().insert((device.to_string(), name.to_string()), payload);
    }

    /// The last `name` event TV PC `device` sent the server.
    pub fn agent_latest(&self, device: &str, name: &str) -> Option<Value> {
        self.agent_events.lock().get(&(device.to_string(), name.to_string())).cloned()
    }

    pub fn agent_mac(&self, device: &str) -> Option<String> {
        self.macs.read(|m| m.get(device).cloned())
    }

    pub fn set_agent_mac(&self, device: &str, mac: &str) {
        if self.agent_mac(device).as_deref() == Some(mac) {
            return;
        }
        if let Err(e) = self.macs.change(|m| {
            m.insert(device.to_string(), mac.to_string());
            Ok(())
        }) {
            tracing::warn!("can't keep {device}'s network address: {e}");
        }
    }
}
