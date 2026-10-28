//! Tuya local LAN protocol, versions 3.3 and 3.5, ported from tinytuya 1.20.
//!
//! Each call opens one TCP connection to port 6668, does its job and closes it: the older
//! ESP-based devices accept only one local client at a time.
//!
//! - 3.3 uses `55AA` frames: header, payload, CRC32, suffix. Payloads are AES-128-ECB under
//!   the device's local key; CONTROL payloads get a plaintext `"3.3"` + 12 NULs header.
//! - 3.5 uses `6699` frames: AES-128-GCM with the header as associated data, under a session
//!   key negotiated on every connection, and the "new" command codes (CONTROL_NEW,
//!   DP_QUERY_NEW).

use std::collections::BTreeMap;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aes::cipher::{generic_array::GenericArray, BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes128;
use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes128Gcm, Nonce};
use hmac::{Hmac, Mac};
use md5::{Digest, Md5};
use serde_json::{Map, Value};
use sha2::Sha256;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::time::{timeout_at, Instant};

use tvl_kit::{Error, Result};

const TCP_PORT: u16 = 6668;
/// UDP ports devices (6666: 3.1 plaintext, 6667: 3.3+ encrypted) and apps (7000) broadcast on.
const UDP_PORTS: [u16; 3] = [6666, 6667, 7000];
/// Budget for a whole call: connect, session key (3.5), request and reply.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// How long `status` waits for a reply before asking again on a new connection.
const QUERY_RETRY: Duration = Duration::from_secs(2);
/// How long `set` waits for an ack before assuming the device is one that never answers.
const ACK_WAIT: Duration = Duration::from_secs(1);
/// Anything bigger is a desynced stream, not a real frame.
const MAX_FRAME: usize = 0x10000;

const PREFIX_55AA: [u8; 4] = [0, 0, 0x55, 0xAA];
const SUFFIX_55AA: [u8; 4] = [0, 0, 0xAA, 0x55];
const PREFIX_6699: [u8; 4] = [0, 0, 0x66, 0x99];
const SUFFIX_6699: [u8; 4] = [0, 0, 0x99, 0x66];

// Command codes (tinytuya core/command_types.py).
const SESS_KEY_NEG_START: u32 = 0x03;
const SESS_KEY_NEG_RESP: u32 = 0x04;
const SESS_KEY_NEG_FINISH: u32 = 0x05;
const CONTROL: u32 = 0x07;
const DP_QUERY: u32 = 0x0a;
const CONTROL_NEW: u32 = 0x0d;
const DP_QUERY_NEW: u32 = 0x10;

/// Tuya LAN protocol version. Parses from `"3.3"` / `"3.5"` (a leading `v` is accepted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    V33,
    V35,
}

impl Version {
    /// The version as Tuya writes it, e.g. `"3.3"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Version::V33 => "3.3",
            Version::V35 => "3.5",
        }
    }
}

impl FromStr for Version {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().trim_start_matches(['v', 'V']) {
            "3.3" => Ok(Version::V33),
            "3.5" => Ok(Version::V35),
            other => Err(Error::msg(format!(
                "unsupported Tuya protocol version {other:?} (only 3.3 and 3.5)"
            ))),
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A device to talk to: its id, LAN IP, 16-character local key and protocol version.
#[derive(Debug, Clone)]
pub struct Target {
    pub id: String,
    pub ip: String,
    pub key: String,
    pub version: Version,
}

/// A device heard broadcasting on the LAN.
#[derive(Debug, Clone)]
pub struct Seen {
    pub id: String,
    pub ip: String,
    /// Protocol version as broadcast, e.g. `"3.3"` (empty if the device didn't say).
    pub version: String,
}

/// Reads the device's data points (`dps`), e.g. `{"20": true, "21": "white", ...}`.
pub async fn status(t: &Target) -> Result<Map<String, Value>> {
    let deadline = Instant::now() + CALL_TIMEOUT;
    // Bulbs now and then ignore a query; like tinytuya, ask again on a fresh connection.
    match query(t, deadline, QUERY_RETRY).await? {
        Some(dps) => Ok(dps),
        None => query(t, deadline, CALL_TIMEOUT)
            .await?
            .ok_or_else(|| Error::msg(format!("{} didn't answer the status request", t.ip))),
    }
}

/// One connection's worth of `status`; None if the device stays silent for `wait`.
async fn query(
    t: &Target,
    deadline: Instant,
    wait: Duration,
) -> Result<Option<Map<String, Value>>> {
    let mut conn = Conn::open(t, deadline).await?;
    let (cmd, json) = query_json(t, unix_now());
    conn.send(cmd, &json, deadline).await?;
    let until = deadline.min(Instant::now() + wait);
    loop {
        match conn.recv(until).await? {
            Recv::Timeout => return Ok(None),
            Recv::Closed => return Err(conn.closed_early()),
            // Skip heartbeats, pushed status updates and empty acks.
            Recv::Msg(m) if m.cmd != cmd || m.data.is_empty() => continue,
            Recv::Msg(m) => {
                let mut v = conn.parse(&m)?;
                return match take_dps(&mut v) {
                    Some(dps) => Ok(Some(dps)),
                    None => Err(Error::msg(format!("{} answered without dps: {v}", t.ip))),
                };
            }
        }
    }
}

/// Sends a CONTROL with `dps` (e.g. `{"20": true}`). Succeeds when the device acks, or, for
/// devices that never answer (IR hubs), when the frame went out and no error came back
/// within a short wait. Never resends, so an IR code goes out at most once.
pub async fn set(t: &Target, dps: Map<String, Value>) -> Result<()> {
    if dps.is_empty() {
        return Err(Error::msg("nothing to set"));
    }
    let deadline = Instant::now() + CALL_TIMEOUT;
    let mut conn = Conn::open(t, deadline).await?;
    let (cmd, json) = control_json(t, &dps, unix_now())?;
    conn.send(cmd, &json, deadline).await?;
    let until = deadline.min(Instant::now() + ACK_WAIT);
    loop {
        match conn.recv(until).await? {
            Recv::Timeout => return Ok(()),
            Recv::Closed => return Err(conn.closed_early()),
            Recv::Msg(m) if m.cmd != cmd => continue,
            Recv::Msg(m) => {
                // A non-empty ack is usually the device's complaint ("data format error").
                if !m.data.is_empty() {
                    conn.parse(&m)?;
                }
                return match m.retcode {
                    Some(rc) if rc != 0 => Err(Error::msg(format!(
                        "{} rejected the command (code {rc})",
                        t.ip
                    ))),
                    _ => Ok(()),
                };
            }
        }
    }
}

/// Listens for `window` to the devices' UDP broadcasts on ports 6666, 6667 and 7000 and
/// returns each device once (by id). Purely passive: nothing is sent.
pub async fn discover(window: Duration) -> Result<Vec<Seen>> {
    let until = Instant::now() + window;
    let (a, b, c) = tokio::join!(
        listen(UDP_PORTS[0], until),
        listen(UDP_PORTS[1], until),
        listen(UDP_PORTS[2], until)
    );
    let mut by_id = BTreeMap::new();
    let mut first_err = None;
    let mut listened = false;
    for r in [a, b, c] {
        match r {
            Ok(seen) => {
                listened = true;
                for s in seen {
                    by_id.entry(s.id.clone()).or_insert(s);
                }
            }
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    match first_err {
        Some(e) if !listened => Err(e),
        _ => Ok(by_id.into_values().collect()),
    }
}

async fn listen(port: u16, until: Instant) -> Result<Vec<Seen>> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port))
        .await
        .map_err(|e| Error::msg(format!("can't listen on UDP port {port}: {e}")))?;
    let mut buf = vec![0u8; 4096];
    let mut seen = Vec::new();
    while let Ok(Ok((n, from))) = timeout_at(until, sock.recv_from(&mut buf)).await {
        seen.extend(parse_broadcast(&buf[..n], from.ip()));
    }
    Ok(seen)
}

fn parse_broadcast(data: &[u8], from: IpAddr) -> Option<Seen> {
    let v: Value = serde_json::from_slice(&decrypt_broadcast(data)?).ok()?;
    let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    // App broadcasts ({"from":"app",...}) have no gwId.
    Some(Seen {
        id: text("gwId")?,
        ip: text("ip").unwrap_or_else(|| from.to_string()),
        version: text("version").unwrap_or_default(),
    })
}

/// Broadcast payload as JSON bytes, decrypted like tinytuya's `decrypt_udp`.
fn decrypt_broadcast(data: &[u8]) -> Option<Vec<u8>> {
    let key = udp_key();
    let mut plain = if data.starts_with(&PREFIX_6699) {
        let (_, _, p) = unpack_6699(&key, data)?;
        split_retcode(&p).1.to_vec()
    } else if data.starts_with(&PREFIX_55AA) {
        let (_, _, payload) = unpack_55aa(data)?;
        let p = split_retcode(payload).1;
        // 3.1 devices broadcast plaintext JSON on 6666.
        if p.first() == Some(&b'{') {
            p.to_vec()
        } else {
            ecb_decrypt(&key, p)?
        }
    } else {
        ecb_decrypt(&key, data)?
    };
    while plain.last() == Some(&0) {
        plain.pop();
    }
    Some(plain)
}

/// Broadcasts are encrypted with md5("yGAdlopoPVldABfn") (from tuya-convert).
fn udp_key() -> [u8; 16] {
    let mut k = [0u8; 16];
    k.copy_from_slice(&Md5::digest(b"yGAdlopoPVldABfn"));
    k
}

// ---- Connection -------------------------------------------------------------------------

/// A decoded frame from the device.
#[derive(Debug)]
struct Msg {
    cmd: u32,
    retcode: Option<u32>,
    /// Decrypted payload without the return code. On 3.5 it may still start with a version
    /// header (encrypted along with the JSON); `Conn::parse` drops it.
    data: Vec<u8>,
}

enum Recv {
    Msg(Msg),
    Timeout,
    Closed,
}

/// Why an incoming frame couldn't be read.
#[derive(Debug, PartialEq)]
enum Garbled {
    Crc,
    Key,
}

struct Conn<'a> {
    t: &'a Target,
    stream: TcpStream,
    /// The local key (3.3) or this connection's session key (3.5).
    key: [u8; 16],
    seq: u32,
    buf: Vec<u8>,
}

impl<'a> Conn<'a> {
    async fn open(t: &'a Target, deadline: Instant) -> Result<Conn<'a>> {
        let key: [u8; 16] =
            t.key.as_bytes().try_into().map_err(|_| {
                Error::msg(format!("the local key for {} must be 16 characters", t.id))
            })?;
        let ip: IpAddr =
            t.ip.parse()
                .map_err(|_| Error::msg(format!("{:?} isn't an IP address", t.ip)))?;
        let stream =
            match timeout_at(deadline, TcpStream::connect(SocketAddr::new(ip, TCP_PORT))).await {
                Err(_) => return Err(Error::msg(format!("{} didn't answer", t.ip))),
                Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                    return Err(Error::msg(format!(
                        "{} refused the connection (is another app connected to it?)",
                        t.ip
                    )))
                }
                Ok(Err(e)) => return Err(Error::msg(format!("couldn't connect to {}: {e}", t.ip))),
                Ok(Ok(s)) => s,
            };
        stream.set_nodelay(true)?;
        let mut conn = Conn {
            t,
            stream,
            key,
            seq: 1,
            buf: Vec::new(),
        };
        if t.version == Version::V35 {
            conn.negotiate(deadline).await?;
        }
        Ok(conn)
    }

    /// 3.5 session key: we send a nonce, the device answers with its nonce plus
    /// HMAC(local key, our nonce), we answer with HMAC(local key, its nonce).
    async fn negotiate(&mut self, deadline: Instant) -> Result<()> {
        let real = self.key;
        let mut local = [0u8; 16];
        random(&mut local)?;
        self.send(SESS_KEY_NEG_START, &local, deadline).await?;
        let reply = loop {
            match self.recv(deadline).await? {
                Recv::Msg(m) if m.cmd == SESS_KEY_NEG_RESP && !m.data.is_empty() => break m.data,
                Recv::Msg(_) => continue,
                // A 3.5 device stays silent when our request was sealed with the wrong key.
                Recv::Timeout => {
                    return Err(Error::msg(format!(
                        "{} didn't answer the session key request (wrong key for {}?)",
                        self.t.ip, self.t.id
                    )))
                }
                Recv::Closed => return Err(self.closed_early()),
            }
        };
        if reply.len() < 48 || hmac_sha256(&real, &local)? != reply[16..48] {
            return Err(self.wrong_key());
        }
        let mut remote = [0u8; 16];
        remote.copy_from_slice(&reply[..16]);
        self.send(SESS_KEY_NEG_FINISH, &hmac_sha256(&real, &remote)?, deadline)
            .await?;
        self.key = session_key(&real, &local, &remote)?;
        Ok(())
    }

    async fn send(&mut self, cmd: u32, json: &[u8], deadline: Instant) -> Result<()> {
        let mut iv = [0u8; 12];
        random(&mut iv)?;
        let frame = encode(self.t.version, &self.key, self.seq, cmd, json, iv)?;
        self.seq += 1;
        match timeout_at(deadline, self.stream.write_all(&frame)).await {
            Err(_) => Err(Error::msg(format!("{} didn't answer", self.t.ip))),
            Ok(r) => Ok(r?),
        }
    }

    /// The next frame from the device, skipping garbage between frames.
    async fn recv(&mut self, until: Instant) -> Result<Recv> {
        loop {
            if let Some(frame) = take_frame(&mut self.buf) {
                return match decode(&self.key, &frame) {
                    Ok(m) => Ok(Recv::Msg(m)),
                    Err(Garbled::Key) => Err(self.wrong_key()),
                    Err(Garbled::Crc) => {
                        Err(Error::msg(format!("{} sent a corrupt frame", self.t.ip)))
                    }
                };
            }
            let mut chunk = [0u8; 2048];
            let n = match timeout_at(until, self.stream.read(&mut chunk)).await {
                Err(_) => return Ok(Recv::Timeout),
                Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionReset => 0,
                Ok(r) => r?,
            };
            if n == 0 {
                return Ok(Recv::Closed);
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }

    /// A reply's JSON. Readable text that isn't JSON is the device complaining.
    fn parse(&self, m: &Msg) -> Result<Value> {
        let mut data = strip_version_header(&m.data);
        while let [rest @ .., 0] = data {
            data = rest;
        }
        if let Ok(v) = serde_json::from_slice(data) {
            return Ok(v);
        }
        match std::str::from_utf8(data) {
            Ok(text) if !text.chars().any(char::is_control) => {
                Err(Error::msg(format!("{} replied {text:?}", self.t.ip)))
            }
            _ => Err(self.wrong_key()),
        }
    }

    fn wrong_key(&self) -> Error {
        Error::msg(format!("wrong key for {} ({})", self.t.id, self.t.ip))
    }

    fn closed_early(&self) -> Error {
        Error::msg(format!(
            "{} closed the connection without answering (is another app connected to it?)",
            self.t.ip
        ))
    }
}

// ---- Payloads -------------------------------------------------------------------------

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Status request: (command, JSON), keys in tinytuya's order.
fn query_json(t: &Target, now: u64) -> (u32, Vec<u8>) {
    match t.version {
        Version::V33 => {
            let id = Value::from(t.id.as_str());
            let json = format!(r#"{{"gwId":{id},"devId":{id},"uid":{id},"t":"{now}"}}"#);
            (DP_QUERY, json.into_bytes())
        }
        Version::V35 => (DP_QUERY_NEW, b"{}".to_vec()),
    }
}

/// Control request: (command, JSON), keys in tinytuya's order.
fn control_json(t: &Target, dps: &Map<String, Value>, now: u64) -> Result<(u32, Vec<u8>)> {
    let dps = serde_json::to_string(dps)?;
    Ok(match t.version {
        Version::V33 => {
            let id = Value::from(t.id.as_str());
            let json = format!(r#"{{"devId":{id},"uid":{id},"t":"{now}","dps":{dps}}}"#);
            (CONTROL, json.into_bytes())
        }
        Version::V35 => {
            let json = format!(r#"{{"protocol":5,"t":{now},"data":{{"dps":{dps}}}}}"#);
            (CONTROL_NEW, json.into_bytes())
        }
    })
}

/// `dps`, or `data.dps` as 3.4+ devices nest it.
fn take_dps(v: &mut Value) -> Option<Map<String, Value>> {
    let dps = match v.get("dps") {
        Some(_) => v.get_mut("dps"),
        None => v.get_mut("data").and_then(|d| d.get_mut("dps")),
    };
    match dps.map(Value::take) {
        Some(Value::Object(m)) => Some(m),
        _ => None,
    }
}

// ---- Frames -------------------------------------------------------------------------

/// A whole client→device frame. Only CONTROL commands carry the "3.x" version header.
fn encode(
    v: Version,
    key: &[u8; 16],
    seq: u32,
    cmd: u32,
    json: &[u8],
    iv: [u8; 12],
) -> Result<Vec<u8>> {
    let mut header = [0u8; 15];
    header[..3].copy_from_slice(v.as_str().as_bytes());
    let with_header = matches!(cmd, CONTROL | CONTROL_NEW);
    match v {
        Version::V33 => {
            let mut payload = Vec::new();
            if with_header {
                payload.extend_from_slice(&header);
            }
            payload.extend(ecb_encrypt(key, json));
            Ok(pack_55aa(seq, cmd, &payload))
        }
        Version::V35 => {
            let mut plain = Vec::new();
            if with_header {
                plain.extend_from_slice(&header);
            }
            plain.extend_from_slice(json);
            pack_6699(key, seq, cmd, &plain, iv)
        }
    }
}

/// Decodes a whole device→client frame (either format).
fn decode(key: &[u8; 16], frame: &[u8]) -> std::result::Result<Msg, Garbled> {
    if frame.starts_with(&PREFIX_6699) {
        let (_, cmd, plain) = unpack_6699(key, frame).ok_or(Garbled::Key)?;
        let (retcode, rest) = split_retcode(&plain);
        // A version header here is inside the encryption; `Conn::parse` drops it.
        Ok(Msg {
            cmd,
            retcode,
            data: rest.to_vec(),
        })
    } else {
        let (_, cmd, payload) = unpack_55aa(frame).ok_or(Garbled::Crc)?;
        let (retcode, rest) = split_retcode(payload);
        let rest = strip_version_header(rest);
        let data = if rest.is_empty() {
            Vec::new()
        } else {
            ecb_decrypt(key, rest).ok_or(Garbled::Key)?
        };
        Ok(Msg { cmd, retcode, data })
    }
}

/// Device→client payloads start with a 4-byte return code; tinytuya always strips it. Keep
/// the payload whole if it visibly starts with JSON or a version header instead.
fn split_retcode(p: &[u8]) -> (Option<u32>, &[u8]) {
    if p.len() >= 4 && p[0] != b'{' && !p.starts_with(b"3.") {
        (Some(be_u32(p)), &p[4..])
    } else {
        (None, p)
    }
}

/// Removes the 15-byte `"3.x"` header (CONTROL and pushed STATUS payloads). We send 12 NULs
/// after the version, but devices fill those bytes, so like tinytuya only the prefix counts.
fn strip_version_header(p: &[u8]) -> &[u8] {
    if p.len() >= 15 && p.starts_with(b"3.") && p[2].is_ascii_digit() {
        &p[15..]
    } else {
        p
    }
}

fn be_u32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn push_u32(f: &mut Vec<u8>, n: u32) {
    f.extend_from_slice(&n.to_be_bytes());
}

/// `55AA` frame: prefix, seq, cmd, length (payload + CRC + suffix), payload, CRC32, suffix.
fn pack_55aa(seq: u32, cmd: u32, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(payload.len() + 24);
    f.extend_from_slice(&PREFIX_55AA);
    push_u32(&mut f, seq);
    push_u32(&mut f, cmd);
    push_u32(&mut f, payload.len() as u32 + 8);
    f.extend_from_slice(payload);
    let crc = crc32fast::hash(&f);
    push_u32(&mut f, crc);
    f.extend_from_slice(&SUFFIX_55AA);
    f
}

/// `6699` frame: prefix, u16 0, seq, cmd, length (IV + ciphertext + tag), IV, ciphertext,
/// GCM tag, suffix. The header after the prefix is the GCM associated data.
fn pack_6699(key: &[u8; 16], seq: u32, cmd: u32, plain: &[u8], iv: [u8; 12]) -> Result<Vec<u8>> {
    let mut f = Vec::with_capacity(plain.len() + 50);
    f.extend_from_slice(&PREFIX_6699);
    f.extend_from_slice(&[0, 0]);
    push_u32(&mut f, seq);
    push_u32(&mut f, cmd);
    push_u32(&mut f, plain.len() as u32 + 12 + 16);
    let sealed = gcm_seal(key, &iv, &f[4..], plain)?;
    f.extend_from_slice(&iv);
    f.extend_from_slice(&sealed);
    f.extend_from_slice(&SUFFIX_6699);
    Ok(f)
}

/// (seq, cmd, payload) of a `55AA` frame; None if it's short or the CRC is wrong.
fn unpack_55aa(f: &[u8]) -> Option<(u32, u32, &[u8])> {
    let total = 16 + be_u32(f.get(12..16)?) as usize;
    if total < 24 || f.len() < total {
        return None;
    }
    let body = total - 8;
    if be_u32(&f[body..]) != crc32fast::hash(&f[..body]) {
        return None;
    }
    Some((be_u32(&f[4..]), be_u32(&f[8..]), &f[16..body]))
}

/// (seq, cmd, decrypted payload) of a `6699` frame; None if it's short or fails GCM
/// authentication (wrong key).
fn unpack_6699(key: &[u8; 16], f: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let total = 18 + be_u32(f.get(14..18)?) as usize + 4;
    if total < 18 + 12 + 16 + 4 || f.len() < total {
        return None;
    }
    let plain = gcm_open(key, &f[18..30], &f[4..18], &f[30..total - 4])?;
    Some((be_u32(&f[6..]), be_u32(&f[10..]), plain))
}

/// Pulls the next whole frame out of a receive buffer, dropping bytes before a prefix.
/// None until enough bytes have arrived.
fn take_frame(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    loop {
        let Some(start) = buf
            .windows(4)
            .position(|w| w == PREFIX_55AA || w == PREFIX_6699)
        else {
            // Keep a possible partial prefix.
            buf.drain(..buf.len().saturating_sub(3));
            return None;
        };
        buf.drain(..start);
        let (header, extra) = if buf.starts_with(&PREFIX_55AA) {
            (16, 0)
        } else {
            (18, 4)
        };
        if buf.len() < header {
            return None;
        }
        let len = be_u32(&buf[header - 4..header]) as usize;
        if len > MAX_FRAME {
            buf.drain(..4); // a bogus prefix inside garbage; resync
            continue;
        }
        let total = header + len + extra;
        if buf.len() < total {
            return None;
        }
        return Some(buf.drain(..total).collect());
    }
}

// ---- Crypto -------------------------------------------------------------------------

fn random(buf: &mut [u8]) -> Result<()> {
    getrandom::fill(buf).map_err(|e| Error::msg(format!("no randomness available: {e}")))
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Result<[u8; 32]> {
    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(key).map_err(|_| Error::msg("bad HMAC key"))?;
    mac.update(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(&mac.finalize().into_bytes());
    Ok(out)
}

/// The 3.5 session key: AES-GCM of (our nonce XOR device nonce) under the local key with
/// the first 12 bytes of our nonce as IV, keeping the 16 ciphertext bytes (tag dropped).
fn session_key(real: &[u8; 16], local: &[u8; 16], remote: &[u8; 16]) -> Result<[u8; 16]> {
    let mut x = [0u8; 16];
    for (o, (a, b)) in x.iter_mut().zip(local.iter().zip(remote)) {
        *o = a ^ b;
    }
    let sealed = gcm_seal(real, &local[..12], &[], &x)?;
    let mut k = [0u8; 16];
    k.copy_from_slice(&sealed[..16]);
    Ok(k)
}

/// AES-128-ECB with PKCS#7 padding.
fn ecb_encrypt(key: &[u8; 16], plain: &[u8]) -> Vec<u8> {
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let pad = 16 - plain.len() % 16;
    let mut out = plain.to_vec();
    out.resize(plain.len() + pad, pad as u8);
    for block in out.as_chunks_mut::<16>().0 {
        cipher.encrypt_block(GenericArray::from_mut_slice(block));
    }
    out
}

/// AES-128-ECB decrypt; like tinytuya only the last padding byte is checked.
fn ecb_decrypt(key: &[u8; 16], data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() || !data.len().is_multiple_of(16) {
        return None;
    }
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut out = data.to_vec();
    for block in out.as_chunks_mut::<16>().0 {
        cipher.decrypt_block(GenericArray::from_mut_slice(block));
    }
    let pad = usize::from(*out.last()?);
    if !(1..=16).contains(&pad) {
        return None;
    }
    out.truncate(out.len() - pad);
    Some(out)
}

/// AES-128-GCM: ciphertext followed by the 16-byte tag. `iv` must be 12 bytes.
fn gcm_seal(key: &[u8; 16], iv: &[u8], aad: &[u8], plain: &[u8]) -> Result<Vec<u8>> {
    Aes128Gcm::new(GenericArray::from_slice(key))
        .encrypt(Nonce::from_slice(iv), Payload { msg: plain, aad })
        .map_err(|_| Error::msg("AES-GCM encryption failed"))
}

fn gcm_open(key: &[u8; 16], iv: &[u8], aad: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
    Aes128Gcm::new(GenericArray::from_slice(key))
        .decrypt(Nonce::from_slice(iv), Payload { msg: sealed, aad })
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fake device used for the vectors below (generated with tinytuya 1.20).
    const ID: &str = "bf0123456789abcdefgh";
    const KEY: &[u8; 16] = b"0123456789abcdef";
    const T: u64 = 1_700_000_000;
    const IV: [u8; 12] = *b"0123456789ab";

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn target(v: Version) -> Target {
        Target {
            id: ID.into(),
            ip: "127.0.0.1".into(),
            key: "0123456789abcdef".into(),
            version: v,
        }
    }

    fn dp20(on: bool) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("20".into(), Value::Bool(on));
        m
    }

    #[test]
    fn crc32_is_the_standard_one() {
        assert_eq!(crc32fast::hash(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn v33_control_frame_matches_tinytuya() {
        let (cmd, json) = control_json(&target(Version::V33), &dp20(true), T).unwrap();
        assert_eq!(cmd, CONTROL);
        let frame = encode(Version::V33, KEY, 5, cmd, &json, IV).unwrap();
        assert_eq!(frame, unhex("000055aa000000050000000700000087332e33000000000000000000000000cf86245f80a43fd581e9f874f8bade17f5a6c29e7d2e67261b300789f2779133ddabaa66ded5463195ff5d708d161f7ae15135b063629f655b8e969d70a003c614289c9042f2aa08005786aab66fcc6dd2aabe6f348ff0b442995f5c9f24d0ec377222e061a924c591cd9c27ea163ed49af2cb4a0000aa55"));
    }

    #[test]
    fn v33_dp_query_frame_matches_tinytuya() {
        let (cmd, json) = query_json(&target(Version::V33), T);
        assert_eq!(cmd, DP_QUERY);
        let frame = encode(Version::V33, KEY, 6, cmd, &json, IV).unwrap();
        assert_eq!(frame, unhex("000055aa000000060000000a00000078a582c52e5a14f42dc57e6b4371d0f7898f84c1b859b79478a8eff0e87abb16691d6e826df5b2d248589c9b1fe24d857c4d81a1c5a9ed3374f5bbddd8805697471289985ddcc6ad4334e0f2be9c78211b3257c3855eea3abd3a03de1f21fbaf54be23f34440aefb47c4fa7fbe7027af0171728b100000aa55"));
    }

    #[test]
    fn v33_replies_decode_with_or_without_version_header() {
        // DP_QUERY reply: retcode + ECB(json).
        let reply = unhex("000055aa000000090000000a0000004c00000000cf86245f80a43fd581e9f874f8bade17f5a6c29e7d2e67261b300789f27791337dafb7b4370dab264878a7dbed69289118d350f83fa0882d83c53bb3c3f1a5aa6b0af7130000aa55");
        let m = decode(KEY, &reply).unwrap();
        assert_eq!((m.cmd, m.retcode), (DP_QUERY, Some(0)));
        let mut v: Value = serde_json::from_slice(&m.data).unwrap();
        assert_eq!(take_dps(&mut v).unwrap()["21"], "white");
        // Pushed STATUS: retcode + "3.3" header + ECB(json).
        let pushed = unhex("000055aa0000000a000000080000004b00000000332e33000000000000000000000000c756271dbf61895af70ee82ef0d1b12add5ebbf5251d8294dc245620871e66c044617062178312f97af429c63ca293b979ce63590000aa55");
        let m = decode(KEY, &pushed).unwrap();
        assert_eq!(m.cmd, 8);
        assert_eq!(m.data, br#"{"dps":{"20":false},"t":1700000000}"#);
        // Real devices fill the 12 bytes after "3.3" (seen on pushed STATUS frames).
        let mut p = vec![0, 0, 0, 0];
        p.extend_from_slice(b"3.3\0\0\0\0\0\0\x12\x25\0\0\0\x01");
        p.extend(ecb_encrypt(KEY, br#"{"dps":{"20":true}}"#));
        let m = decode(KEY, &pack_55aa(3, 8, &p)).unwrap();
        assert_eq!(m.data, br#"{"dps":{"20":true}}"#);
        // Wrong key, and a flipped bit.
        assert_eq!(
            decode(b"fedcba9876543210", &reply).unwrap_err(),
            Garbled::Key
        );
        let mut bad = reply.clone();
        bad[30] ^= 1;
        assert_eq!(decode(KEY, &bad).unwrap_err(), Garbled::Crc);
    }

    #[test]
    fn v35_frame_matches_tinytuya_and_round_trips() {
        let session = b"sessionkey123456";
        let (cmd, json) = control_json(&target(Version::V35), &dp20(true), T).unwrap();
        assert_eq!(cmd, CONTROL_NEW);
        assert_eq!(
            json,
            br#"{"protocol":5,"t":1700000000,"data":{"dps":{"20":true}}}"#
        );
        let frame = encode(Version::V35, session, 7, cmd, &json, IV).unwrap();
        assert_eq!(frame, unhex("000066990000000000070000000d00000063303132333435363738396162c7c52aa2ef340ac079403533d7ef3939865ab21e12dd14616f08fb1b2ce38d269fc1a8652ed3e6d58c9d05dbac411df7a84c0cdd2361c1a62944742eee61a14ea0d9d713d0d33280e32c46d36195ad5ca39bc11747acd900009966"));
        let m = decode(session, &frame).unwrap();
        assert_eq!((m.cmd, m.retcode), (CONTROL_NEW, None));
        assert_eq!(strip_version_header(&m.data), json);
        assert_eq!(decode(KEY, &frame).unwrap_err(), Garbled::Key);
        assert_eq!(
            query_json(&target(Version::V35), T),
            (DP_QUERY_NEW, b"{}".to_vec())
        );
    }

    #[test]
    fn v35_session_key_matches_tinytuya() {
        let local = b"0123456789abcdef";
        let remote = b"fedcba9876543210";
        // The device's step-2 reply (retcode + its nonce + HMAC of ours), from tinytuya.
        let resp = unhex("00006699000000000001000000040000005030313233343536373839616262420dfb480549ea61624edcb3ec96c7a5ba475891e06321f8bfc69df1f5dd2c00034879bcccc26eabcdb754811230d08898cac47b83b72d6b1febf6fba5a9f80f77dbbf00009966");
        let m = decode(KEY, &resp).unwrap();
        assert_eq!((m.cmd, m.retcode), (SESS_KEY_NEG_RESP, Some(0)));
        assert_eq!(&m.data[..16], remote);
        assert_eq!(m.data[16..48], hmac_sha256(KEY, local).unwrap());
        assert_eq!(
            hmac_sha256(KEY, local).unwrap().to_vec(),
            unhex("fb5b26229c20b7ed866706a2fbfae67e3f404bb6abe77ff45063a459a42924a4")
        );
        assert_eq!(
            hmac_sha256(KEY, remote).unwrap().to_vec(),
            unhex("0de373b876d33f8102b2a67aa43aaa88161b7606292f33f47db3b67fd5e2bf4e")
        );
        assert_eq!(
            session_key(KEY, local, remote).unwrap().to_vec(),
            unhex("34165bab783422860c0c23b2d48cf7a5")
        );
    }

    #[test]
    fn broadcasts_decrypt() {
        assert_eq!(
            udp_key().to_vec(),
            unhex("6c1ec8e2bb9bb59ab50b0daf649b410a")
        );
        let from: IpAddr = "192.168.1.9".parse().unwrap();
        // 3.3: 55AA, retcode, ECB under the UDP key.
        let b33 = unhex("000055aa00000000000000130000009c00000000d09766676f3369eb10b5e9f132fd802a70172896956a9e330910344d11ac363f482611d8caa5248ac6cea65bdad007dfa41f010dca2abb30c50459a9f88024ffa483be81e77f2e71f4307bd81685095e7e6fd47351a96972bdc91a04690e1ee1409cbc81d29df0716db156e2b74139632ba7f8bbc5f9653b112ef58ed2340fbc4b95ac1f0a68b021b552634baf99a3c59f7e3fd70000aa55");
        let s = parse_broadcast(&b33, from).unwrap();
        assert_eq!(
            (s.id.as_str(), s.ip.as_str(), s.version.as_str()),
            (ID, "192.168.1.50", "3.3")
        );
        // 3.5: 6699, GCM under the UDP key.
        let b35 = unhex("0000669900000000000000000013000000c6303132333435363738396162042ce52c1986e9abb3bec8436999f733293fda25acbd8c0b2851e6e71da00fd51b5f80a72f672c34128034051fcfa1e5bcc151eb9b5761ea8438f6a0287e56cf7759c433b17b18e1c0956212d5cd72bca73a8a876c6dd71237cf3c425ce06cef0aa6f8484e5241b3d492a60f675ef58652307579b04711c43da67fbe45e6133038f704fbf27c40460733fec94796bfc7ff01cd75e3175beb19c8e0a7900cda68a483e7e3c35cf6e3537185b91eb61fd20cbf6c91d1d0e45da5c000009966");
        let s = parse_broadcast(&b35, from).unwrap();
        assert_eq!(
            (s.id.as_str(), s.ip.as_str(), s.version.as_str()),
            ("eb0123456789abcdefghij", "192.168.1.51", "3.5")
        );
        // App broadcasts and noise are ignored.
        let app = pack_6699(
            &udp_key(),
            0,
            0x25,
            br#"{"from":"app","ip":"192.168.1.2"}"#,
            IV,
        )
        .unwrap();
        assert!(parse_broadcast(&app, from).is_none());
        assert!(parse_broadcast(b"hello", from).is_none());
    }

    #[test]
    fn frames_are_found_in_a_noisy_stream() {
        let a = pack_55aa(1, 9, &[0, 0, 0, 0]);
        let b = pack_6699(KEY, 2, 8, b"{}", IV).unwrap();
        let mut buf = b"\x01\x02junk".to_vec();
        buf.extend_from_slice(&a);
        buf.extend_from_slice(&b[..10]);
        assert_eq!(take_frame(&mut buf).unwrap(), a);
        assert!(take_frame(&mut buf).is_none());
        buf.extend_from_slice(&b[10..]);
        assert_eq!(take_frame(&mut buf).unwrap(), b);
        assert!(buf.is_empty());
        // An ack with nothing but the return code.
        let m = decode(KEY, &a).unwrap();
        assert_eq!((m.cmd, m.retcode, m.data.len()), (9, Some(0), 0));
    }

    #[test]
    fn versions_parse() {
        assert_eq!("3.3".parse::<Version>().unwrap(), Version::V33);
        assert_eq!("v3.5".parse::<Version>().unwrap(), Version::V35);
        assert!("3.4".parse::<Version>().is_err());
    }

    // ---- Real devices (cargo test -- --ignored). Keys are read at run time from the
    // smarthome-local.json of the data folder `TVL_DATA` names, never printed, never in the repo.
    // Bulbs are picked by protocol version, not by address (DHCP moves them).

    fn devices_file() -> std::path::PathBuf {
        let dir = std::env::var_os("TVL_DATA").expect("set TVL_DATA to a data folder with smarthome-local.json");
        std::path::Path::new(&dir).join("smarthome-local.json")
    }

    /// The first bulb speaking protocol `version` ("3.3" or "3.5").
    fn real(version: &str) -> Target {
        let text = std::fs::read_to_string(devices_file()).unwrap();
        let store: Value = serde_json::from_str(&text).unwrap();
        let list = store["tuya"].as_array().unwrap();
        let d = list.iter().find(|d| d["version"].as_str().is_some_and(|v| v.starts_with(version))).unwrap();
        let s = |k: &str| d[k].as_str().unwrap().to_owned();
        Target {
            id: s("id"),
            ip: s("ip"),
            key: s("key"),
            version: s("version").parse().unwrap(),
        }
    }

    async fn status_has_dp20(version: &str) {
        let t = real(version);
        let started = std::time::Instant::now();
        let dps = status(&t).await.unwrap();
        println!(
            "{} v{} status in {:?}: {}",
            t.ip,
            t.version,
            started.elapsed(),
            Value::Object(dps.clone())
        );
        assert!(dps.contains_key("20"));
    }

    #[tokio::test]
    #[ignore]
    async fn real_status_v33_bulb() {
        status_has_dp20("3.3").await;
    }

    #[tokio::test]
    #[ignore]
    async fn real_status_v35_bulb() {
        status_has_dp20("3.5").await;
    }

    /// Re-sends DP 20 with the value the bulb already has: a visible no-op.
    async fn noop_set(version: &str) {
        let t = real(version);
        let dps = status(&t).await.unwrap();
        let mut same = Map::new();
        same.insert("20".into(), dps["20"].clone());
        let started = std::time::Instant::now();
        set(&t, same.clone()).await.unwrap();
        println!(
            "{} v{} set {} ok in {:?}",
            t.ip,
            t.version,
            Value::Object(same),
            started.elapsed()
        );
    }

    #[tokio::test]
    #[ignore]
    async fn real_noop_set_v33_bulb() {
        noop_set("3.3").await;
    }

    #[tokio::test]
    #[ignore]
    async fn real_noop_set_v35_bulb() {
        noop_set("3.5").await;
    }

    #[tokio::test]
    #[ignore]
    async fn real_discover() {
        let seen = discover(Duration::from_secs(6)).await.unwrap();
        for s in &seen {
            println!("{} {} v{}", s.id, s.ip, s.version);
        }
        assert!(seen.len() >= 2, "only {} devices heard", seen.len());
    }
}
