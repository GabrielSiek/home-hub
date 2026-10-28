//! The event socket a companion page keeps open (`/api/events?screen=<id>`): the server's events,
//! the events of the TV PC that page controls, and those meant for it alone. Each event name is
//! sent at most every 100 ms with its latest payload, which keeps an old iPad from drowning when
//! a slider sends a burst of changes.

use std::time::Duration;

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::Response;
use serde::Deserialize;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::MissedTickBehavior;
use tvl_protocol::topics;

use crate::bus::{Envelope, Source};
use crate::ctx::Ctx;

const COALESCE: Duration = Duration::from_millis(100);
/// Safari keeps a dead socket around after the iPad sleeps; pings find out.
const PING: Duration = Duration::from_secs(25);

#[derive(Deserialize)]
pub struct EventsQuery {
    /// The screen this page is (companion_hello), so the house knows it's online.
    screen: Option<String>,
}

pub async fn socket(ws: WebSocketUpgrade, State(ctx): State<Ctx>, Query(q): Query<EventsQuery>) -> Response {
    let screen = q
        .screen
        .filter(|s| !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    ws.on_upgrade(move |socket| relay(socket, ctx, screen))
}

/// Whether the page that is `screen` hears `e`.
pub fn wants(ctx: &Ctx, screen: Option<&str>, e: &Envelope) -> bool {
    match &e.source {
        // Never the TV PCs' own feed (it carries their media keys unmasked).
        Source::Server => !topics::TO_AGENTS.contains(&&*e.event),
        Source::To(id) => screen == Some(&**id),
        Source::Device(d) => topics::for_followers(&e.event) && ctx.devices.followed(screen).as_deref() == Some(&**d),
    }
}

async fn relay(mut socket: WebSocket, ctx: Ctx, screen: Option<String>) {
    let first = !ctx.devices.any_page_open();
    if let Some(id) = &screen {
        ctx.devices.socket(id, true);
    }
    if first {
        crate::smarthome::page_opened(&ctx);
    }
    let mut rx = ctx.bus.subscribe();
    // Waiting to go out, one per event name, in the order first seen.
    let mut pending: Vec<(String, String)> = Vec::new();
    let mut flush = tokio::time::interval(COALESCE);
    flush.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut ping = tokio::time::interval(PING);
    loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                Ok(e) if wants(&ctx, screen.as_deref(), &e) => {
                    let text = e.browser_text();
                    match pending.iter_mut().find(|(name, _)| **name == *e.event) {
                        Some(p) => p.1 = text,
                        None => pending.push((e.event.to_string(), text)),
                    }
                }
                Ok(_) => {}
                // Fell behind: what's queued is stale; the page loads everything again.
                Err(RecvError::Lagged(_)) => {
                    pending.clear();
                    pending.push(("resync".into(), r#"{"event":"resync","payload":null}"#.into()));
                }
                Err(RecvError::Closed) => break,
            },
            _ = flush.tick(), if !pending.is_empty() => {
                let mut gone = false;
                for (_, text) in pending.drain(..) {
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        gone = true;
                        break;
                    }
                }
                if gone {
                    break;
                }
            }
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
            _ = ping.tick() => {
                if socket.send(Message::Ping(Bytes::new())).await.is_err() {
                    break;
                }
            }
            _ = ctx.shutdown.cancelled() => break,
        }
    }
    if let Some(id) = &screen {
        ctx.devices.socket(id, false);
    }
}
