//! A server on a free port, TV PCs played by a fake agent, and browsers played by a plain HTTP
//! client and WebSocket.

#![allow(dead_code)]

use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tvl_protocol::wire::Hello;
use tvl_protocol::{Caps, Frame, PROTOCOL_VERSION};
use tvl_server::{Config, Running};

pub async fn server() -> Running {
    let dir = tvl_kit::testing::temp_dir("server");
    tvl_server::start(Config::for_tests(&dir)).await.expect("the server starts")
}

/// A TV PC registered and paired, as if someone had approved it: its token.
pub fn paired_tv(s: &Running, id: &str, name: &str, room: Option<&str>) -> String {
    s.ctx.devices.add_tv(id, name).unwrap();
    s.ctx.devices.put(id, name, room).unwrap();
    s.ctx.admin.pair_now(id).unwrap()
}

pub type Handler = Arc<dyn Fn(Arc<Agent>, String, Value) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send>> + Send + Sync>;

/// A TV PC's agent: answers the server's calls with `handler`, records them, and can call the
/// server and send events.
pub struct Agent {
    pub device: String,
    out: mpsc::UnboundedSender<Message>,
    pub calls: Mutex<Vec<(String, Value, Option<String>)>>,
    pub events: Mutex<Vec<(String, Value)>>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>,
    next: AtomicU64,
    pub closed: tokio_util::sync::CancellationToken,
}

impl Agent {
    pub fn send_event(&self, name: &str, payload: Value) {
        let _ = self.out.send(Message::Text(Frame::Event { name: name.into(), payload }.to_text().into()));
    }

    /// Calls the server, as the TV UI's server-owned commands do.
    pub async fn call(&self, cmd: &str, args: Value) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1_000_000;
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        let frame = Frame::Call { id, cmd: cmd.into(), args, from: None };
        let _ = self.out.send(Message::Text(frame.to_text().into()));
        tokio::time::timeout(Duration::from_secs(5), rx).await.map_err(|_| "timed out".to_string())?.map_err(|_| "gone".to_string())?
    }

    pub fn calls_of(&self, cmd: &str) -> Vec<(Value, Option<String>)> {
        self.calls.lock().iter().filter(|(c, _, _)| c == cmd).map(|(_, a, f)| (a.clone(), f.clone())).collect()
    }

    pub fn close(&self) {
        let _ = self.out.send(Message::Close(None));
    }
}

pub fn ok_handler() -> Handler {
    Arc::new(|_agent, _cmd, _args| Box::pin(async { Ok(json!(null)) }))
}

async fn open(addr: SocketAddr, token: Option<&str>) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let mut req = format!("ws://{addr}/api/agent").into_client_request().unwrap();
    if let Some(t) = token {
        req.headers_mut().insert("Authorization", format!("Bearer {t}").parse().unwrap());
    }
    tokio_tungstenite::connect_async(req).await.expect("the agent socket opens").0
}

fn text_frame(m: Message) -> Option<Frame> {
    match m {
        Message::Text(t) => serde_json::from_str(t.as_str()).ok(),
        _ => None,
    }
}

pub fn hello(device: &str, version: u32) -> Frame {
    Frame::Hello(Hello {
        version,
        device: device.into(),
        name: device.into(),
        caps: Caps { remote: true, volume: true, ring: true, ..Default::default() },
        mac: Some("00:11:22:33:44:55".into()),
        app_version: "test".into(),
    })
}

/// Connects TV PC `device` with `token`, says hello and expects the welcome.
pub async fn connect(addr: SocketAddr, device: &str, token: &str, handler: Handler) -> Arc<Agent> {
    let mut ws = open(addr, Some(token)).await;
    ws.send(Message::Text(hello(device, PROTOCOL_VERSION).to_text().into())).await.unwrap();
    let welcome = loop {
        match text_frame(ws.next().await.expect("an answer").unwrap()) {
            Some(f) => break f,
            None => continue,
        }
    };
    assert!(matches!(welcome, Frame::Welcome(_)), "expected a welcome, got {welcome:?}");
    run(ws, device, handler)
}

/// The first frame the server sends after `first` (a refusal, a pairing code…).
pub async fn answer_to(addr: SocketAddr, token: Option<&str>, first: Frame) -> Frame {
    let mut ws = open(addr, token).await;
    ws.send(Message::Text(first.to_text().into())).await.unwrap();
    loop {
        match tokio::time::timeout(Duration::from_secs(5), ws.next()).await.expect("an answer in time") {
            Some(Ok(m)) => {
                if let Some(f) = text_frame(m) {
                    return f;
                }
            }
            other => panic!("socket ended: {other:?}"),
        }
    }
}

/// An agent that isn't paired: asks, returns its pairing code and the socket to wait on.
pub async fn ask_to_pair(
    addr: SocketAddr,
    device: &str,
) -> (String, tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>) {
    let mut ws = open(addr, None).await;
    let pair = Frame::Pair { device: device.into(), name: format!("{device} PC"), version: PROTOCOL_VERSION };
    ws.send(Message::Text(pair.to_text().into())).await.unwrap();
    loop {
        if let Some(Frame::PairCode { code }) = text_frame(ws.next().await.unwrap().unwrap()) {
            return (code, ws);
        }
    }
}

fn run(
    ws: tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    device: &str,
    handler: Handler,
) -> Arc<Agent> {
    let (mut write, mut read) = ws.split();
    let (out, mut outgoing) = mpsc::unbounded_channel::<Message>();
    let agent = Arc::new(Agent {
        device: device.into(),
        out,
        calls: Mutex::default(),
        events: Mutex::default(),
        pending: Mutex::default(),
        next: AtomicU64::new(0),
        closed: tokio_util::sync::CancellationToken::new(),
    });
    let a = agent.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                m = outgoing.recv() => match m {
                    Some(m) => {
                        let close = matches!(m, Message::Close(_));
                        if write.send(m).await.is_err() || close {
                            break;
                        }
                    }
                    None => break,
                },
                m = read.next() => match m {
                    Some(Ok(Message::Text(t))) => match serde_json::from_str::<Frame>(t.as_str()) {
                        Ok(Frame::Call { id, cmd, args, from }) => {
                            a.calls.lock().push((cmd.clone(), args.clone(), from));
                            let (a2, h) = (a.clone(), handler.clone());
                            tokio::spawn(async move {
                                let r = h(a2.clone(), cmd, args).await;
                                let _ = a2.out.send(Message::Text(Frame::reply(id, r).to_text().into()));
                            });
                        }
                        Ok(Frame::Reply { id, ok, err }) => {
                            if let Some(tx) = a.pending.lock().remove(&id) {
                                let _ = tx.send(Frame::outcome(ok, err));
                            }
                        }
                        Ok(Frame::Event { name, payload }) => a.events.lock().push((name, payload)),
                        _ => {}
                    },
                    Some(Ok(_)) => {}
                    _ => break,
                },
            }
        }
        a.closed.cancel();
    });
    agent
}

/// A page's call: the status and the JSON answer.
pub async fn post(addr: SocketAddr, cmd: &str, args: Value, headers: &[(&str, &str)]) -> (u16, Value) {
    let mut req = reqwest::Client::new().post(format!("http://{addr}/api/{cmd}")).header("Content-Type", "application/json").json(&args);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let r = req.send().await.expect("the server answers");
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Value::Null))
}

pub async fn post_as(addr: SocketAddr, screen: &str, cmd: &str, args: Value) -> (u16, Value) {
    post(addr, cmd, args, &[("X-TVL-Screen", screen)]).await
}

/// A companion page's event socket; collects the events it hears.
pub struct Page {
    pub events: Arc<Mutex<Vec<(String, Value)>>>,
}

pub async fn page(addr: SocketAddr, screen: &str) -> Page {
    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/api/events?screen={screen}")).await.expect("events socket");
    let (_write, mut read) = ws.split();
    let events = Arc::new(Mutex::new(Vec::new()));
    let e = events.clone();
    tokio::spawn(async move {
        let _keep = _write;
        while let Some(Ok(m)) = read.next().await {
            if let Message::Text(t) = m {
                if let Ok(v) = serde_json::from_str::<Value>(t.as_str()) {
                    e.lock().push((v["event"].as_str().unwrap_or("").to_string(), v["payload"].clone()));
                }
            }
        }
    });
    Page { events }
}

impl Page {
    pub fn heard(&self, event: &str) -> Vec<Value> {
        self.events.lock().iter().filter(|(e, _)| e == event).map(|(_, p)| p.clone()).collect()
    }
}

/// Waits up to five seconds for `cond` (a timer has to run out first).
pub async fn eventually_long(mut cond: impl FnMut() -> bool) -> bool {
    for _ in 0..250 {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    cond()
}

/// Waits up to two seconds for `cond`.
pub async fn eventually(mut cond: impl FnMut() -> bool) -> bool {
    for _ in 0..100 {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    cond()
}
