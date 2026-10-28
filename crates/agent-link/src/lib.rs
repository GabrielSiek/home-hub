//! A TV PC's link to the home server (`/api/agent`), apart from Tauri so it's tested against the
//! real server. It finds the server (the address typed in Settings, else a UDP probe), pairs once
//! (the TV shows a code until someone approves it with the admin PIN), then keeps one socket open:
//! the server's calls and events come in, the TV's go out, and it reconnects whenever it drops.
//!
//! After pairing it connects over `wss://`, trusting only the home certificate authority the
//! server handed over then.

mod connect;
mod discover;

pub use discover::{probe, Found};

use std::collections::HashMap;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{FutureExt, SinkExt, StreamExt};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot, watch, Notify};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use tvl_kit::store::JsonStore;
use tvl_protocol::wire::{self, Hello};
use tvl_protocol::{Caps, Frame, Screen, PROTOCOL_VERSION};

/// What a call says while the server can't be reached.
pub const OFFLINE: &str = "The home server isn't answering";
/// The first calls after starting wait this long for the link before giving up.
const FIRST_WAIT: Duration = Duration::from_secs(2);
/// The server pings every 15 s: this long without a word and the socket is dead.
const SILENT: Duration = Duration::from_secs(40);
/// Pairing codes last ten minutes on the server; a new one is asked for after that.
const PAIRING_LASTS: Duration = Duration::from_secs(10 * 60);
/// After a refusal (another version, pairing denied), until someone asks again from Settings.
const AFTER_REFUSAL: Duration = Duration::from_secs(10 * 60);
const MOST_BETWEEN_TRIES: Duration = Duration::from_secs(30);
/// Probe answers are forgotten after this many failed connections in a row (the server moved).
const PROBE_AGAIN_AFTER: u32 = 3;

/// This TV PC, as it says hello.
#[derive(Debug, Clone)]
pub struct Identity {
    /// Kept by the TV app for good.
    pub device: String,
    pub name: String,
    pub caps: Caps,
    /// For Wake-on-LAN.
    pub mac: Option<String>,
    pub app_version: String,
}

/// What the link keeps between runs (`link.json` in the TV app's data folder).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    /// Typed in Settings: "192.168.68.20", "tvl.local:47960". None: found by the probe.
    pub address: Option<String>,
    /// Where the probe last found it.
    pub found: Option<Found>,
    /// This TV PC's token, from pairing. Secret.
    pub token: Option<String>,
    /// The home certificate authority it trusts for `wss://`.
    pub ca_pem: Option<String>,
    /// The server's HTTPS port, from its welcome.
    pub https: Option<u16>,
}

/// How the link is doing, for the TV's Settings and its "Server offline" banner.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Status {
    /// Looking for the server on the network.
    Searching,
    Connecting { server: String },
    /// Waiting for someone to approve this TV PC (with the admin PIN): the code it shows.
    Pairing { server: String, code: String },
    Online {
        server: String,
        /// This TV PC as the house lists it.
        screen: Screen,
        admin_pin: String,
        /// Over `wss://`.
        secure: bool,
        /// The home certificate authority's fingerprint, to compare with the server's.
        fingerprint: Option<String>,
    },
    /// Not connected; trying again.
    Offline { server: Option<String>, error: String },
    /// The server said no (another version, pairing denied); tried again later or when asked.
    Refused { server: String, reason: String },
}

pub type Reply = Pin<Box<dyn Future<Output = Result<Value, String>> + Send>>;

/// The TV app's side: what runs the server's calls and hears its events.
pub trait Host: Send + Sync + 'static {
    /// The server asks this TV PC to run `cmd`; `from` is the screen that asked.
    fn call(&self, cmd: String, args: Value, from: Option<String>) -> Reply;
    /// An event from the server.
    fn event(&self, name: String, payload: Value);
    fn status(&self, status: &Status);
    /// Connected (again): a moment to tell the server what it should know now.
    fn connected(&self) {}
}

/// The link; clones share it.
#[derive(Clone)]
pub struct Link(Arc<Inner>);

struct Inner {
    me: RwLock<Identity>,
    saved: JsonStore<Saved>,
    host: Arc<dyn Host>,
    probe_ports: Vec<u16>,
    status: RwLock<Status>,
    /// The open socket's outgoing frames; None while there's none.
    out: Mutex<Option<mpsc::UnboundedSender<String>>>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>,
    next: AtomicU64,
    online: watch::Sender<bool>,
    /// Drops the socket (or the wait) and tries again now: the address changed, pairing was asked.
    again: Notify,
    shutdown: CancellationToken,
    started: Instant,
}

/// Where to connect this time.
struct Target {
    host: String,
    http: u16,
    https: Option<u16>,
    probed: bool,
}

impl Target {
    fn label(&self) -> String {
        format!("{}:{}", self.host, self.http)
    }
}

/// How a connection ended.
enum Ended {
    /// It never got going, or dropped: tried again soon.
    Failed(String),
    /// It was up and closed.
    Closed,
    Refused(String),
    /// Asked to try again at once.
    Again,
    Stopped,
}

/// "192.168.68.20" → (host, 47960); "tvl.local:47970" → (host, 47970); "[fe80::1]:1" works too.
pub fn parse_address(text: &str) -> Option<(String, u16)> {
    let text = text.trim().trim_start_matches("http://").trim_start_matches("https://").trim_end_matches('/');
    if text.is_empty() {
        return None;
    }
    if let Some(rest) = text.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        let port = match after.strip_prefix(':') {
            Some(p) => p.parse().ok()?,
            None => 47960,
        };
        return Some((host.to_string(), port));
    }
    match text.rsplit_once(':') {
        // One colon: host and port. More: a bare IPv6 address.
        Some((host, port)) if !host.contains(':') => Some((host.to_string(), port.parse().ok()?)),
        _ => Some((text.to_string(), 47960)),
    }
}

impl Link {
    /// The link of the TV PC `me`, keeping its state in `dir`. `probe_ports`: where to look for the
    /// server when no address is typed. Nothing happens until `run`.
    pub fn new(dir: &Path, me: Identity, host: Arc<dyn Host>, probe_ports: Vec<u16>) -> Link {
        Link(Arc::new(Inner {
            me: RwLock::new(me),
            saved: JsonStore::open(dir.join("link.json")),
            host,
            probe_ports,
            status: RwLock::new(Status::Searching),
            out: Mutex::default(),
            pending: Mutex::default(),
            next: AtomicU64::new(1),
            online: watch::Sender::new(false),
            again: Notify::new(),
            shutdown: CancellationToken::new(),
            started: Instant::now(),
        }))
    }

    pub fn status(&self) -> Status {
        self.0.status.read().clone()
    }

    pub fn is_online(&self) -> bool {
        *self.0.online.borrow()
    }

    /// The address typed in Settings, if any.
    pub fn address(&self) -> Option<String> {
        self.0.saved.read(|s| s.address.clone())
    }

    pub fn is_paired(&self) -> bool {
        self.0.saved.read(|s| s.token.is_some())
    }

    /// Sets (or, with None or blank, clears) the server's address, and connects there now.
    pub fn set_address(&self, address: Option<&str>) -> tvl_kit::Result<()> {
        let address = address.map(str::trim).filter(|a| !a.is_empty());
        if let Some(a) = address {
            if parse_address(a).is_none() {
                return Err(tvl_kit::Error::msg("Not an address: try 192.168.0.10 or tvl.local:47960"));
            }
        }
        self.0.saved.change(|s| {
            s.address = address.map(str::to_string);
            s.found = None;
            Ok(())
        })?;
        self.0.again.notify_one();
        Ok(())
    }

    /// Forgets this TV PC's pairing and asks for a new one (a new code on the TV).
    pub fn pair_again(&self) -> tvl_kit::Result<()> {
        self.0.saved.change(|s| {
            s.token = None;
            s.ca_pem = None;
            s.https = None;
            Ok(())
        })?;
        self.0.again.notify_one();
        Ok(())
    }

    /// Tries now instead of waiting (after a refusal, or a "Retry" in Settings).
    pub fn retry(&self) {
        self.0.again.notify_one();
    }

    /// The name it says at the next hello.
    pub fn rename(&self, name: &str) {
        self.0.me.write().name = name.to_string();
    }

    pub fn stop(&self) {
        self.0.shutdown.cancel();
    }

    /// Runs `cmd` on the server and waits for its answer. Right after starting it waits a moment
    /// for the link; after that, while it's down, it fails at once.
    pub async fn call(&self, cmd: &str, args: Value) -> Result<Value, String> {
        let me = &self.0;
        if !self.is_online() {
            let Some(left) = FIRST_WAIT.checked_sub(me.started.elapsed()) else { return Err(OFFLINE.into()) };
            let mut online = me.online.subscribe();
            let _ = tokio::time::timeout(left, online.wait_for(|on| *on)).await;
        }
        let Some(out) = me.out.lock().clone() else { return Err(OFFLINE.into()) };
        let id = me.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        me.pending.lock().insert(id, tx);
        let frame = Frame::Call { id, cmd: cmd.to_string(), args, from: None };
        if out.send(frame.to_text()).is_err() {
            me.pending.lock().remove(&id);
            return Err(OFFLINE.into());
        }
        // The server's own wait for the command, and a little for the trip.
        let within = Duration::from_millis(u64::from(tvl_protocol::spec(cmd).map_or(10_000, |s| s.timeout_ms)) + 2_000);
        match tokio::time::timeout(within, rx).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => Err("The home server went away".into()),
            Err(_) => {
                me.pending.lock().remove(&id);
                Err("The home server didn't answer in time".into())
            }
        }
    }

    /// Sends the server an event; false while offline (it's dropped: events are snapshots, and
    /// `Host::connected` sends what matters again).
    pub fn event(&self, name: &str, payload: Value) -> bool {
        let Some(out) = self.0.out.lock().clone() else { return false };
        out.send(Frame::Event { name: name.to_string(), payload }.to_text()).is_ok()
    }

    fn set_status(&self, status: Status) {
        let changed = {
            let mut now = self.0.status.write();
            let changed = *now != status;
            *now = status.clone();
            changed
        };
        if changed {
            self.0.host.status(&status);
        }
    }

    /// Connects, and connects again, until `stop`.
    pub async fn run(self) {
        let mut failures = 0u32;
        while !self.0.shutdown.is_cancelled() {
            let wait = match self.target().await {
                None => {
                    self.set_status(Status::Searching);
                    failures += 1;
                    between_tries(failures)
                }
                Some(target) => match self.session(&target).await {
                    Ended::Failed(error) => {
                        failures += 1;
                        if target.probed && failures >= PROBE_AGAIN_AFTER {
                            let _ = self.0.saved.change(|s| {
                                s.found = None;
                                Ok(())
                            });
                        }
                        tracing::info!("server link: {error}");
                        self.set_status(Status::Offline { server: Some(target.label()), error });
                        between_tries(failures)
                    }
                    Ended::Closed => {
                        failures = 0;
                        self.set_status(Status::Offline { server: Some(target.label()), error: "The connection dropped".into() });
                        between_tries(1)
                    }
                    Ended::Refused(reason) => {
                        tracing::warn!("server link refused: {reason}");
                        self.set_status(Status::Refused { server: target.label(), reason });
                        AFTER_REFUSAL
                    }
                    Ended::Again => Duration::ZERO,
                    Ended::Stopped => break,
                },
            };
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = self.0.again.notified() => failures = 0,
                _ = self.0.shutdown.cancelled() => break,
            }
        }
        self.set_status(Status::Offline { server: None, error: "Stopped".into() });
    }

    async fn target(&self) -> Option<Target> {
        let saved = self.0.saved.get();
        if let Some((host, http)) = saved.address.as_deref().and_then(parse_address) {
            return Some(Target { host, http, https: saved.https, probed: false });
        }
        let found = match saved.found {
            Some(f) => f,
            None => {
                let f = discover::probe(&self.0.probe_ports, Duration::from_secs(2)).await?;
                tracing::info!(host = f.host, port = f.http, "found the server");
                let _ = self.0.saved.change(|s| {
                    s.found = Some(f.clone());
                    Ok(())
                });
                f
            }
        };
        Some(Target { host: found.host, http: found.http, https: saved.https.or(found.https), probed: true })
    }

    /// One connection, from opening to its end.
    async fn session(&self, target: &Target) -> Ended {
        let me = &self.0;
        let saved = me.saved.get();
        let server = target.label();
        self.set_status(Status::Connecting { server: server.clone() });
        let ca = saved.ca_pem.as_deref().filter(|p| !p.trim().is_empty());
        let secure = match (saved.token.is_some(), target.https, ca) {
            (true, Some(port), Some(ca)) => Some((port, ca)),
            _ => None,
        };
        let opening = connect::open(&target.host, target.http, secure, saved.token.as_deref());
        let (socket, over_tls) = tokio::select! {
            opened = opening => match opened {
                Ok(s) => s,
                Err(e) => return Ended::Failed(e),
            },
            _ = me.again.notified() => return Ended::Again,
            _ = me.shutdown.cancelled() => return Ended::Stopped,
        };
        let (mut sink, mut stream) = socket.split();

        if saved.token.is_none() {
            if let Err(ended) = self.pair(&server, &mut sink, &mut stream).await {
                return ended;
            }
        }
        let hello = {
            let i = me.me.read();
            Frame::Hello(Hello {
                version: PROTOCOL_VERSION,
                device: i.device.clone(),
                name: i.name.clone(),
                caps: i.caps.clone(),
                mac: i.mac.clone(),
                app_version: i.app_version.clone(),
            })
        };
        if sink.send(Message::Text(hello.to_text().into())).await.is_err() {
            return Ended::Failed("The server closed the connection".into());
        }
        let welcome = match next_frame(&mut stream, Duration::from_secs(10)).await {
            Some(Frame::Welcome(w)) => w,
            Some(Frame::Refused { reason }) => return self.refused(reason),
            _ => return Ended::Failed("The server didn't answer".into()),
        };
        if welcome.version != PROTOCOL_VERSION {
            return Ended::Refused(format!("Update this TV PC: the server speaks version {}", welcome.version));
        }
        if saved.https != welcome.https {
            let _ = me.saved.change(|s| {
                s.https = welcome.https;
                Ok(())
            });
        }

        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        *me.out.lock() = Some(tx.clone());
        me.online.send_replace(true);
        let fingerprint = me.saved.read(|s| s.ca_pem.as_deref().and_then(connect::fingerprint));
        tracing::info!(server, secure = over_tls, "connected to the server");
        self.set_status(Status::Online { server, screen: welcome.screen, admin_pin: welcome.admin_pin, secure: over_tls, fingerprint });
        me.host.connected();

        let mut heard = Instant::now();
        let mut check = tokio::time::interval(Duration::from_secs(5));
        let ended = loop {
            tokio::select! {
                _ = me.shutdown.cancelled() => break Ended::Stopped,
                _ = me.again.notified() => break Ended::Again,
                out = rx.recv() => {
                    let Some(text) = out else { break Ended::Closed };
                    if sink.send(Message::Text(text.into())).await.is_err() {
                        break Ended::Closed;
                    }
                }
                incoming = stream.next() => match incoming {
                    Some(Ok(Message::Text(t))) => {
                        heard = Instant::now();
                        self.received(&tx, t.as_str());
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break Ended::Closed,
                    // Pings (answered by the socket itself) and pongs.
                    Some(Ok(_)) => heard = Instant::now(),
                },
                _ = check.tick() => {
                    if heard.elapsed() > SILENT {
                        break Ended::Failed("The server stopped answering".into());
                    }
                }
            }
        };
        *me.out.lock() = None;
        me.online.send_replace(false);
        for (_, waiting) in me.pending.lock().drain() {
            let _ = waiting.send(Err("The home server went away".into()));
        }
        let _ = sink.close().await;
        ended
    }

    /// Asks to join the house and waits, showing the code, until someone approves it.
    async fn pair<S, R>(&self, server: &str, sink: &mut S, stream: &mut R) -> Result<(), Ended>
    where
        S: futures_util::Sink<Message> + Unpin,
        R: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    {
        let me = &self.0;
        let ask = {
            let i = me.me.read();
            Frame::Pair { device: i.device.clone(), name: i.name.clone(), version: PROTOCOL_VERSION }
        };
        if sink.send(Message::Text(ask.to_text().into())).await.is_err() {
            return Err(Ended::Failed("The server closed the connection".into()));
        }
        let until = Instant::now() + PAIRING_LASTS;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            let frame = tokio::select! {
                f = next_frame(stream, left) => f,
                _ = me.again.notified() => return Err(Ended::Again),
                _ = me.shutdown.cancelled() => return Err(Ended::Stopped),
            };
            match frame {
                Some(Frame::PairCode { code }) => {
                    tracing::info!("pairing: the TV shows code {code}");
                    self.set_status(Status::Pairing { server: server.to_string(), code });
                }
                Some(Frame::Paired { token, ca_pem }) => {
                    let ca = Some(ca_pem).filter(|p| !p.trim().is_empty());
                    me.saved.change(|s| {
                        s.token = Some(token);
                        s.ca_pem = ca;
                        Ok(())
                    })
                    .map_err(|e| Ended::Failed(format!("Can't keep the pairing: {e}")))?;
                    tracing::info!("paired with the server");
                    return Ok(());
                }
                Some(Frame::Refused { reason }) => return Err(Ended::Refused(reason)),
                Some(_) => {}
                // The code ran out, or the socket closed: ask again.
                None => return Err(Ended::Failed("Pairing stopped: asking again".into())),
            }
        }
    }

    /// The server refused this TV PC. A token it doesn't know (any more) is forgotten, so the
    /// next try asks to pair.
    fn refused(&self, reason: String) -> Ended {
        if wire::means_pair_again(&reason) {
            let _ = self.0.saved.change(|s| {
                s.token = None;
                Ok(())
            });
            return Ended::Failed(reason);
        }
        Ended::Refused(reason)
    }

    fn received(&self, tx: &mpsc::UnboundedSender<String>, text: &str) {
        let frame: Frame = match serde_json::from_str(text) {
            Ok(f) => f,
            Err(e) => return tracing::debug!("unreadable frame from the server: {e}"),
        };
        match frame {
            Frame::Call { id, cmd, args, from } => {
                let (host, tx) = (self.0.host.clone(), tx.clone());
                // Each on its own task: a call can wait on one of ours (the assistant asks this
                // TV to open an app while the TV waits for the assistant).
                tokio::spawn(async move {
                    let outcome = AssertUnwindSafe(host.call(cmd.clone(), args, from))
                        .catch_unwind()
                        .await
                        .unwrap_or_else(|_| Err(format!("{cmd} crashed on the TV PC")));
                    let _ = tx.send(Frame::reply(id, outcome).to_text());
                });
            }
            Frame::Reply { id, ok, err } => {
                if let Some(waiting) = self.0.pending.lock().remove(&id) {
                    let _ = waiting.send(Frame::outcome(ok, err));
                }
            }
            Frame::Event { name, payload } => self.0.host.event(name, payload),
            other => tracing::debug!("frame out of place from the server: {other:?}"),
        }
    }
}

/// 1 s, 2 s, 4 s… up to 30 s, a little off each time so TV PCs don't all come back together.
fn between_tries(failures: u32) -> Duration {
    let base = Duration::from_secs(1u64 << failures.saturating_sub(1).min(5)).min(MOST_BETWEEN_TRIES);
    let jitter = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_millis() % 250);
    base + Duration::from_millis(u64::from(jitter))
}

/// The next frame within `within`, skipping pings; None on a closed socket, garbage or the time.
async fn next_frame<R>(stream: &mut R, within: Duration) -> Option<Frame>
where
    R: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let until = tokio::time::Instant::now() + within;
    loop {
        match tokio::time::timeout_at(until, stream.next()).await.ok()?? {
            Ok(Message::Text(t)) => return serde_json::from_str(t.as_str()).ok(),
            Ok(Message::Ping(_) | Message::Pong(_)) => continue,
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_take_a_default_port() {
        assert_eq!(parse_address("192.168.68.20"), Some(("192.168.68.20".into(), 47960)));
        assert_eq!(parse_address(" http://tvl.local:47970/ "), Some(("tvl.local".into(), 47970)));
        assert_eq!(parse_address("[fe80::1]:47961"), Some(("fe80::1".into(), 47961)));
        assert_eq!(parse_address("fe80::1"), Some(("fe80::1".into(), 47960)));
        assert_eq!(parse_address("tvl:port"), None);
        assert_eq!(parse_address("  "), None);
    }

    #[test]
    fn tries_spread_out_up_to_half_a_minute() {
        assert!(between_tries(1) < Duration::from_millis(1300));
        assert!(between_tries(3) >= Duration::from_secs(4));
        assert!(between_tries(40) <= MOST_BETWEEN_TRIES + Duration::from_millis(250));
    }
}
