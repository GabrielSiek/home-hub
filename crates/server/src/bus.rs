//! Events on their way to screens and TV PCs. Each says where it came from, so a browser hears
//! the server's events and only those of the TV PC it controls.

use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The server: for everyone.
    Server,
    /// A TV PC: for the screens controlling it.
    Device(Arc<str>),
    /// For one screen only (`follow`, `resync`).
    To(Arc<str>),
}

#[derive(Debug, Clone)]
pub struct Envelope {
    pub event: Arc<str>,
    /// The payload as JSON text, made once for every listener.
    pub payload: Arc<str>,
    pub source: Source,
}

impl Envelope {
    /// The form browsers read: {"event": …, "payload": …}, as the old launcher sent it.
    pub fn browser_text(&self) -> String {
        format!(r#"{{"event":{},"payload":{}}}"#, Value::String(self.event.to_string()), self.payload)
    }
}

#[derive(Clone)]
pub struct Bus {
    tx: broadcast::Sender<Envelope>,
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

impl Bus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(512);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Envelope> {
        self.tx.subscribe()
    }

    /// From the server, to everyone.
    pub fn emit(&self, event: &str, payload: &impl Serialize) {
        self.send(event, payload, Source::Server);
    }

    /// From TV PC `device`, to the screens controlling it.
    pub fn emit_from(&self, device: &str, event: &str, payload: &impl Serialize) {
        self.send(event, payload, Source::Device(device.into()));
    }

    /// To screen `screen` only.
    pub fn emit_to(&self, screen: &str, event: &str, payload: &impl Serialize) {
        self.send(event, payload, Source::To(screen.into()));
    }

    fn send(&self, event: &str, payload: &impl Serialize, source: Source) {
        let payload = match serde_json::to_string(payload) {
            Ok(p) => p,
            Err(e) => return tracing::error!(event, "can't serialize the event: {e}"),
        };
        // No one listening is fine.
        let _ = self.tx.send(Envelope { event: event.into(), payload: payload.into(), source });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn browsers_get_the_old_launchers_format() {
        let bus = Bus::new();
        let mut rx = bus.subscribe();
        bus.emit_from("pc-quarto", "tv", &json!({"mode": "launcher"}));
        let e = rx.try_recv().unwrap();
        assert_eq!(e.source, Source::Device("pc-quarto".into()));
        assert_eq!(e.browser_text(), r#"{"event":"tv","payload":{"mode":"launcher"}}"#);
    }
}
