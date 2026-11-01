//! Google Cast, as a phone speaks it: TLS to port 8009, then length-prefixed protobuf frames
//! carrying JSON. The device's certificate is its own self-signed one, so it isn't checked; only
//! addresses on the home network, found by DNS-SD, are ever reached.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{ring, verify_tls12_signature, verify_tls13_signature, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;
use tvl_kit::{Error, Result};

const SENDER: &str = "sender-tvl";
const RECEIVER: &str = "receiver-0";
const NS_CONNECTION: &str = "urn:x-cast:com.google.cast.tp.connection";
const NS_HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
const NS_RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";
const NS_MEDIA: &str = "urn:x-cast:com.google.cast.media";
/// Cast's own player, for a file at an address.
pub const MEDIA_APP: &str = "CC1AD845";
/// Spotify. On a Google TV this opens its Spotify app, which then shows up as a Connect device.
pub const SPOTIFY_APP: &str = "CC32E753";
const ANSWER_WAIT: Duration = Duration::from_secs(20);
const LARGEST_FRAME: usize = 1 << 20;

#[derive(Debug)]
struct AnyCertificate(WebPkiSupportedAlgorithms);

impl ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(&self, _: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> std::result::Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.0)
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.0)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }
}

/// A TLS connection to a device on the home network, with our certificate when it wants one.
pub(super) async fn tls(addr: SocketAddr, client: Option<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)>) -> Result<TlsStream<TcpStream>> {
    let provider = Arc::new(ring::default_provider());
    let algorithms = provider.signature_verification_algorithms;
    let builder = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::msg(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyCertificate(algorithms)));
    let config = match client {
        Some((certificates, key)) => builder.with_client_auth_cert(certificates, key).map_err(|e| Error::msg(format!("Our certificate: {e}")))?,
        None => builder.with_no_client_auth(),
    };
    let name = ServerName::from(addr.ip());
    let tcp = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(addr))
        .await
        .map_err(|_| Error::msg("The TV didn't answer: is it on?"))??;
    tokio::time::timeout(Duration::from_secs(5), TlsConnector::from(Arc::new(config)).connect(name, tcp))
        .await
        .map_err(|_| Error::msg("The TV didn't answer"))?
        .map_err(Error::from)
}

// ---------- Frames ----------

pub(super) fn put_varint(out: &mut Vec<u8>, mut n: u64) {
    while n >= 0x80 {
        out.push(n as u8 | 0x80);
        n >>= 7;
    }
    out.push(n as u8);
}

fn put_text(out: &mut Vec<u8>, field: u8, text: &str) {
    out.push(field << 3 | 2);
    put_varint(out, text.len() as u64);
    out.extend_from_slice(text.as_bytes());
}

/// A CastMessage: version 0 and payload type 0 (text) are the only ones used.
fn encode(source: &str, destination: &str, namespace: &str, payload: &str) -> Vec<u8> {
    let mut m = vec![1 << 3, 0];
    put_text(&mut m, 2, source);
    put_text(&mut m, 3, destination);
    put_text(&mut m, 4, namespace);
    m.extend_from_slice(&[5 << 3, 0]);
    put_text(&mut m, 6, payload);
    m
}

fn varint(m: &[u8], at: &mut usize) -> Option<u64> {
    let (mut n, mut shift) = (0u64, 0);
    loop {
        let b = *m.get(*at)?;
        *at += 1;
        n |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Some(n);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

#[derive(Debug, Default, PartialEq)]
struct Message {
    source: String,
    namespace: String,
    payload: String,
}

fn decode(m: &[u8]) -> Option<Message> {
    let mut out = Message::default();
    let mut at = 0;
    while at < m.len() {
        let tag = varint(m, &mut at)?;
        match tag & 7 {
            0 => {
                varint(m, &mut at)?;
            }
            2 => {
                let len = varint(m, &mut at)? as usize;
                let text = m.get(at..at.checked_add(len)?)?;
                at += len;
                let text = String::from_utf8_lossy(text).into_owned();
                match tag >> 3 {
                    2 => out.source = text,
                    4 => out.namespace = text,
                    6 => out.payload = text,
                    _ => {}
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

// ---------- A connection ----------

struct Session {
    stream: TlsStream<TcpStream>,
    request: u64,
}

impl Session {
    async fn open(addr: SocketAddr) -> Result<Self> {
        let mut s = Session { stream: tls(addr, None).await?, request: 0 };
        s.connect_to(RECEIVER).await?;
        Ok(s)
    }

    async fn send(&mut self, destination: &str, namespace: &str, payload: &Value) -> Result<()> {
        let m = encode(SENDER, destination, namespace, &payload.to_string());
        self.stream.write_all(&(m.len() as u32).to_be_bytes()).await?;
        self.stream.write_all(&m).await?;
        Ok(self.stream.flush().await?)
    }

    async fn connect_to(&mut self, destination: &str) -> Result<()> {
        self.send(destination, NS_CONNECTION, &json!({ "type": "CONNECT" })).await
    }

    /// Sends a request; its answer carries the same id.
    async fn ask(&mut self, destination: &str, namespace: &str, mut payload: Value) -> Result<Value> {
        self.request += 1;
        let id = self.request;
        payload["requestId"] = json!(id);
        self.send(destination, namespace, &payload).await?;
        let answer = async {
            loop {
                let m = self.next().await?;
                let v: Value = serde_json::from_str(&m.payload)?;
                if v["requestId"].as_u64() == Some(id) {
                    return Ok::<Value, Error>(v);
                }
            }
        };
        tokio::time::timeout(ANSWER_WAIT, answer).await.map_err(|_| Error::msg("The TV didn't answer in time"))?
    }

    /// The next message for the caller; the device's pings are answered on the way.
    async fn next(&mut self) -> Result<Message> {
        loop {
            let mut len = [0u8; 4];
            self.stream.read_exact(&mut len).await?;
            let len = u32::from_be_bytes(len) as usize;
            if len > LARGEST_FRAME {
                return Err(Error::msg("The TV sent something too large"));
            }
            let mut frame = vec![0u8; len];
            self.stream.read_exact(&mut frame).await?;
            let Some(m) = decode(&frame) else { continue };
            if m.namespace == NS_HEARTBEAT {
                let from = m.source.clone();
                self.send(&from, NS_HEARTBEAT, &json!({ "type": "PONG" })).await?;
                continue;
            }
            return Ok(m);
        }
    }

    async fn status(&mut self) -> Result<Value> {
        self.ask(RECEIVER, NS_RECEIVER, json!({ "type": "GET_STATUS" })).await
    }

    /// Opens `app` (or finds it open) and joins it: its transport id.
    async fn launch(&mut self, app: &str) -> Result<String> {
        let v = self.ask(RECEIVER, NS_RECEIVER, json!({ "type": "LAUNCH", "appId": app })).await?;
        if v["type"] != "RECEIVER_STATUS" {
            return Err(Error::msg(format!("The TV didn't open it: {}", v["reason"].as_str().unwrap_or("no reason given"))));
        }
        let transport = running(&v, app).ok_or_else(|| Error::msg("The TV didn't open it"))?;
        self.connect_to(&transport).await?;
        Ok(transport)
    }
}

/// The transport of `app` in a RECEIVER_STATUS, when it's open.
fn running(status: &Value, app: &str) -> Option<String> {
    let apps = status["status"]["applications"].as_array()?;
    apps.iter().find(|a| a["appId"] == app)?["transportId"].as_str().map(str::to_string)
}

/// What's open on the device, by app id: read only, nothing on the screen changes.
pub async fn open_apps(addr: SocketAddr) -> Result<Vec<String>> {
    let mut s = Session::open(addr).await?;
    let v = s.status().await?;
    let apps = v["status"]["applications"].as_array().into_iter().flatten();
    Ok(apps.filter_map(|a| a["appId"].as_str().map(str::to_string)).collect())
}

/// Opens `app`, waking the TV if it has to.
pub async fn open_app(addr: SocketAddr, app: &str) -> Result<()> {
    Session::open(addr).await?.launch(app).await.map(|_| ())
}

/// Why a file wouldn't start, from a MEDIA_STATUS or a refusal; None when it's playing.
fn refused(answer: &Value) -> Option<String> {
    match answer["type"].as_str() {
        Some("MEDIA_STATUS") => {
            let status = answer["status"].get(0)?;
            (status["playerState"] == "IDLE" && status["idleReason"] == "ERROR").then(|| "The TV can't play this file's format".to_string())
        }
        Some("LOAD_FAILED") => Some("The TV can't play this file's format".to_string()),
        Some("LOAD_CANCELLED") => Some("Something else started on the TV".to_string()),
        Some(other) => Some(format!("The TV refused it: {}", answer["reason"].as_str().unwrap_or(other))),
        None => Some("The TV refused it".to_string()),
    }
}

/// Plays the file at `url` on the device's own player.
pub async fn play(addr: SocketAddr, url: &str, content_type: &str, title: &str) -> Result<()> {
    let mut s = Session::open(addr).await?;
    let transport = s.launch(MEDIA_APP).await?;
    let load = json!({
        "type": "LOAD",
        "autoplay": true,
        "media": {
            "contentId": url,
            "contentType": content_type,
            "streamType": "BUFFERED",
            "metadata": { "metadataType": 0, "title": title },
        },
    });
    let answer = s.ask(&transport, NS_MEDIA, load).await?;
    match refused(&answer) {
        Some(why) => Err(Error::msg(why)),
        None => Ok(()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Play,
    Pause,
    Stop,
}

/// Acts on what `MEDIA_APP` is playing; with nothing there, nothing happens.
pub async fn control(addr: SocketAddr, action: Action) -> Result<()> {
    let mut s = Session::open(addr).await?;
    let status = s.status().await?;
    let Some(transport) = running(&status, MEDIA_APP) else { return Ok(()) };
    if action == Action::Stop {
        let session = status["status"]["applications"].as_array().and_then(|a| a.iter().find(|a| a["appId"] == MEDIA_APP)).map(|a| a["sessionId"].clone());
        s.ask(RECEIVER, NS_RECEIVER, json!({ "type": "STOP", "sessionId": session })).await?;
        return Ok(());
    }
    s.connect_to(&transport).await?;
    let media = s.ask(&transport, NS_MEDIA, json!({ "type": "GET_STATUS" })).await?;
    let Some(session) = media["status"].get(0).map(|m| m["mediaSessionId"].clone()) else { return Ok(()) };
    let kind = if action == Action::Play { "PLAY" } else { "PAUSE" };
    s.ask(&transport, NS_MEDIA, json!({ "type": kind, "mediaSessionId": session })).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_survives_the_wire() {
        let m = encode("sender-tvl", "receiver-0", NS_RECEIVER, r#"{"type":"GET_STATUS"}"#);
        assert_eq!(&m[..2], [8, 0], "version 0 first");
        let d = decode(&m).unwrap();
        assert_eq!(d.source, "sender-tvl");
        assert_eq!(d.namespace, NS_RECEIVER);
        assert_eq!(d.payload, r#"{"type":"GET_STATUS"}"#);
    }

    #[test]
    fn long_payloads_use_a_longer_length() {
        let long = "x".repeat(70_000);
        assert_eq!(decode(&encode("a", "b", "c", &long)).unwrap().payload, long);
    }

    #[test]
    fn a_cut_frame_is_refused() {
        let m = encode("a", "b", "c", "payload");
        assert!(decode(&m[..m.len() - 2]).is_none());
    }

    #[test]
    fn finds_an_app_in_the_status() {
        let v = json!({ "status": { "applications": [{ "appId": MEDIA_APP, "transportId": "t-1" }] } });
        assert_eq!(running(&v, MEDIA_APP).as_deref(), Some("t-1"));
        assert_eq!(running(&v, SPOTIFY_APP), None);
        assert_eq!(running(&json!({ "status": {} }), MEDIA_APP), None);
    }

    #[test]
    fn says_when_a_file_wont_play() {
        assert_eq!(refused(&json!({ "type": "MEDIA_STATUS", "status": [{ "playerState": "BUFFERING" }] })), None);
        assert!(refused(&json!({ "type": "MEDIA_STATUS", "status": [{ "playerState": "IDLE", "idleReason": "ERROR" }] })).is_some());
        assert!(refused(&json!({ "type": "LOAD_FAILED" })).is_some());
        assert!(refused(&json!({ "type": "INVALID_REQUEST", "reason": "INVALID_COMMAND" })).unwrap().contains("INVALID_COMMAND"));
    }

    /// `CAST=192.168.68.120:8009 cargo test -p tvl-server cast_status -- --ignored --nocapture`:
    /// asks a real device what's open. Changes nothing on its screen.
    #[tokio::test]
    #[ignore = "needs a real device"]
    async fn cast_status() {
        let addr = std::env::var("CAST").unwrap().parse().unwrap();
        println!("open: {:?}", open_apps(addr).await.unwrap());
    }
}
