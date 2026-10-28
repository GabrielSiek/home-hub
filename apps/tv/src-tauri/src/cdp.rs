//! Drives the launched browser through the DevTools protocol. The pad goes to TV mode
//! (tvnav.js, injected into every page) or straight to the page as keys, without needing the
//! browser window to have focus. Apps with a `user_agent` also get a matching
//! navigator.userAgentData: Edge's --user-agent flag leaves it saying "Edge on Windows",
//! which YouTube's TV interface checks before letting a browser in.

use std::collections::{HashMap, HashSet};
use std::net::TcpListener;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::config::Nav;
use crate::input::{Action, Extra};
use crate::state::AppState;

/// TV mode, with the launcher's player controls set up first for tvnav.js to drive: each
/// service's adapter (player/<service>.js), then the overlay that uses them.
const TV_SCRIPT: &str = concat!(
    include_str!("player/netflix.js"),
    "\n",
    include_str!("player/prime.js"),
    "\n",
    include_str!("player/hbomax.js"),
    "\n",
    include_str!("player/disney.js"),
    "\n",
    include_str!("player/overlay.js"),
    "\n",
    include_str!("tvnav.js"),
);
/// Players' fullscreen without the browser's "press Esc to exit" bubble.
const FULLSCREEN_SCRIPT: &str = include_str!("player/fullscreen.js");
const PORT_WAIT: Duration = Duration::from_secs(20);

/// What was opened, and how to drive it.
pub struct Page {
    pub url: String,
    pub nav: Nav,
    pub user_agent: Option<String>,
    /// Playing from the launcher's copy of a streaming service: watch.js, set up for what to
    /// play, goes in every page. Its reports are collected and handed to watch/mod.rs.
    pub watch: Option<String>,
    /// A live sports channel from the Sports tab: the overlay (player/livetv.js) goes in every
    /// page, and its reports go to sports/mod.rs.
    pub live: Option<String>,
}

/// How often watch.js's reports are collected.
const POLL_EVERY: Duration = Duration::from_millis(1000);

/// Takes watch.js's queued reports out of the page, as one JSON array.
const POLL_SCRIPT: &str =
    "(() => { const q = window.__tvlQueue || []; window.__tvlQueue = []; return JSON.stringify(q); })()";

/// The page calls this (a DevTools binding) when a report can't wait for the next collection.
const NOW_BINDING: &str = "__tvlNow";

/// The page asks for a cookie it can't read itself (HttpOnly, like Prime Video's profile), by
/// name: the answer comes back through `window.__tvlCookie(name, value)`.
const COOKIE_BINDING: &str = "__tvlCookieAsk";

/// Mutes and pauses every video and audio in the page, shadow roots included.
const SILENCE_SCRIPT: &str = "(() => { const quiet = (root) => {     for (const m of root.querySelectorAll('video, audio')) { m.muted = true; m.pause(); }     for (const el of root.querySelectorAll('*')) if (el.shadowRoot) quiet(el.shadowRoot); };     quiet(document); })()";

/// What the page asked of the launcher.
#[derive(Debug, Clone, PartialEq)]
pub enum PageEvent {
    /// A watch.js report (JSON), for watch::on_report.
    Report(String),
    /// TV mode asked to leave: Back with nothing left to go back to while playing.
    Close,
}

/// A pad input for the browser, a script to run in its page, or the request to shut it down.
#[derive(Debug, Clone)]
pub enum Cmd {
    Action(Action),
    /// The left stick's up or down: TV mode can tell it from the D-pad's (volume in a player).
    Stick(Action),
    Extra(Extra),
    /// Runs in the page with the pad (the score overlay over a sports broadcast).
    Eval(String),
    Close,
}

pub struct Link {
    tx: mpsc::UnboundedSender<Cmd>,
}

/// A free local port for the browser's DevTools.
/// Not --remote-debugging-port=0: with that, Chromium sets navigator.webdriver (it's how
/// automation tools start it), and sign-in pages such as Twitch's and Spotify's refuse the browser.
pub fn free_port() -> Option<u16> {
    TcpListener::bind("127.0.0.1:0").ok()?.local_addr().ok().map(|a| a.port())
}

/// Connects to the browser that was just started with DevTools on `port`, in the background.
pub fn connect(app: &AppHandle, port: u16, page: Page) {
    let (tx, rx) = mpsc::unbounded_channel();
    *app.state::<AppState>().cdp.lock().unwrap() = Some(Link { tx });
    let http = app.state::<AppState>().http.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let on_event = |e: PageEvent| match e {
            PageEvent::Report(r) if crate::sports::is_live_report(&r) => crate::sports::on_live_report(&app, &r),
            PageEvent::Report(r) => crate::watch::on_report(&app, &r),
            PageEvent::Close => crate::watch::close_soon(&app),
        };
        if let Err(e) = run(&http, port, page, rx, on_event).await {
            eprintln!("browser control: {e}");
        }
    });
}

/// Lets go of the browser, asking it to shut down first. True when the request went out.
/// A clean exit is what saves the profile: Chromium writes localStorage (where YouTube's TV
/// interface keeps its sign-in) and cookies lazily, and a killed browser loses what's pending.
pub fn disconnect(st: &AppState) -> bool {
    st.cdp
        .lock()
        .unwrap()
        .take()
        .is_some_and(|l| l.tx.send(Cmd::Close).is_ok())
}

/// Hands a pad input to the browser. False when no browser is under control,
/// and the caller falls back to OS-level keys.
pub fn send(app: &AppHandle, cmd: Cmd) -> bool {
    app.state::<AppState>()
        .cdp
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|l| l.tx.send(cmd).is_ok())
}

async fn run(
    http: &reqwest::Client,
    port: u16,
    page: Page,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
    on_event: impl Fn(PageEvent),
) -> Result<(), String> {
    let from = std::time::Instant::now();
    let url = browser_ws(http, port).await?;
    eprintln!("browser control: DevTools up after {:.1}s", from.elapsed().as_secs_f64());
    let (ws, _) = tokio_tungstenite::connect_async(url.as_str())
        .await
        .map_err(|e| e.to_string())?;
    let (mut sink, mut stream) = ws.split();

    // Presses made while the browser was starting are stale by now.
    while rx.try_recv().is_ok() {}

    let mut b = Browser::new(page);
    b.start();
    let mut poll = tokio::time::interval(POLL_EVERY);
    loop {
        for m in std::mem::take(&mut b.outbox) {
            sink.send(Message::Text(m.into())).await.map_err(|e| e.to_string())?;
        }
        for e in std::mem::take(&mut b.events) {
            on_event(e);
        }
        tokio::select! {
            _ = poll.tick() => b.poll(),
            msg = stream.next() => match msg {
                Some(Ok(Message::Text(t))) => b.on_message(t.as_str()),
                Some(Ok(Message::Close(_))) | None => return Ok(()),
                Some(Err(e)) => return Err(e.to_string()),
                Some(Ok(_)) => {}
            },
            // None: the app was closed or replaced; dropping the socket lets go of the browser.
            cmd = rx.recv() => match cmd {
                Some(c) => b.command(c),
                None => return Ok(()),
            },
        }
    }
}

/// The browser's DevTools WebSocket, once it is listening.
pub(crate) async fn browser_ws(http: &reqwest::Client, port: u16) -> Result<String, String> {
    let step = Duration::from_millis(150);
    let mut waited = Duration::ZERO;
    while waited < PORT_WAIT {
        let version = http
            .get(format!("http://127.0.0.1:{port}/json/version"))
            .timeout(Duration::from_secs(1))
            .send()
            .await;
        if let Ok(r) = version {
            if let Some(url) = r.json::<Value>().await.ok().and_then(|v| ws_url(&v, port)) {
                return Ok(url);
            }
        }
        tokio::time::sleep(step).await;
        waited += step;
    }
    Err("the browser didn't open its DevTools port".into())
}

/// The browser-level socket from /json/version, only if it is the local one we asked for.
fn ws_url(version: &Value, port: u16) -> Option<String> {
    let url = version["webSocketDebuggerUrl"].as_str()?;
    url.starts_with(&format!("ws://127.0.0.1:{port}/devtools/browser/"))
        .then(|| url.to_string())
}

/// The protocol state for one browser. Outgoing messages collect in `outbox`.
struct Browser {
    page: Page,
    next_id: u64,
    /// Replies we act on: TV-mode presses, by request id.
    pending: HashMap<u64, Press>,
    /// Page sessions, most recently attached last: that one gets the pad.
    sessions: Vec<(String, String)>,
    attaching: HashSet<String>,
    navigated: bool,
    outbox: Vec<String>,
    /// For the caller: what the page asked for.
    events: Vec<PageEvent>,
    /// Report collections in flight, by request id: the session asked.
    polls: HashMap<u64, String>,
    /// Cookie lookups in flight, by request id: the session that asked, and the cookie's name.
    cookie_asks: HashMap<u64, (String, String)>,
}

struct Press {
    session: String,
    fallback: &'static str,
}

impl Browser {
    fn new(page: Page) -> Self {
        Self {
            page,
            next_id: 0,
            pending: HashMap::new(),
            sessions: Vec::new(),
            attaching: HashSet::new(),
            navigated: false,
            outbox: Vec::new(),
            events: Vec::new(),
            polls: HashMap::new(),
            cookie_asks: HashMap::new(),
        }
    }

    fn call(&mut self, session: Option<&str>, method: &str, params: Value) -> u64 {
        self.next_id += 1;
        let mut m = json!({ "id": self.next_id, "method": method, "params": params });
        if let Some(s) = session {
            m["sessionId"] = json!(s);
        }
        self.outbox.push(m.to_string());
        self.next_id
    }

    /// Reports the pages already open and every one opened later (pop-ups, sign-ins).
    fn start(&mut self) {
        self.call(None, "Target.setDiscoverTargets", json!({ "discover": true }));
    }

    fn on_message(&mut self, text: &str) {
        let Ok(v) = serde_json::from_str::<Value>(text) else {
            return;
        };
        if let Some(id) = v["id"].as_u64() {
            if let Some(p) = self.pending.remove(&id) {
                self.on_press_reply(p, &v);
            } else if let Some(session) = self.polls.remove(&id) {
                self.on_poll_reply(&session, &v);
            } else if let Some((session, name)) = self.cookie_asks.remove(&id) {
                self.on_cookie_reply(&session, &name, &v);
            }
            return;
        }
        let params = &v["params"];
        match v["method"].as_str() {
            Some("Target.targetCreated") => {
                let info = &params["targetInfo"];
                if let (Some("page"), Some(id)) = (info["type"].as_str(), info["targetId"].as_str()) {
                    if self.attaching.insert(id.to_string()) {
                        self.call(None, "Target.attachToTarget", json!({ "targetId": id, "flatten": true }));
                    }
                }
            }
            Some("Target.attachedToTarget") => {
                let (Some(session), Some(target)) =
                    (params["sessionId"].as_str(), params["targetInfo"]["targetId"].as_str())
                else {
                    return;
                };
                if params["targetInfo"]["type"].as_str() == Some("page") {
                    self.on_page(session.to_string(), target.to_string());
                }
            }
            Some("Runtime.bindingCalled") if params["name"] == NOW_BINDING => {
                if let Some(session) = v["sessionId"].as_str() {
                    self.collect(session.to_string());
                }
            }
            Some("Runtime.bindingCalled") if params["name"] == COOKIE_BINDING => {
                if let (Some(session), Some(name)) = (v["sessionId"].as_str(), params["payload"].as_str()) {
                    // The browser's cookie jar, HttpOnly ones included.
                    let id = self.call(None, "Storage.getCookies", json!({}));
                    self.cookie_asks.insert(id, (session.to_string(), name.to_string()));
                }
            }
            Some("Target.detachedFromTarget") => {
                let session = params["sessionId"].as_str().unwrap_or_default();
                self.sessions.retain(|(s, _)| s != session);
            }
            Some("Target.targetDestroyed") => {
                let target = params["targetId"].as_str().unwrap_or_default();
                self.attaching.remove(target);
                self.sessions.retain(|(_, t)| t != target);
            }
            _ => {}
        }
    }

    fn on_page(&mut self, session: String, target: String) {
        let s = Some(session.as_str());
        if let Some(ua) = self.page.user_agent.clone() {
            self.call(
                s,
                "Emulation.setUserAgentOverride",
                json!({ "userAgent": ua, "userAgentMetadata": tv_metadata() }),
            );
        }
        if self.page.nav == Nav::Tv || self.page.watch.is_some() || self.page.live.is_some() {
            self.call(s, "Page.enable", json!({}));
            self.call(s, "Page.addScriptToEvaluateOnNewDocument", json!({ "source": FULLSCREEN_SCRIPT }));
            self.call(s, "Runtime.evaluate", json!({ "expression": FULLSCREEN_SCRIPT }));
        }
        if let Some(script) = self.page.watch.clone().or_else(|| self.page.live.clone()) {
            // Without the Runtime domain on, the bindings miss the documents loaded later.
            self.call(s, "Runtime.enable", json!({}));
            self.call(s, "Runtime.addBinding", json!({ "name": NOW_BINDING }));
            self.call(s, "Runtime.addBinding", json!({ "name": COOKIE_BINDING }));
            self.call(s, "Page.addScriptToEvaluateOnNewDocument", json!({ "source": script }));
            self.call(s, "Runtime.evaluate", json!({ "expression": script }));
        }
        if self.page.nav == Nav::Tv {
            self.call(s, "Page.addScriptToEvaluateOnNewDocument", json!({ "source": TV_SCRIPT }));
            // The page that's already loaded doesn't get new-document scripts.
            self.call(s, "Runtime.evaluate", json!({ "expression": TV_SCRIPT }));
        }
        // The first page loaded before the TV identity was in place: load it again with it.
        if self.page.user_agent.is_some() && !self.navigated {
            self.navigated = true;
            let url = self.page.url.clone();
            self.call(s, "Page.navigate", json!({ "url": url }));
        }
        self.sessions.push((session, target));
    }

    fn command(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Close => {
                // Quiet first: the launcher is already back in front while the browser shuts down.
                for session in self.sessions.iter().map(|(s, _)| s.clone()).collect::<Vec<_>>() {
                    self.call(Some(&session), "Runtime.evaluate", json!({ "expression": SILENCE_SCRIPT }));
                }
                self.call(None, "Browser.close", json!({}));
                return;
            }
            Cmd::Eval(script) => {
                if let Some(session) = self.sessions.last().map(|(s, _)| s.clone()) {
                    self.call(Some(&session), "Runtime.evaluate", json!({ "expression": script }));
                }
                return;
            }
            Cmd::Action(_) | Cmd::Stick(_) | Cmd::Extra(_) => {}
        }
        let Some((name, fallback)) = press_names(cmd) else {
            return;
        };
        let Some(session) = self.sessions.last().map(|(s, _)| s.clone()) else {
            return;
        };
        match self.page.nav {
            // A live channel's overlay (CazéTV on the YouTube TV app) answers first; what it
            // leaves goes to the page as the key.
            Nav::Native if self.page.live.is_some() => {
                let id = self.call(
                    Some(&session),
                    "Runtime.evaluate",
                    json!({
                        "expression": format!("window.__tvlLive ? window.__tvlLive.press('{name}') : false"),
                        "returnByValue": true,
                    }),
                );
                self.pending.insert(id, Press { session, fallback });
            }
            Nav::Native => self.key(&session, fallback),
            Nav::Tv => {
                let id = self.call(
                    Some(&session),
                    "Runtime.evaluate",
                    json!({
                        "expression": format!("window.__tvnav ? window.__tvnav.press('{name}') : false"),
                        "returnByValue": true,
                    }),
                );
                self.pending.insert(id, Press { session, fallback });
            }
        }
    }

    /// Collects watch.js's (or the live overlay's) reports from the page with the pad.
    fn poll(&mut self) {
        if self.page.watch.is_none() && self.page.live.is_none() {
            return;
        }
        if let Some(session) = self.sessions.last().map(|(s, _)| s.clone()) {
            self.collect(session);
        }
    }

    /// Collects the reports queued in one page.
    fn collect(&mut self, session: String) {
        // One collection at a time: a page that doesn't answer doesn't pile them up.
        if self.polls.values().any(|s| *s == session) {
            return;
        }
        let id = self.call(
            Some(&session),
            "Runtime.evaluate",
            json!({ "expression": POLL_SCRIPT, "returnByValue": true }),
        );
        self.polls.insert(id, session);
    }

    fn on_cookie_reply(&mut self, session: &str, name: &str, v: &Value) {
        let value = v["result"]["cookies"]
            .as_array()
            .and_then(|l| l.iter().find(|c| c["name"] == name))
            .and_then(|c| c["value"].as_str());
        let expression = format!("window.__tvlCookie && window.__tvlCookie({}, {})", json!(name), json!(value));
        self.call(Some(session), "Runtime.evaluate", json!({ "expression": expression }));
    }

    fn on_poll_reply(&mut self, session: &str, v: &Value) {
        let Some(text) = v["result"]["result"]["value"].as_str() else {
            return;
        };
        let Ok(Value::Array(reports)) = serde_json::from_str::<Value>(text) else {
            return;
        };
        for r in reports {
            if r["kind"] == "click" {
                if let (Some(x), Some(y)) = (r["x"].as_f64(), r["y"].as_f64()) {
                    self.click(session, x, y);
                }
            } else {
                self.events.push(PageEvent::Report(r.to_string()));
            }
        }
    }

    /// A real (trusted) left click at `x`, `y` in the page's viewport.
    fn click(&mut self, session: &str, x: f64, y: f64) {
        let s = Some(session);
        self.call(s, "Input.dispatchMouseEvent", json!({ "type": "mouseMoved", "x": x, "y": y }));
        for kind in ["mousePressed", "mouseReleased"] {
            self.call(
                s,
                "Input.dispatchMouseEvent",
                json!({ "type": kind, "x": x, "y": y, "button": "left", "buttons": 1, "clickCount": 1 }),
            );
        }
    }

    /// TV mode answers true (handled), a key to press for real, "close" (leave the app),
    /// or false (press the usual key).
    fn on_press_reply(&mut self, p: Press, v: &Value) {
        self.answer(&p, &v["result"]["result"]["value"]);
    }

    fn answer(&mut self, p: &Press, value: &Value) {
        match value {
            Value::Bool(true) => {}
            Value::String(k) if k == "close" => self.events.push(PageEvent::Close),
            Value::String(k) => {
                let k = k.clone();
                self.key(&p.session, &k);
            }
            // { move: { x, y }, then: answer }: a real mouse move first, for players that only
            // bring their controls back for one (Prime Video), then the answer itself. Prime
            // wants a move of some distance: away from the spot and back to it.
            Value::Object(o) if o.contains_key("then") => {
                if let (Some(x), Some(y)) = (o["move"]["x"].as_f64(), o["move"]["y"].as_f64()) {
                    for (mx, my) in [(x - 30.0, y - 20.0), (x, y)] {
                        self.call(
                            Some(&p.session),
                            "Input.dispatchMouseEvent",
                            json!({ "type": "mouseMoved", "x": mx, "y": my }),
                        );
                    }
                }
                self.answer(p, &o["then"]);
            }
            _ => self.key(&p.session, p.fallback),
        }
    }

    /// A real (trusted) key press in the page, wherever the window focus is.
    fn key(&mut self, session: &str, name: &str) {
        let Some((code, vk, text)) = key_def(name) else {
            return;
        };
        let mut down = json!({
            "type": if text.is_some() { "keyDown" } else { "rawKeyDown" },
            "key": name,
            "code": code,
            "windowsVirtualKeyCode": vk,
            "nativeVirtualKeyCode": vk,
        });
        if let Some(t) = text {
            down["text"] = json!(t);
            down["unmodifiedText"] = json!(t);
        }
        self.call(Some(session), "Input.dispatchKeyEvent", down);
        self.call(
            Some(session),
            "Input.dispatchKeyEvent",
            json!({ "type": "keyUp", "key": name, "code": code,
                    "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk }),
        );
    }
}

/// TV mode's name for a press, and the key pressed when TV mode leaves it to the page.
fn press_names(cmd: Cmd) -> Option<(&'static str, &'static str)> {
    Some(match cmd {
        Cmd::Action(Action::Up) => ("up", "ArrowUp"),
        Cmd::Action(Action::Down) => ("down", "ArrowDown"),
        Cmd::Action(Action::Left) => ("left", "ArrowLeft"),
        Cmd::Action(Action::Right) => ("right", "ArrowRight"),
        Cmd::Stick(Action::Up) => ("stickup", "ArrowUp"),
        Cmd::Stick(Action::Down) => ("stickdown", "ArrowDown"),
        Cmd::Action(Action::Select) => ("select", "Enter"),
        Cmd::Action(Action::Back) => ("back", "Escape"),
        Cmd::Extra(Extra::X) => ("playpause", " "),
        // Players toggle fullscreen on F.
        Cmd::Extra(Extra::Y) => ("y", "f"),
        Cmd::Extra(Extra::Lb) => ("pageup", "PageUp"),
        Cmd::Extra(Extra::Rb) => ("pagedown", "PageDown"),
        _ => return None,
    })
}

/// DOM `code`, Windows virtual-key code and typed text for the keys we press.
fn key_def(key: &str) -> Option<(&'static str, u32, Option<&'static str>)> {
    Some(match key {
        "ArrowUp" => ("ArrowUp", 38, None),
        "ArrowDown" => ("ArrowDown", 40, None),
        "ArrowLeft" => ("ArrowLeft", 37, None),
        "ArrowRight" => ("ArrowRight", 39, None),
        "Enter" => ("Enter", 13, Some("\r")),
        "Escape" => ("Escape", 27, None),
        " " => ("Space", 32, Some(" ")),
        "f" => ("KeyF", 70, Some("f")),
        "PageUp" => ("PageUp", 33, None),
        "PageDown" => ("PageDown", 34, None),
        _ => return None,
    })
}

/// Client hints of a TV device, in the shape Cobalt (the TV YouTube apps' runtime) reports them.
fn tv_metadata() -> Value {
    json!({
        "brands": [{ "brand": "TVLauncher", "version": "27" }],
        "fullVersionList": [{ "brand": "TVLauncher", "version": "27.lts.3" }],
        "fullVersion": "27.lts.3",
        "platform": "Starboard",
        "platformVersion": "",
        "architecture": "x86",
        "model": "",
        "mobile": false,
        "bitness": "64",
        "wow64": false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(nav: Nav, ua: Option<&str>) -> Page {
        Page { url: "https://x.test/".into(), nav, user_agent: ua.map(Into::into), watch: None, live: None }
    }

    fn sent(b: &mut Browser) -> Vec<Value> {
        std::mem::take(&mut b.outbox)
            .iter()
            .map(|m| serde_json::from_str(m).unwrap())
            .collect()
    }

    fn methods(msgs: &[Value]) -> Vec<&str> {
        msgs.iter().map(|m| m["method"].as_str().unwrap()).collect()
    }

    fn attach(b: &mut Browser, session: &str, target: &str) {
        b.on_message(
            &json!({ "method": "Target.targetCreated",
                     "params": { "targetInfo": { "targetId": target, "type": "page" } } })
            .to_string(),
        );
        b.on_message(
            &json!({ "method": "Target.attachedToTarget",
                     "params": { "sessionId": session, "targetInfo": { "targetId": target, "type": "page" } } })
            .to_string(),
        );
    }

    #[test]
    fn devtools_socket_from_version() {
        let v = json!({ "webSocketDebuggerUrl": "ws://127.0.0.1:51234/devtools/browser/abc-123" });
        assert_eq!(ws_url(&v, 51234).as_deref(), Some("ws://127.0.0.1:51234/devtools/browser/abc-123"));
        assert_eq!(ws_url(&v, 9222), None);
        assert_eq!(ws_url(&json!({}), 51234), None);
        let elsewhere = json!({ "webSocketDebuggerUrl": "ws://other.test:51234/devtools/browser/x" });
        assert_eq!(ws_url(&elsewhere, 51234), None);
    }

    #[test]
    fn free_port_is_a_real_port() {
        assert!(free_port().is_some_and(|p| p != 0));
    }

    #[test]
    fn tv_page_gets_the_script_and_presses_go_through_it() {
        let mut b = Browser::new(page(Nav::Tv, None));
        b.start();
        attach(&mut b, "S1", "T1");
        let msgs = sent(&mut b);
        assert_eq!(
            methods(&msgs),
            [
                "Target.setDiscoverTargets",
                "Target.attachToTarget",
                "Page.enable",
                "Page.addScriptToEvaluateOnNewDocument",
                "Runtime.evaluate",
                "Page.addScriptToEvaluateOnNewDocument",
                "Runtime.evaluate",
            ]
        );
        assert_eq!(msgs[2]["sessionId"], "S1");
        assert_eq!(msgs[3]["params"]["source"], FULLSCREEN_SCRIPT);

        // The left stick's up and down get names of their own.
        b.command(Cmd::Stick(Action::Up));
        assert!(sent(&mut b)[0]["params"]["expression"].as_str().unwrap().contains("press('stickup')"));

        b.command(Cmd::Action(Action::Down));
        let msgs = sent(&mut b);
        assert!(msgs[0]["params"]["expression"].as_str().unwrap().contains("press('down')"));
        let id = msgs[0]["id"].as_u64().unwrap();

        // Handled by TV mode: no key.
        b.on_message(&json!({ "id": id, "result": { "result": { "type": "boolean", "value": true } } }).to_string());
        assert!(sent(&mut b).is_empty());

        // TV mode asks for a real key.
        b.command(Cmd::Action(Action::Left));
        let id = sent(&mut b)[0]["id"].as_u64().unwrap();
        b.on_message(&json!({ "id": id, "result": { "result": { "type": "string", "value": "ArrowLeft" } } }).to_string());
        let keys = sent(&mut b);
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0]["params"]["type"], "rawKeyDown");
        assert_eq!(keys[0]["params"]["windowsVirtualKeyCode"], 37);
        assert_eq!(keys[1]["params"]["type"], "keyUp");

        // No TV mode in the page (error or false): the usual key.
        b.command(Cmd::Action(Action::Select));
        let id = sent(&mut b)[0]["id"].as_u64().unwrap();
        b.on_message(&json!({ "id": id, "error": { "message": "x" } }).to_string());
        let keys = sent(&mut b);
        assert_eq!(keys[0]["params"]["key"], "Enter");
        assert_eq!(keys[0]["params"]["text"], "\r");

        // A real mouse move first (a player's controls), then the answer itself.
        b.command(Cmd::Action(Action::Right));
        let id = sent(&mut b)[0]["id"].as_u64().unwrap();
        let value = json!({ "move": { "x": 960, "y": 540 }, "then": "ArrowRight" });
        b.on_message(&json!({ "id": id, "result": { "result": { "type": "object", "value": value } } }).to_string());
        let msgs = sent(&mut b);
        assert_eq!(msgs[0]["method"], "Input.dispatchMouseEvent");
        assert_eq!(msgs[0]["params"]["type"], "mouseMoved");
        assert_eq!(msgs[0]["params"]["x"], 930.0);
        assert_eq!(msgs[1]["params"]["x"], 960.0);
        assert_eq!(msgs[1]["params"]["y"], 540.0);
        assert_eq!(msgs[2]["params"]["key"], "ArrowRight");
    }

    #[test]
    fn native_page_gets_the_identity_reloaded_and_plain_keys() {
        let mut b = Browser::new(page(Nav::Native, Some("Cobalt/27")));
        attach(&mut b, "S1", "T1");
        let msgs = sent(&mut b);
        assert_eq!(
            methods(&msgs),
            ["Target.attachToTarget", "Emulation.setUserAgentOverride", "Page.navigate"]
        );
        assert_eq!(msgs[1]["params"]["userAgentMetadata"]["platform"], "Starboard");
        assert_eq!(msgs[2]["params"]["url"], "https://x.test/");

        // A pop-up gets the identity but isn't sent back to the start page.
        attach(&mut b, "S2", "T2");
        assert_eq!(methods(&sent(&mut b)), ["Target.attachToTarget", "Emulation.setUserAgentOverride"]);

        b.command(Cmd::Extra(Extra::X));
        let keys = sent(&mut b);
        assert_eq!(keys[0]["sessionId"], "S2");
        assert_eq!(keys[0]["params"]["key"], " ");
        assert_eq!(keys[0]["params"]["type"], "keyDown");
    }

    #[test]
    fn closed_pop_up_hands_the_pad_back() {
        let mut b = Browser::new(page(Nav::Native, None));
        attach(&mut b, "S1", "T1");
        attach(&mut b, "S2", "T2");
        b.on_message(&json!({ "method": "Target.detachedFromTarget", "params": { "sessionId": "S2" } }).to_string());
        sent(&mut b);
        b.command(Cmd::Action(Action::Back));
        assert_eq!(sent(&mut b)[0]["sessionId"], "S1");
    }

    #[test]
    fn close_shuts_the_browser_down() {
        let mut b = Browser::new(page(Nav::Native, None));
        attach(&mut b, "S1", "T1");
        sent(&mut b);
        b.command(Cmd::Close);
        let msgs = sent(&mut b);
        assert_eq!(methods(&msgs), ["Runtime.evaluate", "Browser.close"]);
        assert_eq!(msgs[0]["sessionId"], "S1");
        assert!(msgs[0]["params"]["expression"].as_str().unwrap().contains("muted = true"));
        assert!(msgs[1].get("sessionId").is_none());
    }

    #[test]
    fn watch_pages_get_the_script_and_reports_are_collected() {
        let mut p = page(Nav::Tv, None);
        p.watch = Some("/*watch*/".into());
        let mut b = Browser::new(p);
        attach(&mut b, "S1", "T1");
        let msgs = sent(&mut b);
        assert_eq!(
            methods(&msgs),
            [
                "Target.attachToTarget",
                "Page.enable",
                "Page.addScriptToEvaluateOnNewDocument",
                "Runtime.evaluate",
                "Runtime.enable",
                "Runtime.addBinding",
                "Runtime.addBinding",
                "Page.addScriptToEvaluateOnNewDocument",
                "Runtime.evaluate",
                "Page.addScriptToEvaluateOnNewDocument",
                "Runtime.evaluate",
            ]
        );
        assert_eq!(msgs[5]["params"]["name"], "__tvlNow");
        assert_eq!(msgs[6]["params"]["name"], "__tvlCookieAsk");
        assert_eq!(msgs[7]["params"]["source"], "/*watch*/");

        b.poll();
        // Only one collection in flight.
        b.poll();
        let msgs = sent(&mut b);
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0]["params"]["expression"].as_str().unwrap().contains("__tvlQueue"));
        let id = msgs[0]["id"].as_u64().unwrap();
        let reports = r#"[{"kind":"sample","t":3},{"kind":"click","x":10,"y":20.5}]"#;
        b.on_message(&json!({ "id": id, "result": { "result": { "type": "string", "value": reports } } }).to_string());
        let clicks = sent(&mut b);
        assert_eq!(clicks.len(), 3);
        assert_eq!(clicks[1]["params"]["type"], "mousePressed");
        assert_eq!(clicks[1]["params"]["y"], 20.5);
        assert_eq!(clicks[2]["sessionId"], "S1");
        let events = std::mem::take(&mut b.events);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], PageEvent::Report(r) if r.contains("sample")));

        // The page asks for a collection now, without waiting for the next tick.
        b.on_message(
            &json!({ "method": "Runtime.bindingCalled", "sessionId": "S1",
                     "params": { "name": "__tvlNow", "payload": "", "executionContextId": 1 } })
            .to_string(),
        );
        let msgs = sent(&mut b);
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0]["params"]["expression"].as_str().unwrap().contains("__tvlQueue"));

        // A cookie the page can't read: looked up in the browser, answered in the page.
        b.on_message(
            &json!({ "method": "Runtime.bindingCalled", "sessionId": "S1",
                     "params": { "name": "__tvlCookieAsk", "payload": "av-profile", "executionContextId": 1 } })
            .to_string(),
        );
        let msgs = sent(&mut b);
        assert_eq!(methods(&msgs), ["Storage.getCookies"]);
        assert!(msgs[0].get("sessionId").is_none());
        let id = msgs[0]["id"].as_u64().unwrap();
        let cookies = json!([{ "name": "other", "value": "x" }, { "name": "av-profile", "value": "cGlk" }]);
        b.on_message(&json!({ "id": id, "result": { "cookies": cookies } }).to_string());
        let msgs = sent(&mut b);
        assert_eq!(msgs[0]["sessionId"], "S1");
        assert_eq!(msgs[0]["params"]["expression"], r#"window.__tvlCookie && window.__tvlCookie("av-profile", "cGlk")"#);
    }

    #[test]
    fn tv_mode_can_ask_to_close() {
        let mut b = Browser::new(page(Nav::Tv, None));
        attach(&mut b, "S1", "T1");
        sent(&mut b);
        b.command(Cmd::Action(Action::Back));
        let id = sent(&mut b)[0]["id"].as_u64().unwrap();
        b.on_message(&json!({ "id": id, "result": { "result": { "type": "string", "value": "close" } } }).to_string());
        assert!(sent(&mut b).is_empty());
        assert_eq!(b.events, vec![PageEvent::Close]);
        // Pages opened outside a watch aren't polled.
        b.poll();
        assert!(sent(&mut b).is_empty());
    }

    #[test]
    fn a_page_is_attached_once_and_triggers_are_left_alone() {
        let mut b = Browser::new(page(Nav::Tv, None));
        attach(&mut b, "S1", "T1");
        sent(&mut b);
        b.on_message(
            &json!({ "method": "Target.targetCreated",
                     "params": { "targetInfo": { "targetId": "T1", "type": "page" } } })
            .to_string(),
        );
        assert!(sent(&mut b).is_empty());
        b.command(Cmd::Extra(Extra::Rt));
        assert!(sent(&mut b).is_empty());
    }
}
