//! The TV PCs' sockets (`/api/agent`). A paired TV PC connects with its token and says hello; one
//! that isn't paired yet asks to join and shows a code until someone approves it with the PIN.
//! Then calls go both ways on the socket (a phone's remote key to the TV PC, the TV PC's Smart Home
//! screen to the server), and so do events.
//!
//! Every frame that comes in is handled on its own task, so a call that waits on another call
//! (the TV asks the assistant, which asks that same TV to open an app) never blocks the socket.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{header, HeaderMap};
use axum::response::Response;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tvl_protocol::wire::{Hello, Welcome};
use tvl_protocol::{topics, Frame, PROTOCOL_VERSION};

use crate::bus::{Envelope, Source};
use crate::ctx::Ctx;
use crate::router::{self, Caller};

/// How often the server pings a TV PC; two pings unanswered (plus a little) and it's gone.
const PING: Duration = Duration::from_secs(15);
const DEAD_AFTER: Duration = Duration::from_secs(35);
/// How long a TV PC has to say hello or ask to pair.
const FIRST_FRAME: Duration = Duration::from_secs(10);

#[derive(Debug, PartialEq)]
pub enum CallError {
    /// No socket from that TV PC, or it dropped while the call waited.
    Offline,
    /// It didn't answer in time.
    Timeout,
    /// It answered with an error.
    Failed(String),
}

struct Session {
    generation: u64,
    /// Frames to send, as JSON text.
    tx: mpsc::UnboundedSender<String>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>,
    /// Held while an ordered call (remote keys, text) is on its way, so they arrive as sent.
    order: tokio::sync::Mutex<()>,
    /// Cancelled when a newer socket from the same TV PC takes over.
    closed: CancellationToken,
}

impl Session {
    fn fail_all(&self, why: &str) {
        for (_, waiting) in self.pending.lock().drain() {
            let _ = waiting.send(Err(why.to_string()));
        }
    }
}

#[derive(Default)]
pub struct Agents {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    next: AtomicU64,
}

impl Agents {
    pub fn online(&self, device: &str) -> bool {
        self.sessions.lock().contains_key(device)
    }

    /// The newest socket of a TV PC wins: an older one (left half-open after the PC slept) is
    /// closed and its waiting calls fail at once.
    fn register(&self, device: &str, tx: mpsc::UnboundedSender<String>) -> Arc<Session> {
        let session = Arc::new(Session {
            generation: self.next.fetch_add(1, Ordering::Relaxed),
            tx,
            pending: Mutex::default(),
            order: tokio::sync::Mutex::new(()),
            closed: CancellationToken::new(),
        });
        if let Some(old) = self.sessions.lock().insert(device.to_string(), session.clone()) {
            old.closed.cancel();
            old.fail_all("The TV PC reconnected");
        }
        session
    }

    /// Closes TV PC `device`'s socket (it was removed from the house): it comes back, if at all,
    /// to be told to pair again.
    pub fn disconnect(&self, device: &str) {
        if let Some(session) = self.sessions.lock().get(device) {
            session.closed.cancel();
        }
    }

    /// The socket ended. True when it was still the TV PC's current one.
    fn unregister(&self, device: &str, session: &Session) -> bool {
        let mut sessions = self.sessions.lock();
        let current = sessions.get(device).is_some_and(|s| s.generation == session.generation);
        if current {
            sessions.remove(device);
        }
        drop(sessions);
        session.fail_all("The TV PC went offline");
        current
    }

    /// Runs `cmd` on TV PC `device` and waits for its answer. `from` names the screen that asked.
    /// `ordered` calls reach the TV PC one at a time, in the order they were made.
    pub async fn call(
        &self,
        device: &str,
        cmd: &str,
        args: Value,
        from: Option<&str>,
        timeout_ms: u32,
        ordered: bool,
    ) -> Result<Value, CallError> {
        let session = self.sessions.lock().get(device).cloned().ok_or(CallError::Offline)?;
        let _in_order = if ordered { Some(session.order.lock().await) } else { None };
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        session.pending.lock().insert(id, tx);
        let frame = Frame::Call { id, cmd: cmd.to_string(), args, from: from.map(str::to_string) };
        if session.tx.send(frame.to_text()).is_err() {
            session.pending.lock().remove(&id);
            return Err(CallError::Offline);
        }
        match tokio::time::timeout(Duration::from_millis(u64::from(timeout_ms)), rx).await {
            Ok(Ok(Ok(v))) => Ok(v),
            Ok(Ok(Err(e))) if e.starts_with("The TV PC ") => Err(CallError::Offline),
            Ok(Ok(Err(e))) => Err(CallError::Failed(e)),
            Ok(Err(_)) => Err(CallError::Offline),
            Err(_) => {
                session.pending.lock().remove(&id);
                Err(CallError::Timeout)
            }
        }
    }

    fn answered(&self, session: &Session, id: u64, outcome: Result<Value, String>) {
        if let Some(waiting) = session.pending.lock().remove(&id) {
            let _ = waiting.send(outcome);
        }
    }
}

pub async fn socket(ws: WebSocketUpgrade, State(ctx): State<Ctx>, headers: HeaderMap) -> Response {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t.trim().to_string());
    ws.on_upgrade(move |socket| run(socket, ctx, token))
}

type Sink = SplitSink<WebSocket, Message>;
type Stream = SplitStream<WebSocket>;

async fn send(sink: &mut Sink, frame: &Frame) -> bool {
    sink.send(Message::Text(frame.to_text().into())).await.is_ok()
}

async fn refuse(sink: &mut Sink, reason: &str) {
    tracing::info!("refused a TV PC: {reason}");
    let _ = send(sink, &Frame::Refused { reason: reason.to_string() }).await;
    let _ = sink.close().await;
}

/// The next frame, skipping pings; None on a closed socket, garbage or the deadline.
async fn next_frame(stream: &mut Stream, within: Duration) -> Option<Frame> {
    let deadline = tokio::time::Instant::now() + within;
    loop {
        match tokio::time::timeout_at(deadline, stream.next()).await.ok()?? {
            Ok(Message::Text(t)) => return serde_json::from_str(&t).ok(),
            Ok(Message::Ping(_) | Message::Pong(_)) => continue,
            _ => return None,
        }
    }
}

async fn run(socket: WebSocket, ctx: Ctx, token: Option<String>) {
    let (mut sink, mut stream) = socket.split();
    let device = match token {
        Some(t) => match ctx.admin.agent_for(&t) {
            Some(d) => d,
            None => return refuse(&mut sink, "This TV PC isn't paired with the server (any more): pair it again").await,
        },
        None => match pair(&ctx, &mut sink, &mut stream).await {
            Some(d) => d,
            None => return,
        },
    };
    let hello = match next_frame(&mut stream, FIRST_FRAME).await {
        Some(Frame::Hello(h)) => h,
        _ => return refuse(&mut sink, "Say hello first").await,
    };
    if hello.version != PROTOCOL_VERSION {
        let why = format!("Update this TV PC: the server speaks version {PROTOCOL_VERSION}, it speaks {}", hello.version);
        return refuse(&mut sink, &why).await;
    }
    if hello.device != device {
        return refuse(&mut sink, "That token belongs to another TV PC: pair it again").await;
    }
    serve(ctx, device, hello, sink, stream).await;
}

/// A TV PC without a token asks to join; it shows the code until someone approves it.
async fn pair(ctx: &Ctx, sink: &mut Sink, stream: &mut Stream) -> Option<String> {
    let (device, name, version) = match next_frame(stream, FIRST_FRAME).await {
        Some(Frame::Pair { device, name, version }) => (device, name, version),
        _ => {
            refuse(sink, "Pair this TV PC first").await;
            return None;
        }
    };
    if version != PROTOCOL_VERSION {
        refuse(sink, &format!("Update this TV PC: the server speaks version {PROTOCOL_VERSION}")).await;
        return None;
    }
    let (code, answer) = ctx.admin.request_pairing(&device, &name);
    tracing::info!(device, name, "a TV PC asks to pair: code {code}");
    ctx.bus.emit("pairing", &ctx.admin.pairings().len());
    if !send(sink, &Frame::PairCode { code }).await {
        return None;
    }
    // Wait for the answer, or for the TV PC to give up.
    let token = tokio::select! {
        a = answer => a.ok().flatten(),
        _ = async { while let Some(Ok(_)) = stream.next().await {} } => None,
        _ = ctx.shutdown.cancelled() => None,
    };
    ctx.bus.emit("pairing", &ctx.admin.pairings().len());
    let Some(token) = token else {
        refuse(sink, "Pairing denied").await;
        return None;
    };
    if let Err(e) = ctx.devices.add_tv(&device, &name) {
        refuse(sink, &e.to_string()).await;
        return None;
    }
    let ca_pem = ctx.ca.as_ref().map(|ca| ca.pem().to_string()).unwrap_or_default();
    if !send(sink, &Frame::Paired { token, ca_pem }).await {
        return None;
    }
    Some(device)
}

/// A server event as the TV PC reads it.
fn event_frame(e: &Envelope) -> String {
    format!(r#"{{"t":"event","name":{},"payload":{}}}"#, Value::String(e.event.to_string()), e.payload)
}

async fn serve(ctx: Ctx, device: String, hello: Hello, mut sink: Sink, mut stream: Stream) {
    if ctx.devices.get(&device).is_none() {
        return refuse(&mut sink, "This TV PC was forgotten: pair it again").await;
    }
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let session = ctx.agents.register(&device, tx.clone());
    ctx.devices.tv_online(&device, true, Some(hello.caps.clone()));
    if let Some(mac) = hello.mac.as_deref().filter(|m| !m.is_empty()) {
        ctx.set_agent_mac(&device, mac);
    }
    tracing::info!(device, name = hello.name, version = hello.app_version, "TV PC connected");
    let screen = ctx.devices.get(&device).unwrap_or_else(|| unreachable!("checked above"));
    let welcome = Frame::Welcome(Welcome {
        version: PROTOCOL_VERSION,
        screen,
        admin_pin: ctx.admin.pin(),
        https: ctx.https_port.get().copied(),
    });
    if !send(&mut sink, &welcome).await {
        return end(&ctx, &device, &session);
    }
    // The keys its media modules need (the settings event goes to TV PCs only).
    let keys = crate::settings::for_tvs(&ctx.settings.get());
    let _ = tx.send(Frame::Event { name: "settings".into(), payload: keys }.to_text());
    // Whatever rings on it now, even nothing (a stop it missed while away).
    crate::clock::resend(&ctx, &device);

    let mut events = ctx.bus.subscribe();
    let mut ping = tokio::time::interval(PING);
    let mut heard = Instant::now();
    loop {
        tokio::select! {
            _ = session.closed.cancelled() => break,
            _ = ctx.shutdown.cancelled() => break,
            out = rx.recv() => match out {
                Some(text) => {
                    if sink.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                None => break,
            },
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Text(t))) => {
                    heard = Instant::now();
                    received(&ctx, &device, &session, &t);
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => heard = Instant::now(),
            },
            ev = events.recv() => match ev {
                Ok(e) if e.source == Source::Server && topics::agent_receives(&e.event) => {
                    let _ = tx.send(event_frame(&e));
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let _ = tx.send(Frame::Event { name: "resync".into(), payload: Value::Null }.to_text());
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            },
            _ = ping.tick() => {
                if heard.elapsed() > DEAD_AFTER {
                    tracing::info!(device, "TV PC stopped answering");
                    break;
                }
                if sink.send(Message::Ping(Default::default())).await.is_err() {
                    break;
                }
            }
        }
    }
    let _ = sink.close().await;
    end(&ctx, &device, &session);
}

/// The socket is gone: its waiting calls fail now; the TV PC shows as off only if no new socket
/// arrives within a few seconds (a Wi-Fi hiccup shouldn't flicker every screen).
fn end(ctx: &Ctx, device: &str, session: &Session) {
    if !ctx.agents.unregister(device, session) {
        return;
    }
    crate::smarthome::forget_watcher(ctx, device);
    let (ctx, device) = (ctx.clone(), device.to_string());
    tokio::spawn(async move {
        tokio::time::sleep(ctx.config.offline_grace).await;
        if !ctx.agents.online(&device) {
            tracing::info!(device, "TV PC disconnected");
            ctx.devices.tv_online(&device, false, None);
        }
    });
}

fn received(ctx: &Ctx, device: &str, session: &Arc<Session>, text: &str) {
    let frame: Frame = match serde_json::from_str(text) {
        Ok(f) => f,
        Err(e) => return tracing::debug!(device, "unreadable frame: {e}"),
    };
    match frame {
        Frame::Call { id, cmd, args, .. } => {
            let (ctx, tx) = (ctx.clone(), session.tx.clone());
            let caller = Caller::agent(device);
            tokio::spawn(async move {
                let result = router::dispatch(&ctx, &caller, &cmd, args).await.map_err(|f| f.to_string());
                let _ = tx.send(Frame::reply(id, result).to_text());
            });
        }
        Frame::Reply { id, ok, err } => ctx.agents.answered(session, id, Frame::outcome(ok, err)),
        Frame::Event { name, payload } => {
            if topics::for_followers(&name) {
                ctx.bus.emit_from(device, &name, &payload);
            } else if topics::FROM_AGENT_TO_SERVER.contains(&name.as_str()) {
                if name == "feed.sports" {
                    crate::calendar::feed(ctx, payload.clone());
                }
                ctx.agent_event(device, &name, payload);
            } else {
                tracing::debug!(device, event = name, "event a TV PC may not send, dropped");
            }
        }
        _ => tracing::debug!(device, "frame out of place, dropped"),
    }
}
