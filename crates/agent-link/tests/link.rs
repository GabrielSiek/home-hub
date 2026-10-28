//! The link against the real server, in-process: pairing, coming back with the token, calls and
//! events both ways, being removed from the house, wss:// after pairing, the probe.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tvl_agent_link::{Host, Identity, Link, Reply, Status, OFFLINE};
use tvl_protocol::Caps;
use tvl_server::{Config, Running};

#[derive(Default)]
struct Tv {
    calls: Mutex<Vec<(String, Value, Option<String>)>>,
    events: Mutex<Vec<(String, Value)>>,
    statuses: Mutex<Vec<Status>>,
    connected: AtomicUsize,
}

impl Host for Tv {
    fn call(&self, cmd: String, args: Value, from: Option<String>) -> Reply {
        self.calls.lock().push((cmd.clone(), args, from));
        Box::pin(async move { Ok(json!({ "did": cmd })) })
    }
    fn event(&self, name: String, payload: Value) {
        self.events.lock().push((name, payload));
    }
    fn status(&self, status: &Status) {
        self.statuses.lock().push(status.clone());
    }
    fn connected(&self) {
        self.connected.fetch_add(1, Ordering::SeqCst);
    }
}

impl Tv {
    fn called(&self, cmd: &str) -> bool {
        self.calls.lock().iter().any(|(c, _, _)| c == cmd)
    }
    fn heard(&self, event: &str) -> bool {
        self.events.lock().iter().any(|(e, _)| e == event)
    }
    fn code(&self) -> Option<String> {
        self.statuses.lock().iter().rev().find_map(|s| match s {
            Status::Pairing { code, .. } => Some(code.clone()),
            _ => None,
        })
    }
}

fn me(device: &str) -> Identity {
    Identity {
        device: device.into(),
        name: format!("{device} TV"),
        caps: Caps { remote: true, volume: true, ring: true, ..Default::default() },
        mac: Some("00:11:22:33:44:55".into()),
        app_version: "test".into(),
    }
}

async fn server_with(change: impl FnOnce(&mut Config)) -> Running {
    let dir = tvl_kit::testing::temp_dir("link-server");
    let mut config = Config::for_tests(&dir);
    change(&mut config);
    tvl_server::start(config).await.expect("the server starts")
}

/// A TV PC's link to `s`, running.
fn tv_link(s: &Running, dir: &std::path::Path, device: &str) -> (Link, Arc<Tv>) {
    let tv = Arc::new(Tv::default());
    let link = Link::new(dir, me(device), tv.clone(), Vec::new());
    link.set_address(Some(&format!("127.0.0.1:{}", s.http.port()))).unwrap();
    tokio::spawn(link.clone().run());
    (link, tv)
}

/// A TV PC the house already knows, with its token in `dir`.
fn known_tv(s: &Running, device: &str, room: &str) -> std::path::PathBuf {
    s.ctx.devices.add_tv(device, &format!("{device} TV")).unwrap();
    s.ctx.devices.put(device, &format!("{device} TV"), Some(room)).unwrap();
    let token = s.ctx.admin.pair_now(device).unwrap();
    let dir = tvl_kit::testing::temp_dir("link-tv");
    std::fs::write(dir.join("link.json"), json!({ "token": token }).to_string()).unwrap();
    dir
}

async fn until(within: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + within;
    while Instant::now() < end {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    cond()
}

fn online(link: &Link) -> bool {
    matches!(link.status(), Status::Online { .. })
}

#[tokio::test]
async fn pairs_once_then_comes_back_with_its_token() {
    let s = server_with(|_| {}).await;
    let dir = tvl_kit::testing::temp_dir("link-tv");
    let (link, tv) = tv_link(&s, &dir, "pc-quarto");
    assert!(until(Duration::from_secs(5), || tv.code().is_some()).await, "the TV shows a code: {:?}", tv.statuses.lock());
    s.ctx.admin.approve(&tv.code().unwrap()).unwrap();
    assert!(until(Duration::from_secs(5), || online(&link)).await, "{:?}", link.status());
    assert!(link.is_paired());
    assert!(until(Duration::from_secs(2), || s.ctx.devices.get("pc-quarto").is_some_and(|d| d.online)).await);
    let Status::Online { admin_pin, secure, .. } = link.status() else { unreachable!() };
    assert_eq!(admin_pin, s.ctx.admin.pin());
    assert!(!secure, "no HTTPS on this server");

    // Started again (the PC rebooted): straight back in, no new code.
    link.stop();
    assert!(until(Duration::from_secs(2), || !s.ctx.devices.get("pc-quarto").unwrap().online).await);
    let tv2 = Arc::new(Tv::default());
    let again = Link::new(&dir, me("pc-quarto"), tv2.clone(), Vec::new());
    tokio::spawn(again.clone().run());
    assert!(until(Duration::from_secs(5), || online(&again)).await, "{:?}", again.status());
    assert!(tv2.code().is_none(), "it didn't ask to pair again");
    assert_eq!(tv2.connected.load(Ordering::SeqCst), 1);
    again.stop();
    s.stop();
}

#[tokio::test]
async fn calls_and_events_go_both_ways() {
    let s = server_with(|_| {}).await;
    let dir = known_tv(&s, "pc-quarto", "Quarto");
    let (link, tv) = tv_link(&s, &dir, "pc-quarto");
    assert!(until(Duration::from_secs(5), || online(&link)).await, "{:?}", link.status());

    // A phone's remote key reaches this TV PC and its answer goes back.
    let r = reqwest::Client::new()
        .post(format!("http://{}/api/tv_now", s.http))
        .header("Content-Type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.json::<Value>().await.unwrap(), json!({ "did": "tv_now" }));

    // The TV's own UI asks the server (a server-owned command), and hears what it changed.
    let clock = link.call("clock_timer_start", json!({ "ms": 60_000, "label": "Pasta" })).await.unwrap();
    assert_eq!(clock["timers"][0]["label"], "Pasta");
    assert!(until(Duration::from_secs(2), || tv.heard("clock")).await);
    // It rings here: the server sends this TV PC what rings on it.
    assert!(until(Duration::from_secs(2), || tv.called("ring.set")).await);
    // The household keys its media modules need.
    assert!(tv.heard("settings"));

    // What the TV says reaches the pages following it.
    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/api/events", s.http)).await.unwrap();
    let (_w, mut read) = ws.split();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(link.event("tv", json!({ "volume": 30 })));
    let heard = tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(Ok(m)) = read.next().await {
            if let Ok(v) = serde_json::from_str::<Value>(m.to_text().unwrap_or("")) {
                if v["event"] == "tv" {
                    return v["payload"].clone();
                }
            }
        }
        Value::Null
    })
    .await
    .unwrap();
    assert_eq!(heard, json!({ "volume": 30 }));
    link.stop();
    s.stop();
}

#[tokio::test]
async fn a_tv_removed_from_the_house_asks_to_pair_again() {
    let s = server_with(|_| {}).await;
    let dir = known_tv(&s, "pc-sala", "Sala");
    let (link, tv) = tv_link(&s, &dir, "pc-sala");
    assert!(until(Duration::from_secs(5), || online(&link)).await, "{:?}", link.status());
    // Removed with the admin PIN (a paired TV PC counts as admin, as here). Removing itself, its
    // socket closes as the answer goes out, so the answer may not arrive; the removal does.
    let _ = link.call("smarthome_forget_screen", json!({ "id": "pc-sala" })).await;
    assert!(until(Duration::from_secs(8), || tv.code().is_some()).await, "{:?}", tv.statuses.lock());
    assert!(!link.is_paired());
    link.stop();
    s.stop();
}

#[tokio::test]
async fn while_the_server_is_away_calls_fail_fast() {
    let tv = Arc::new(Tv::default());
    let dir = tvl_kit::testing::temp_dir("link-tv");
    let link = Link::new(&dir, me("pc-x"), tv, Vec::new());
    // Nothing listens there.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    link.set_address(Some(&format!("127.0.0.1:{port}"))).unwrap();
    tokio::spawn(link.clone().run());
    // Right after starting, a call waits a moment for the link…
    let first = Instant::now();
    assert_eq!(link.call("clock_get", json!({})).await.unwrap_err(), OFFLINE);
    assert!(first.elapsed() >= Duration::from_millis(1500));
    // …and after that fails at once.
    let next = Instant::now();
    assert_eq!(link.call("clock_get", json!({})).await.unwrap_err(), OFFLINE);
    assert!(next.elapsed() < Duration::from_millis(200));
    assert!(!link.event("tv", json!({})));
    // (Windows takes a couple of seconds to give up on a closed port.)
    assert!(until(Duration::from_secs(5), || matches!(link.status(), Status::Offline { .. })).await, "{:?}", link.status());
    link.stop();
}

#[tokio::test]
async fn once_paired_it_connects_over_tls_trusting_the_home_authority() {
    let https = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let s = server_with(|c| c.https_port = Some(https)).await;
    assert!(s.https.is_some());
    let dir = tvl_kit::testing::temp_dir("link-tv");
    let (link, tv) = tv_link(&s, &dir, "pc-quarto");
    assert!(until(Duration::from_secs(5), || tv.code().is_some()).await);
    s.ctx.admin.approve(&tv.code().unwrap()).unwrap();
    // Paired over ws://; told the HTTPS port in the welcome.
    assert!(until(Duration::from_secs(5), || online(&link)).await, "{:?}", link.status());
    link.stop();
    let again = Link::new(&dir, me("pc-quarto"), Arc::new(Tv::default()), Vec::new());
    tokio::spawn(again.clone().run());
    assert!(until(Duration::from_secs(5), || online(&again)).await, "{:?}", again.status());
    let Status::Online { secure, fingerprint, .. } = again.status() else { unreachable!() };
    assert!(secure, "wss:// the second time");
    assert_eq!(fingerprint, s.ctx.ca.as_ref().map(|ca| ca.fingerprint()));
    again.stop();
    s.stop();
}

#[tokio::test]
async fn the_probe_finds_the_server() {
    let probe = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let s = server_with(|c| c.probe_port = Some(probe)).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let found = tvl_agent_link::probe(&[probe], Duration::from_secs(2)).await.expect("an answer");
    assert_eq!((found.host.as_str(), found.http), ("127.0.0.1", s.http.port()));
    s.stop();
}
