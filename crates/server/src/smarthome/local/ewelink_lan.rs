//! Sonoff / eWeLink devices over the LAN, without the eWeLink cloud.
//!
//! Devices announce themselves over mDNS as `_ewelink._tcp.local`; their TXT record carries the
//! current params (switch states, ...) and is the only way to read state on devices that refuse
//! `info`/`getState`. Commands go to `POST http://<ip>:8081/zeroconf/<command>`.
//!
//! In "encrypted LAN mode" both directions use AES-128-CBC with key = md5(devicekey) and PKCS#7
//! padding, with the IV sent base64-encoded next to the data.

use tvl_kit::{Error, Result};
use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use md5::{Digest, Md5};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::future::poll_fn;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::task::Poll;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::ReadBuf;
use tokio::net::UdpSocket;
use tokio::time::{timeout_at, Instant};

type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

const SERVICE: &str = "_ewelink._tcp.local";
const MDNS_GROUP: SocketAddr =
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(224, 0, 0, 251), 5353));
/// Dozing ESP32 devices miss queries, so `discover` repeats its query this often.
const QUERY_EVERY: Duration = Duration::from_secs(1);
/// A dozing ESP32 can stall TCP connects for 5-10 s; three short attempts stay under ~15 s.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(5);
const ATTEMPTS: u32 = 3;

const TYPE_A: u16 = 1;
const TYPE_PTR: u16 = 12;
const TYPE_TXT: u16 = 16;
const TYPE_SRV: u16 = 33;

/// An eWeLink device seen on the LAN, from its mDNS announcement.
#[derive(Debug, Clone)]
pub struct Found {
    pub deviceid: String,
    pub ip: String,
    pub port: u16,
    pub encrypted: bool,
    /// Base64 IV for `data`, when encrypted.
    pub iv: Option<String>,
    /// TXT data1..dataN joined in order, still encrypted (base64) when `encrypted`.
    pub data: Option<String>,
}

/// Browses `_ewelink._tcp.local` for `window` and returns the devices that answered, one per
/// deviceid (the latest answer wins).
///
/// Queries ask for unicast replies (QU) from an ephemeral port, which Windows Firewall lets back
/// in; they go out both on the default multicast interface and from the LAN address of the
/// default route, since Windows may pick a link-local or virtual adapter for multicast.
pub async fn discover(window: Duration) -> Result<Vec<Found>> {
    let deadline = Instant::now() + window;
    let sockets = query_sockets().await?;
    let query = ptr_query(SERVICE);
    let mut buf = vec![0u8; 9000];
    let mut found: Vec<Found> = Vec::new();
    let mut next_query = Instant::now();
    while Instant::now() < deadline {
        if Instant::now() >= next_query {
            for socket in &sockets {
                // A socket on an interface with no route just fails; the other may still work.
                let _ = socket.send_to(&query, MDNS_GROUP).await;
            }
            next_query += QUERY_EVERY;
        }
        let wait_until = next_query.min(deadline);
        if let Ok(Ok((len, from))) = timeout_at(wait_until, recv_any(&sockets, &mut buf)).await {
            for device in parse_response(&buf[..len], from.ip()) {
                match found.iter_mut().find(|f| f.deviceid == device.deviceid) {
                    Some(seen) => *seen = device,
                    None => found.push(device),
                }
            }
        }
    }
    Ok(found)
}

/// The device's params (e.g. `{"switches":[{"switch":"on","outlet":0},...]}`) from its TXT
/// data, decrypted with its devicekey (or parsed as is when the device isn't encrypting).
pub fn decrypt_params(devicekey: &str, found: &Found) -> Result<Value> {
    let id = &found.deviceid;
    let missing = |what: &str| Error::msg(format!("eWeLink {id}: no {what} in its mDNS record"));
    let data = found.data.as_deref().ok_or_else(|| missing("data"))?;
    if !found.encrypted {
        return Ok(serde_json::from_str(data)?);
    }
    let iv = found.iv.as_deref().ok_or_else(|| missing("iv"))?;
    let plain = decrypt_b64(&aes_key(devicekey), iv, data)?;
    Ok(serde_json::from_slice(&plain)?)
}

/// Sends `params` to `POST http://ip:port/zeroconf/<command>` (e.g. "switches" with
/// `{"switches":[{"switch":"on","outlet":0}]}`), encrypted with the devicekey.
///
/// Returns the reply's decrypted `data`, if it has any, or an error carrying the device's
/// error code when it isn't 0. Connects that stall are retried (the device dozes on Wi-Fi).
/// One client for every Sonoff command (the old launcher made one per command).
fn client() -> Result<reqwest::Client> {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    if let Some(c) = CLIENT.get() {
        return Ok(c.clone());
    }
    let c = reqwest::Client::builder()
        // The firmware's HTTP server never answers lowercase header names (hyper's default).
        .http1_title_case_headers()
        .no_proxy()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(ATTEMPT_TIMEOUT)
        .build()?;
    Ok(CLIENT.get_or_init(|| c).clone())
}

pub async fn send(
    ip: &str,
    port: u16,
    deviceid: &str,
    devicekey: &str,
    command: &str,
    params: Value,
) -> Result<Option<Value>> {
    let key = aes_key(devicekey);
    let url = format!("http://{ip}:{port}/zeroconf/{command}");
    let client = client()?;
    let mut attempt = 1;
    let reply: Value = loop {
        let body = request_body(&key, deviceid, &random_iv()?, &params, &now_ms())?;
        match client.post(&url).json(&body).send().await {
            Ok(response) => break response.json().await?,
            Err(e) if attempt < ATTEMPTS && (e.is_connect() || e.is_timeout()) => attempt += 1,
            Err(e) => return Err(e.into()),
        }
    };
    parse_reply(&key, command, &reply)
}

// ---- crypto ----

fn aes_key(devicekey: &str) -> [u8; 16] {
    Md5::digest(devicekey.as_bytes()).into()
}

fn encrypt(key: &[u8; 16], iv: &[u8; 16], plain: &[u8]) -> Vec<u8> {
    Aes128CbcEnc::new(key.into(), iv.into()).encrypt_padded_vec_mut::<Pkcs7>(plain)
}

fn decrypt(key: &[u8; 16], iv: &[u8; 16], cipher: &[u8]) -> Result<Vec<u8>> {
    Aes128CbcDec::new(key.into(), iv.into())
        .decrypt_padded_vec_mut::<Pkcs7>(cipher)
        .map_err(|_| Error::msg("eWeLink: can't decrypt (wrong devicekey?)"))
}

fn decrypt_b64(key: &[u8; 16], iv: &str, data: &str) -> Result<Vec<u8>> {
    let iv: [u8; 16] = b64(iv)?
        .try_into()
        .map_err(|_| Error::msg("eWeLink: IV is not 16 bytes"))?;
    decrypt(key, &iv, &b64(data)?)
}

fn b64(s: &str) -> Result<Vec<u8>> {
    B64.decode(s.trim())
        .map_err(|e| Error::msg(format!("eWeLink: bad base64: {e}")))
}

fn random_iv() -> Result<[u8; 16]> {
    let mut iv = [0u8; 16];
    getrandom::fill(&mut iv).map_err(|e| Error::msg(format!("random IV: {e}")))?;
    Ok(iv)
}

// ---- HTTP ----

fn now_ms() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    now.as_millis().to_string()
}

fn request_body(
    key: &[u8; 16],
    deviceid: &str,
    iv: &[u8; 16],
    params: &Value,
    sequence: &str,
) -> Result<Value> {
    let data = encrypt(key, iv, &serde_json::to_vec(params)?);
    Ok(json!({
        "sequence": sequence,
        "deviceid": deviceid,
        "selfApikey": "123",
        "encrypt": true,
        "iv": B64.encode(iv),
        "data": B64.encode(data),
    }))
}

/// Checks a /zeroconf reply's `error` and decrypts its `data`, if any.
fn parse_reply(key: &[u8; 16], command: &str, reply: &Value) -> Result<Option<Value>> {
    let code = reply.get("error").and_then(Value::as_i64).unwrap_or(0);
    if code != 0 {
        return Err(Error::msg(format!(
            "eWeLink {command}: device error {code}"
        )));
    }
    match reply.get("data") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(data)) if data.is_empty() => Ok(None),
        Some(Value::String(data)) if reply["encrypt"] == true => {
            let iv = reply.get("iv").and_then(Value::as_str).unwrap_or_default();
            Ok(Some(serde_json::from_slice(&decrypt_b64(key, iv, data)?)?))
        }
        Some(Value::String(data)) => Ok(Some(serde_json::from_str(data)?)),
        Some(other) => Ok(Some(other.clone())),
    }
}

// ---- mDNS ----

/// One socket on the OS's default multicast interface, plus one bound to the LAN address of
/// the default route (binding picks the outgoing interface for multicast on Windows).
async fn query_sockets() -> Result<Vec<UdpSocket>> {
    let mut sockets = vec![UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?];
    if let Some(ip) = default_route_ip().await {
        if let Ok(socket) = UdpSocket::bind((ip, 0)).await {
            sockets.push(socket);
        }
    }
    for socket in &sockets {
        let _ = socket.set_multicast_ttl_v4(255); // RFC 6762 asks for TTL 255
    }
    Ok(sockets)
}

/// The local IPv4 address used for internet traffic (connecting a UDP socket sends nothing).
async fn default_route_ip() -> Option<Ipv4Addr> {
    let probe = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await.ok()?;
    probe.connect((Ipv4Addr::new(1, 1, 1, 1), 53)).await.ok()?;
    match probe.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// Waits for a datagram on any of the sockets.
async fn recv_any(sockets: &[UdpSocket], buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
    poll_fn(|cx| {
        for socket in sockets {
            let mut read = ReadBuf::new(&mut *buf);
            if let Poll::Ready(result) = socket.poll_recv_from(cx, &mut read) {
                return Poll::Ready(result.map(|from| (read.filled().len(), from)));
            }
        }
        Poll::Pending
    })
    .await
}

/// A one-question mDNS query for `name`'s PTR records, asking for a unicast reply (QU bit).
fn ptr_query(name: &str) -> Vec<u8> {
    let mut query = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0]; // id 0, flags 0, 1 question
    for label in name.split('.') {
        query.push(label.len() as u8);
        query.extend_from_slice(label.as_bytes());
    }
    query.push(0);
    query.extend_from_slice(&TYPE_PTR.to_be_bytes());
    query.extend_from_slice(&0x8001u16.to_be_bytes()); // QU + class IN
    query
}

struct Record<'a> {
    name: String,
    rtype: u16,
    /// Offset of the rdata in the message, for names compressed against earlier bytes.
    at: usize,
    rdata: &'a [u8],
}

fn be16(msg: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*msg.get(at)?, *msg.get(at + 1)?]))
}

/// Reads a possibly compressed name at `pos`; returns it and the offset just past it.
fn read_name(msg: &[u8], mut pos: usize) -> Option<(String, usize)> {
    let mut labels = Vec::new();
    let mut end = None;
    // Bounded, so a pointer loop in a malformed packet can't spin forever.
    for _ in 0..128 {
        let len = usize::from(*msg.get(pos)?);
        if len == 0 {
            return Some((labels.join("."), end.unwrap_or(pos + 1)));
        } else if len & 0xC0 == 0xC0 {
            end.get_or_insert(pos + 2);
            pos = usize::from(be16(msg, pos)? & 0x3FFF);
        } else if len < 64 {
            labels.push(String::from_utf8_lossy(msg.get(pos + 1..pos + 1 + len)?).into_owned());
            pos += 1 + len;
        } else {
            return None;
        }
    }
    None
}

/// Every resource record in a DNS response (answers, authority and additional).
fn records(msg: &[u8]) -> Option<Vec<Record<'_>>> {
    if be16(msg, 2)? & 0x8000 == 0 {
        return None; // a query (maybe with stale known answers), not a response
    }
    let questions = be16(msg, 4)?;
    let count =
        usize::from(be16(msg, 6)?) + usize::from(be16(msg, 8)?) + usize::from(be16(msg, 10)?);
    let mut pos = 12;
    for _ in 0..questions {
        pos = read_name(msg, pos)?.1 + 4;
    }
    let mut records = Vec::new();
    for _ in 0..count {
        let (name, p) = read_name(msg, pos)?;
        let (rtype, len) = (be16(msg, p)?, usize::from(be16(msg, p + 8)?));
        let at = p + 10;
        records.push(Record {
            name,
            rtype,
            at,
            rdata: msg.get(at..at + len)?,
        });
        pos = at + len;
    }
    Some(records)
}

/// TXT strings as key → value; keys lowercased (DNS-SD keys are case-insensitive).
fn txt_fields(rdata: &[u8]) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    let mut pos = 0;
    while let Some(&len) = rdata.get(pos) {
        let Some(entry) = rdata.get(pos + 1..pos + 1 + usize::from(len)) else {
            break;
        };
        let entry = String::from_utf8_lossy(entry);
        let (key, value) = entry.split_once('=').unwrap_or((&*entry, ""));
        fields.insert(key.to_ascii_lowercase(), value.to_string());
        pos += 1 + usize::from(len);
    }
    fields
}

fn is_ewelink_instance(name: &str) -> bool {
    let (name, suffix) = (name.as_bytes(), b"._ewelink._tcp.local");
    name.len() > suffix.len() && name[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

/// The eWeLink devices in one mDNS response. `from` (the sender) stands in for the IP when the
/// response has no A record for the device's host.
fn parse_response(msg: &[u8], from: IpAddr) -> Vec<Found> {
    let Some(records) = records(msg) else {
        return Vec::new();
    };
    let find = |rtype: u16, name: &str| {
        records
            .iter()
            .find(|r| r.rtype == rtype && r.name.eq_ignore_ascii_case(name))
    };
    let txts = records
        .iter()
        .filter(|r| r.rtype == TYPE_TXT && is_ewelink_instance(&r.name));
    txts.filter_map(|txt| {
        let fields = txt_fields(txt.rdata);
        let srv = find(TYPE_SRV, &txt.name);
        let ip = srv
            .and_then(|srv| read_name(msg, srv.at + 6)) // SRV: priority, weight, port, target
            .and_then(|(host, _)| find(TYPE_A, &host))
            .and_then(|a| <[u8; 4]>::try_from(a.rdata).ok())
            .map_or(from, IpAddr::from);
        // Long params are split over data1, data2, ... (TXT strings max out at 255 bytes).
        let mut chunks: Vec<(u32, &str)> = fields
            .iter()
            .filter_map(|(k, v)| Some((k.strip_prefix("data")?.parse().ok()?, v.as_str())))
            .collect();
        chunks.sort_unstable();
        Some(Found {
            deviceid: fields.get("id")?.clone(),
            ip: ip.to_string(),
            port: srv.and_then(|srv| be16(msg, srv.at + 4)).unwrap_or(8081),
            encrypted: fields.get("encrypt").is_some_and(|v| v == "true"),
            iv: fields.get("iv").cloned(),
            data: (!chunks.is_empty()).then(|| chunks.iter().map(|(_, v)| *v).collect()),
        })
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Made up; the expected ciphertexts below came from lan.py's `crypt` with this key.
    const KEY: &str = "11111111-2222-3333-4444-555555555555";

    fn unhex(lines: &[&str]) -> Vec<u8> {
        let hex: String = lines.concat();
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn aes_cbc_round_trip() {
        let (key, iv) = (aes_key(KEY), [7u8; 16]);
        for len in [0, 1, 15, 16, 17, 100] {
            let plain: Vec<u8> = (0..len as u8).collect();
            let cipher = encrypt(&key, &iv, &plain);
            assert_eq!(
                cipher.len(),
                (len / 16 + 1) * 16,
                "PKCS#7 pads to the next whole block"
            );
            assert_eq!(decrypt(&key, &iv, &cipher).unwrap(), plain);
        }
        let cipher = encrypt(&key, &iv, b"hello");
        assert_ne!(
            decrypt(&aes_key("other key"), &iv, &cipher).ok(),
            Some(b"hello".to_vec())
        );
    }

    #[test]
    fn request_body_matches_lan_py() {
        // Keys already sorted, so the JSON text doesn't depend on serde_json's preserve_order.
        let params: Value =
            serde_json::from_str(r#"{"switches":[{"outlet":0,"switch":"on"}]}"#).unwrap();
        let iv: [u8; 16] = std::array::from_fn(|i| i as u8);
        let body =
            request_body(&aes_key(KEY), "1000abcdef", &iv, &params, "1700000000000").unwrap();
        assert_eq!(
            body,
            json!({
                "sequence": "1700000000000",
                "deviceid": "1000abcdef",
                "selfApikey": "123",
                "encrypt": true,
                "iv": "AAECAwQFBgcICQoLDA0ODw==",
                "data": "eV2egBPpxqOnBLCaoN8uK/JNfR+AEvj/dIvcWIepFlRf6Owrq6yVg0hwoVwJcX2M",
            })
        );
    }

    #[test]
    fn reply_errors_and_data() {
        let key = aes_key(KEY);
        let err = parse_reply(&key, "info", &json!({"seq": 1, "error": 400})).unwrap_err();
        assert_eq!(err.to_string(), "eWeLink info: device error 400");
        assert_eq!(
            parse_reply(&key, "switches", &json!({"seq": 2, "error": 0})).unwrap(),
            None
        );

        let iv = [3u8; 16];
        let data = B64.encode(encrypt(&key, &iv, br#"{"switch":"on"}"#));
        let reply = json!({"error": 0, "encrypt": true, "iv": B64.encode(iv), "data": data});
        assert_eq!(
            parse_reply(&key, "info", &reply).unwrap(),
            Some(json!({"switch": "on"}))
        );
    }

    #[test]
    fn query_packing() {
        let mut expected = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        expected.extend_from_slice(b"\x08_ewelink\x04_tcp\x05local\x00");
        expected.extend_from_slice(&[0, 12, 0x80, 1]); // PTR, QU + IN
        assert_eq!(ptr_query(SERVICE), expected);
    }

    // Two real replies from the M5 (1001f28fa2) to the same query; its mDNS stack compresses
    // names differently from one reply to the next (e.g. the SRV name is a bare pointer in one).
    const M5_REPLY_A: &[&str] = &[
        "000084000000000300000001085f6577656c696e6b045f746370056c6f63616c00000c000100001194001e126557654c",
        "696e6b5f31303031663238666132085f6577656c696e6bc015c02b00210001000000780020000000001f91126557654c",
        "696e6b5f31303031663238666132056c6f63616c00126557654c696e6b5f31303031663238666132085f6577656c696e",
        "6bc01500100001000011940111b264617461313d355579722b61504249365a36464d32694f426d61324c4c7763535567",
        "54546a5275746a664b4a376a3034694e476e364c78695a64553339343056384150616d434f446145554648534f4a6d7a",
        "2f683959663169336e4b7148316b4a352f6e36306547664b736f68794969654a62764e724347594b32456363686a752f",
        "43786c535164544b5649513551614b747670574c7a71774c69347549436f5a62785852504b47464c356c38377076673d",
        "087365713d313133311b69763d4d6a45774d546b354e4441794d44557a4e4449774d413d3d0c656e63727970743d7472",
        "756509617069766572733d3109747970653d706c75670d69643d3130303166323866613209747874766572733d31c05b",
        "00010001000000780004c0a84473",
    ];
    const M5_REPLY_B: &[&str] = &[
        "000084000000000300000001085f6577656c696e6b045f746370056c6f63616c00000c000100001194001e126557654c",
        "696e6b5f31303031663238666132085f6577656c696e6bc015126557654c696e6b5f31303031663238666132085f6577",
        "656c696e6bc01500210001000000780020000000001f91126557654c696e6b5f31303031663238666132056c6f63616c",
        "00126557654c696e6b5f31303031663238666132085f6577656c696e6bc01500100001000011940111b264617461313d",
        "355579722b61504249365a36464d32694f426d61324c4c776353556754546a5275746a664b4a376a3034694e476e364c",
        "78695a64553339343056384150616d434f446145554648534f4a6d7a2f683959663169336e4b7148316b4a352f6e3630",
        "6547664b736f68794969654a62764e724347594b32456363686a752f43786c535164544b5649513551614b747670574c",
        "7a71774c69347549436f5a62785852504b47464c356c38377076673d087365713d313133311b69763d4d6a45774d546b",
        "354e4441794d44557a4e4449774d413d3d0c656e63727970743d7472756509617069766572733d3109747970653d706c",
        "75670d69643d3130303166323866613209747874766572733d31c07700010001000000780004c0a84473",
    ];

    #[test]
    fn parses_captured_m5_replies() {
        let sender: IpAddr = Ipv4Addr::new(10, 9, 9, 9).into(); // the A record must win over this
        for reply in [M5_REPLY_A, M5_REPLY_B] {
            let found = parse_response(&unhex(reply), sender);
            assert_eq!(found.len(), 1);
            let m5 = &found[0];
            assert_eq!(m5.deviceid, "1001f28fa2");
            assert_eq!((m5.ip.as_str(), m5.port), ("192.168.68.115", 8081));
            assert!(m5.encrypted);
            assert_eq!(m5.iv.as_deref(), Some("MjEwMTk5NDAyMDUzNDIwMA=="));
            let data = m5.data.as_deref().unwrap();
            assert!(data.starts_with("5Uyr+aPBI6Z6") && data.ends_with("l87pvg="));
            assert_eq!(data.len(), 172);
        }
    }

    #[test]
    fn rejects_queries_and_garbage() {
        let sender: IpAddr = Ipv4Addr::LOCALHOST.into();
        assert!(parse_response(&ptr_query(SERVICE), sender).is_empty());
        let mut truncated = unhex(M5_REPLY_A);
        truncated.truncate(300);
        assert!(parse_response(&truncated, sender).is_empty());
        // A name pointing at itself must not loop forever.
        let mut looped = vec![0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0xC0, 12];
        looped.extend_from_slice(&[0, 16, 0, 1, 0, 0, 0, 0, 0, 0]);
        assert!(parse_response(&looped, sender).is_empty());
    }

    /// Params long enough to need data1..data4 (lan.py's `crypt`, KEY, IV "0123456789abcdef").
    const LONG_PARAMS: &str = r#"{"switches":[{"switch":"off","outlet":0},{"switch":"on","outlet":1},{"switch":"off","outlet":2},{"switch":"off","outlet":3}],"configure":[{"startup":"off","outlet":0},{"startup":"off","outlet":1},{"startup":"off","outlet":2},{"startup":"off","outlet":3}],"pulses":[{"pulse":"off","switch":"off","outlet":0,"width":1000},{"pulse":"off","switch":"off","outlet":1,"width":1000},{"pulse":"off","switch":"off","outlet":2,"width":1000},{"pulse":"off","switch":"off","outlet":3,"width":1000}],"sledOnline":"on","fwVersion":"1.2.2"}"#;
    const LONG_DATA: &str = concat!(
        "V4gx8bW/L4ag8YNdpxgDDwUs3VjElRBEiIDa8YYybdIKqpyzsqiaNNISHfovkyBLshcwkl0MT4d7FG6GKlKNfNGlJF9gR54X6YXM/nNYB05pu0jGchoTt+sd5oAzvolAop1kpBPkUXF5qKec7EEhK+g8UFGctCvcprfz/qWoJ5oUvjQzXXFrhAPEuTdLM5YL1BDMo4bwZvi7owgjYrQxAlWMmLMAHSKJmMAV58Ad3RA4fC/x0SQKE20EV",
        "Dqn8WUWfUkOYbfrSfYvrDtRvJIPF9B+UvqeCXHIpj2gXLt7yhPjH23fcWHvpNXIO4ktFM5w8NMr7/jYOiBsOkN3dxYLTp7k9wf/BdC8utXdMdvzkwjJ+/w30+wyqZxDWRmpyr6kPhLEPd0JmOdVdk06xtVsCfVLIcamAbC53Ri8YGMXaj/HxNGsbD1qGuSaeFfwZsJ+aHdgNA0+aLvjighdIkvNKW3vD3Pdne5tJwW0+ftFU+YOi/5b20",
        "V/tbtQlDGbBgrPrIS4OCPPIQsU6kGZitNY1Z24ifJmv0BoqKlNjDZaB3Tm7pteeaMdNrD2PDswgCVeE7Wd2ILa7p1hiPJh7aX1aAyYt5cH1oTb6ojKomrHQWrxnDq9iKjhiRS2QF5uts9fHj90i4ezjuZIgpiEZR+3SFHsM6HegrHvHUvRnrXC9KK09wgAzw4hvPJWh2usxiQ8",
    );

    /// A response with a PTR answer and a TXT whose name is a pointer into the PTR's rdata,
    /// its data split in four and listed out of order; no SRV or A record.
    fn split_txt_response() -> Vec<u8> {
        let mut msg = vec![0, 0, 0x84, 0, 0, 0, 0, 2, 0, 0, 0, 0];
        let mut answer = |name: &[u8], rtype: u16, rdata: &[u8]| {
            msg.extend_from_slice(name);
            msg.extend_from_slice(&rtype.to_be_bytes());
            msg.extend_from_slice(&[0x80, 1, 0, 0, 0x11, 0x94]); // cache-flush IN, TTL 4500
            msg.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
            msg.extend_from_slice(rdata);
        };
        // PTR at 12: _ewelink._tcp.local -> eWeLink_1000aabbcc.<ptr 12>; its rdata starts at 43.
        answer(
            b"\x08_ewelink\x04_tcp\x05local\x00",
            TYPE_PTR,
            b"\x12eWeLink_1000aabbcc\xc0\x0c",
        );
        let chunks: Vec<&str> = (0..4)
            .map(|i| &LONG_DATA[i * 200..LONG_DATA.len().min(i * 200 + 200)])
            .collect();
        let mut txt = Vec::new();
        for (key, value) in [
            ("data3", chunks[2]),
            ("id", "1000aabbcc"),
            ("data1", chunks[0]),
            ("type", "strip"),
            ("encrypt", "true"),
            ("data4", chunks[3]),
            ("iv", "MDEyMzQ1Njc4OWFiY2RlZg=="),
            ("data2", chunks[1]),
            ("seq", "7"),
        ] {
            let entry = format!("{key}={value}");
            txt.push(entry.len() as u8);
            txt.extend_from_slice(entry.as_bytes());
        }
        answer(b"\xc0\x2b", TYPE_TXT, &txt);
        msg
    }

    #[test]
    fn joins_split_txt_data_and_decrypts() {
        assert_eq!(LONG_DATA.len(), 704);
        let sender: IpAddr = Ipv4Addr::new(192, 168, 1, 50).into();
        let found = parse_response(&split_txt_response(), sender);
        assert_eq!(found.len(), 1);
        let strip = &found[0];
        assert_eq!(strip.deviceid, "1000aabbcc");
        assert_eq!((strip.ip.as_str(), strip.port), ("192.168.1.50", 8081)); // no A / SRV: fallbacks
        assert_eq!(strip.data.as_deref(), Some(LONG_DATA));
        let params = decrypt_params(KEY, strip).unwrap();
        assert_eq!(params, serde_json::from_str::<Value>(LONG_PARAMS).unwrap());
        assert!(decrypt_params("wrong key", strip).is_err());
    }

    #[test]
    fn plain_params_when_not_encrypted() {
        let found = Found {
            deviceid: "1000aabbcc".into(),
            ip: "192.168.1.50".into(),
            port: 8081,
            encrypted: false,
            iv: None,
            data: Some(r#"{"switch":"on"}"#.into()),
        };
        assert_eq!(
            decrypt_params("unused", &found).unwrap(),
            json!({"switch": "on"})
        );
    }

    // ---- real network: cargo test -- --ignored --nocapture ----

    const M5: &str = "1001f28fa2";

    /// The M5's devicekey from the smarthome-local.json of the data folder `TVL_DATA` names (never
    /// printed; the keys never go in the repo).
    fn m5_devicekey() -> String {
        let dir = std::env::var_os("TVL_DATA").expect("set TVL_DATA to a data folder with smarthome-local.json");
        let path = std::path::Path::new(&dir).join("smarthome-local.json");
        let store: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        store["ewelink"]
            .as_array()
            .and_then(|l| l.iter().find(|d| d["id"] == M5))
            .and_then(|d| d["key"].as_str())
            .expect("no devicekey for the M5")
            .to_string()
    }

    #[tokio::test]
    #[ignore = "needs the real M5 on the LAN"]
    async fn discover_finds_the_m5() {
        let started = std::time::Instant::now();
        let found = discover(Duration::from_secs(5)).await.unwrap();
        for f in &found {
            println!(
                "{} at {}:{}",
                f.deviceid, f.ip, f.port
            );
        }
        println!("took {:?}", started.elapsed());
        let m5 = found
            .iter()
            .find(|f| f.deviceid == M5)
            .expect("M5 not found");
        assert!(m5.ip.parse::<Ipv4Addr>().is_ok());
        assert_eq!(m5.port, 8081);
    }

    #[tokio::test]
    #[ignore = "needs the real M5 on the LAN"]
    async fn decrypts_the_m5_state() {
        let key = m5_devicekey();
        let found = discover(Duration::from_secs(5)).await.unwrap();
        let m5 = found
            .iter()
            .find(|f| f.deviceid == M5)
            .expect("M5 not found");
        let params = decrypt_params(&key, m5).unwrap();
        println!("{params}");
        assert!(params["switches"].as_array().is_some_and(|s| !s.is_empty()));
    }

    /// Exercises `send` end to end with `info`, a read-only command the M5 answers with
    /// error 400 (never a switching command here: that would toggle real lights).
    #[tokio::test]
    #[ignore = "needs the real M5 on the LAN"]
    async fn send_reports_the_m5_refusing_info() {
        let key = m5_devicekey();
        let found = discover(Duration::from_secs(3)).await.unwrap();
        let m5 = found
            .iter()
            .find(|f| f.deviceid == M5)
            .expect("M5 not found");
        let started = std::time::Instant::now();
        let result = send(&m5.ip, m5.port, M5, &key, "info", json!({})).await;
        println!("{result:?} after {:?}", started.elapsed());
        assert_eq!(
            result.unwrap_err().to_string(),
            "eWeLink info: device error 400"
        );
    }
}
