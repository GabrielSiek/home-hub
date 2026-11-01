//! The Android TV remote protocol: what Google's own TV remote app speaks, and the way to open an
//! app or a title on a Google TV. Cast can't: it opens receivers, not the TV's installed apps.
//!
//! Two TLS ports, both with a client certificate of ours that the TV learns once. On 6467 the TV
//! shows a code and proves with it that both ends hold the keys they see; on 6466, from then on, it
//! takes keys and links. Both carry protobuf messages, each after its length as a varint.
//! Field numbers are those of Google's polo.proto and remotemessage.proto.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use rsa::pkcs8::{EncodePrivateKey, LineEnding};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tvl_kit::{Error, Result};
use x509_parser::prelude::{FromDer, X509Certificate};
use x509_parser::public_key::PublicKey;

use super::cast::{put_varint, tls};

const PAIRING_PORT: u16 = 6467;
const REMOTE_PORT: u16 = 6466;
const CLIENT_NAME: &str = "Home Hub";
const SERVICE_NAME: &str = "atvremote";
const LARGEST_FRAME: usize = 1 << 16;
const ANSWER_WAIT: Duration = Duration::from_secs(10);
/// A TV just woken takes this long to take a link.
const WAKE_WAIT: Duration = Duration::from_secs(2);
/// Long enough for a refusal to come back; a link taken says nothing.
const REFUSAL_WAIT: Duration = Duration::from_millis(600);

const STATUS_OK: u64 = 200;
const STATUS_BAD_SECRET: u64 = 402;
const PROTOCOL_VERSION: u64 = 2;
const ENCODING_HEXADECIMAL: u64 = 3;
const CODE_LENGTH: u64 = 6;
const ROLE_INPUT: u64 = 1;

// polo.proto: OuterMessage.
const PAIRING_REQUEST: u32 = 10;
const PAIRING_REQUEST_ACK: u32 = 11;
const OPTIONS: u32 = 20;
const CONFIGURATION: u32 = 30;
const CONFIGURATION_ACK: u32 = 31;
const SECRET: u32 = 40;
const SECRET_ACK: u32 = 41;

// remotemessage.proto: RemoteMessage.
const CONFIGURE: u32 = 1;
const SET_ACTIVE: u32 = 2;
const ERROR: u32 = 3;
const PING_REQUEST: u32 = 8;
const PING_RESPONSE: u32 = 9;
const KEY_INJECT: u32 = 10;
const START: u32 = 40;
const APP_LINK: u32 = 90;

/// Ping, keys, power, volume and app links.
const FEATURES: u64 = 1 | 2 | 32 | 64 | 512;
const KEYCODE_WAKEUP: u64 = 224;
const DIRECTION_SHORT: u64 = 3;

// ---------- Protobuf ----------

fn put_int(out: &mut Vec<u8>, field: u32, n: u64) {
    put_varint(out, u64::from(field) << 3);
    put_varint(out, n);
}

fn put_bytes(out: &mut Vec<u8>, field: u32, data: &[u8]) {
    put_varint(out, u64::from(field) << 3 | 2);
    put_varint(out, data.len() as u64);
    out.extend_from_slice(data);
}

#[derive(Debug, PartialEq)]
enum Field<'a> {
    Int(u64),
    Bytes(&'a [u8]),
}

fn take_varint(m: &[u8], at: &mut usize) -> Option<u64> {
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

fn fields(m: &[u8]) -> Option<Vec<(u32, Field<'_>)>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < m.len() {
        let tag = take_varint(m, &mut at)?;
        let number = (tag >> 3) as u32;
        match tag & 7 {
            0 => out.push((number, Field::Int(take_varint(m, &mut at)?))),
            2 => {
                let len = take_varint(m, &mut at)? as usize;
                let data = m.get(at..at.checked_add(len)?)?;
                at += len;
                out.push((number, Field::Bytes(data)));
            }
            _ => return None,
        }
    }
    Some(out)
}

fn int_of(m: &[u8], field: u32) -> Option<u64> {
    fields(m)?.into_iter().find_map(|(n, v)| match v {
        Field::Int(i) if n == field => Some(i),
        _ => None,
    })
}

async fn send<S: AsyncWrite + Unpin>(stream: &mut S, message: &[u8]) -> Result<()> {
    let mut frame = Vec::with_capacity(message.len() + 2);
    put_varint(&mut frame, message.len() as u64);
    frame.extend_from_slice(message);
    stream.write_all(&frame).await?;
    Ok(stream.flush().await?)
}

async fn receive<S: AsyncRead + Unpin>(stream: &mut S) -> Result<Vec<u8>> {
    let (mut len, mut shift) = (0usize, 0);
    loop {
        let b = stream.read_u8().await?;
        len |= usize::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 21 {
            return Err(Error::msg("The TV sent something unreadable"));
        }
    }
    if len > LARGEST_FRAME {
        return Err(Error::msg("The TV sent something too large"));
    }
    let mut message = vec![0u8; len];
    stream.read_exact(&mut message).await?;
    Ok(message)
}

// ---------- Who we are to the TV ----------

/// The certificate the TV learns at pairing. It has to be RSA: the code is worked out from the key.
#[derive(Clone, Serialize, Deserialize)]
pub struct Identity {
    certificate: String,
    key: String,
}

impl Identity {
    /// Slow: making the key takes seconds, so it's made once, on the blocking pool.
    pub async fn create() -> Result<Self> {
        tokio::task::spawn_blocking(|| {
            let private = rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).map_err(|e| Error::msg(format!("Making a key: {e}")))?;
            let key = private.to_pkcs8_pem(LineEnding::LF).map_err(|e| Error::msg(format!("Keeping a key: {e}")))?.to_string();
            let pair = rcgen::KeyPair::from_pem_and_sign_algo(&key, &rcgen::PKCS_RSA_SHA256).map_err(|e| Error::msg(format!("Reading a key: {e}")))?;
            let mut params = rcgen::CertificateParams::new(vec!["home-hub".to_string()]).map_err(|e| Error::msg(e.to_string()))?;
            params.distinguished_name.push(rcgen::DnType::CommonName, "home-hub");
            params.not_after = time::OffsetDateTime::now_utc() + time::Duration::days(20 * 365);
            let certificate = params.self_signed(&pair).map_err(|e| Error::msg(format!("Making a certificate: {e}")))?.pem();
            Ok(Identity { certificate, key })
        })
        .await
        .map_err(|e| Error::msg(e.to_string()))?
    }

    fn der(&self) -> Result<CertificateDer<'static>> {
        CertificateDer::from_pem_slice(self.certificate.as_bytes()).map_err(|e| Error::msg(format!("Reading our certificate: {e}")))
    }

    fn private_key(&self) -> Result<PrivateKeyDer<'static>> {
        PrivateKeyDer::from_pem_slice(self.key.as_bytes()).map_err(|e| Error::msg(format!("Reading our key: {e}")))
    }

    async fn connect(&self, ip: IpAddr, port: u16) -> Result<TlsStream<TcpStream>> {
        tls(SocketAddr::new(ip, port), Some((vec![self.der()?], self.private_key()?))).await
    }
}

/// The modulus and exponent of a certificate's RSA key, without leading zeros.
fn rsa_numbers(der: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let not_rsa = || Error::msg("The TV's certificate isn't one this can pair with");
    let (_, certificate) = X509Certificate::from_der(der).map_err(|_| not_rsa())?;
    let public = certificate.public_key().parsed().map_err(|_| not_rsa())?;
    let PublicKey::RSA(key) = public else { return Err(not_rsa()) };
    let trim = |b: &[u8]| b.iter().copied().skip_while(|&x| x == 0).collect();
    Ok((trim(key.modulus), trim(key.exponent)))
}

// ---------- Pairing ----------

fn outer(field: u32, inner: &[u8]) -> Vec<u8> {
    let mut m = Vec::new();
    put_int(&mut m, 1, PROTOCOL_VERSION);
    put_int(&mut m, 2, STATUS_OK);
    put_bytes(&mut m, field, inner);
    m
}

fn encoding() -> Vec<u8> {
    let mut e = Vec::new();
    put_int(&mut e, 1, ENCODING_HEXADECIMAL);
    put_int(&mut e, 2, CODE_LENGTH);
    e
}

fn pairing_request() -> Vec<u8> {
    let mut r = Vec::new();
    put_bytes(&mut r, 1, SERVICE_NAME.as_bytes());
    put_bytes(&mut r, 2, CLIENT_NAME.as_bytes());
    outer(PAIRING_REQUEST, &r)
}

fn options() -> Vec<u8> {
    let mut o = Vec::new();
    put_bytes(&mut o, 1, &encoding());
    put_int(&mut o, 3, ROLE_INPUT);
    outer(OPTIONS, &o)
}

fn configuration() -> Vec<u8> {
    let mut c = Vec::new();
    put_bytes(&mut c, 1, &encoding());
    put_int(&mut c, 2, ROLE_INPUT);
    outer(CONFIGURATION, &c)
}

/// The next pairing message, which has to carry `field`.
async fn expect<S: AsyncRead + Unpin>(stream: &mut S, field: u32) -> Result<()> {
    let m = tokio::time::timeout(ANSWER_WAIT, receive(stream)).await.map_err(|_| Error::msg("The TV didn't answer in time"))??;
    let list = fields(&m).ok_or_else(|| Error::msg("The TV sent something unreadable"))?;
    match int_of(&m, 2) {
        Some(STATUS_OK) => {}
        Some(STATUS_BAD_SECRET) => return Err(Error::msg("That isn't the code on the TV")),
        Some(status) => return Err(Error::msg(format!("The TV turned the pairing down ({status})"))),
        None => return Err(Error::msg("The TV sent something unreadable")),
    }
    if list.iter().any(|(n, _)| *n == field) {
        Ok(())
    } else {
        Err(Error::msg("The TV answered out of turn: start the pairing again"))
    }
}

/// Up to the point where the TV shows its code.
async fn ask_for_code<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S) -> Result<()> {
    send(stream, &pairing_request()).await?;
    expect(stream, PAIRING_REQUEST_ACK).await?;
    send(stream, &options()).await?;
    expect(stream, OPTIONS).await?;
    send(stream, &configuration()).await?;
    expect(stream, CONFIGURATION_ACK).await
}

/// What proves the code: a hash of both keys and the code's last four digits, whose first byte is
/// the code's first two. So a mistyped code is caught here, before the TV counts a failed try.
fn secret(client: &(Vec<u8>, Vec<u8>), server: &(Vec<u8>, Vec<u8>), code: &str) -> Result<Vec<u8>> {
    let bad = || Error::msg("The code on the TV has six digits and letters, 0-9 and A-F");
    let code = code.trim();
    if code.len() != 6 || !code.is_ascii() {
        return Err(bad());
    }
    let byte = |at: usize| u8::from_str_radix(&code[at..at + 2], 16).map_err(|_| bad());
    let (check, nonce) = (byte(0)?, [byte(2)?, byte(4)?]);
    let mut h = Sha256::new();
    h.update(&client.0);
    h.update(&client.1);
    h.update(&server.0);
    h.update(&server.1);
    h.update(nonce);
    let hash = h.finalize().to_vec();
    if hash[0] != check {
        return Err(Error::msg("That isn't the code on the TV"));
    }
    Ok(hash)
}

async fn send_secret<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, secret: &[u8]) -> Result<()> {
    let mut s = Vec::new();
    put_bytes(&mut s, 1, secret);
    send(stream, &outer(SECRET, &s)).await?;
    expect(stream, SECRET_ACK).await
}

/// A pairing waiting for the code the TV is showing.
pub struct Pairing {
    stream: TlsStream<TcpStream>,
    identity: Identity,
}

/// Makes the TV show its code.
pub async fn start_pairing(identity: Identity, ip: IpAddr) -> Result<Pairing> {
    let mut stream = identity.connect(ip, PAIRING_PORT).await?;
    ask_for_code(&mut stream).await?;
    Ok(Pairing { stream, identity })
}

impl Pairing {
    pub async fn finish(mut self, code: &str) -> Result<()> {
        let theirs = self.stream.get_ref().1.peer_certificates().and_then(|c| c.first()).ok_or_else(|| Error::msg("The TV showed no certificate"))?;
        let theirs = rsa_numbers(theirs)?;
        let ours = rsa_numbers(&self.identity.der()?)?;
        let secret = secret(&ours, &theirs, code)?;
        send_secret(&mut self.stream, &secret).await
    }
}

// ---------- The remote ----------

fn configure_answer(theirs: u64) -> Vec<u8> {
    let mut info = Vec::new();
    put_int(&mut info, 3, 1);
    put_bytes(&mut info, 4, b"1");
    put_bytes(&mut info, 5, SERVICE_NAME.as_bytes());
    put_bytes(&mut info, 6, b"1.0.0");
    let mut c = Vec::new();
    put_int(&mut c, 1, theirs & FEATURES);
    put_bytes(&mut c, 2, &info);
    let mut m = Vec::new();
    put_bytes(&mut m, CONFIGURE, &c);
    m
}

fn single(field: u32, inner_field: u32, n: u64) -> Vec<u8> {
    let mut inner = Vec::new();
    put_int(&mut inner, inner_field, n);
    let mut m = Vec::new();
    put_bytes(&mut m, field, &inner);
    m
}

fn key(code: u64) -> Vec<u8> {
    let mut k = Vec::new();
    put_int(&mut k, 1, code);
    put_int(&mut k, 2, DIRECTION_SHORT);
    let mut m = Vec::new();
    put_bytes(&mut m, KEY_INJECT, &k);
    m
}

fn app_link(link: &str) -> Vec<u8> {
    let mut l = Vec::new();
    put_bytes(&mut l, 1, link.as_bytes());
    let mut m = Vec::new();
    put_bytes(&mut m, APP_LINK, &l);
    m
}

/// Answers one message of the session's opening. `Some` once the TV says it's ready: whether its
/// screen is on.
async fn opening<S: AsyncWrite + Unpin>(stream: &mut S, features: &mut u64, m: &[u8]) -> Result<Option<bool>> {
    for (field, value) in fields(m).ok_or_else(|| Error::msg("The TV sent something unreadable"))? {
        let Field::Bytes(inner) = value else { continue };
        match field {
            CONFIGURE => {
                *features = int_of(inner, 1).unwrap_or(FEATURES) & FEATURES;
                send(stream, &configure_answer(*features)).await?;
            }
            SET_ACTIVE => send(stream, &single(SET_ACTIVE, 1, *features)).await?,
            PING_REQUEST => send(stream, &single(PING_RESPONSE, 1, int_of(inner, 1).unwrap_or(0))).await?,
            START => return Ok(Some(int_of(inner, 1).unwrap_or(0) != 0)),
            ERROR => return Err(Error::msg("The TV refused that")),
            _ => {}
        }
    }
    Ok(None)
}

/// Opens `link` on the TV at the end of `stream`, waking it first.
async fn open_on<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, link: &str) -> Result<()> {
    let mut features = FEATURES;
    let ready = async {
        loop {
            let m = receive(stream).await?;
            if let Some(on) = opening(stream, &mut features, &m).await? {
                return Ok::<bool, Error>(on);
            }
        }
    };
    let on = tokio::time::timeout(ANSWER_WAIT, ready).await.map_err(|_| Error::msg("The TV didn't answer in time"))??;
    if !on {
        send(stream, &key(KEYCODE_WAKEUP)).await?;
        tokio::time::sleep(WAKE_WAIT).await;
    }
    send(stream, &app_link(link)).await?;
    let mut ignored = 0u64;
    let refused = async {
        loop {
            let m = receive(stream).await?;
            opening(stream, &mut ignored, &m).await?;
        }
    };
    match tokio::time::timeout(REFUSAL_WAIT, refused).await {
        Ok(Err::<(), Error>(e)) if e.to_string() == "The TV refused that" => Err(e),
        _ => Ok(()),
    }
}

/// Opens `link`: an app, or a title in one, on a TV paired before.
pub async fn open(identity: &Identity, ip: IpAddr, link: &str) -> Result<()> {
    let mut stream = identity.connect(ip, REMOTE_PORT).await?;
    open_on(&mut stream, link).await.map_err(|e| match e {
        Error::Io(_) => Error::msg("The TV hung up: pair it again"),
        e => e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    fn tv_outer(field: u32, status: u64) -> Vec<u8> {
        let mut m = Vec::new();
        put_int(&mut m, 1, PROTOCOL_VERSION);
        put_int(&mut m, 2, status);
        put_bytes(&mut m, field, &[]);
        m
    }

    fn has(m: &[u8], field: u32) -> bool {
        fields(m).unwrap().iter().any(|(n, _)| *n == field)
    }

    fn nested(m: &[u8], field: u32) -> &[u8] {
        let Some((_, Field::Bytes(b))) = fields(m).unwrap().into_iter().find(|(n, _)| *n == field) else { panic!("no field {field}") };
        b
    }

    #[test]
    fn varints_and_nested_messages_read_back() {
        let m = outer(SECRET, &[1, 2, 3]);
        assert_eq!(int_of(&m, 1), Some(2));
        assert_eq!(int_of(&m, 2), Some(200), "200 takes two bytes");
        assert_eq!(nested(&m, SECRET), [1, 2, 3]);
    }

    #[test]
    fn pairing_messages_match_polo_proto() {
        assert_eq!(hex(&options()), "080210c80 1a2 0108 0a0408 0310 06 1801".replace(' ', ""));
        // Field 30, then encoding {3, 6} and role 1.
        assert_eq!(hex(&configuration()), "080210c801f20108 0a0408031006 1001".replace(' ', ""));
        let r = pairing_request();
        let inner = nested(&r, PAIRING_REQUEST);
        assert_eq!(nested(inner, 1), b"atvremote");
        assert_eq!(nested(inner, 2), b"Home Hub");
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn the_secret_is_the_hash_the_code_checks() {
        // Worked out separately, as the Python reference does: sha256(n1 e1 n2 e2 nonce).
        let client = (vec![0xaa, 0xbb], vec![1, 0, 1]);
        let server = (vec![0xcc, 0xdd], vec![1, 0, 1]);
        let mut h = Sha256::new();
        h.update([0xaa, 0xbb, 1, 0, 1, 0xcc, 0xdd, 1, 0, 1, 0x12, 0x34]);
        let hash = h.finalize().to_vec();
        let code = format!("{:02X}1234", hash[0]);
        assert_eq!(secret(&client, &server, &code).unwrap(), hash);
        assert_eq!(secret(&client, &server, &code.to_lowercase()).unwrap(), hash);
        let wrong = format!("{:02X}1234", hash[0] ^ 1);
        assert!(secret(&client, &server, &wrong).unwrap_err().to_string().contains("isn't the code"));
        assert!(secret(&client, &server, "12").is_err());
        assert!(secret(&client, &server, "GGGGGG").is_err());
    }

    #[tokio::test]
    async fn pairing_goes_step_by_step_until_the_tv_shows_its_code() {
        let (mut ours, mut tv) = duplex(4096);
        let fake = tokio::spawn(async move {
            for (asked, answer) in [(PAIRING_REQUEST, PAIRING_REQUEST_ACK), (OPTIONS, OPTIONS), (CONFIGURATION, CONFIGURATION_ACK)] {
                assert!(has(&receive(&mut tv).await.unwrap(), asked));
                send(&mut tv, &tv_outer(answer, STATUS_OK)).await.unwrap();
            }
            assert!(has(&receive(&mut tv).await.unwrap(), SECRET));
            send(&mut tv, &tv_outer(SECRET_ACK, STATUS_OK)).await.unwrap();
        });
        ask_for_code(&mut ours).await.unwrap();
        send_secret(&mut ours, &[0; 32]).await.unwrap();
        fake.await.unwrap();
    }

    #[tokio::test]
    async fn a_refused_code_says_so() {
        let (mut ours, mut tv) = duplex(4096);
        tokio::spawn(async move {
            receive(&mut tv).await.unwrap();
            send(&mut tv, &tv_outer(SECRET_ACK, STATUS_BAD_SECRET)).await.unwrap();
        });
        let e = send_secret(&mut ours, &[0; 32]).await.unwrap_err();
        assert!(e.to_string().contains("isn't the code"), "{e}");
    }

    /// The TV's side of a session: configure, set active, a ping, start; then what it got.
    async fn fake_tv(mut tv: tokio::io::DuplexStream, started: bool) -> Vec<Vec<u8>> {
        let mut c = Vec::new();
        put_int(&mut c, 1, 0xffff);
        let mut m = Vec::new();
        put_bytes(&mut m, CONFIGURE, &c);
        send(&mut tv, &m).await.unwrap();
        let answer = receive(&mut tv).await.unwrap();
        assert_eq!(int_of(nested(&answer, CONFIGURE), 1), Some(FEATURES), "only what we use");
        send(&mut tv, &single(SET_ACTIVE, 1, 0)).await.unwrap();
        assert_eq!(int_of(nested(&receive(&mut tv).await.unwrap(), SET_ACTIVE), 1), Some(FEATURES));
        send(&mut tv, &single(PING_REQUEST, 1, 7)).await.unwrap();
        assert_eq!(int_of(nested(&receive(&mut tv).await.unwrap(), PING_RESPONSE), 1), Some(7));
        send(&mut tv, &single(START, 1, u64::from(started))).await.unwrap();
        let mut got = Vec::new();
        while let Ok(m) = receive(&mut tv).await {
            got.push(m);
        }
        got
    }

    #[tokio::test]
    async fn a_link_follows_the_opening() {
        let (mut ours, tv) = duplex(4096);
        let fake = tokio::spawn(fake_tv(tv, true));
        open_on(&mut ours, "https://www.netflix.com/title/1").await.unwrap();
        drop(ours);
        let got = fake.await.unwrap();
        assert_eq!(got.len(), 1, "no key: the screen is on");
        assert_eq!(nested(nested(&got[0], APP_LINK), 1), b"https://www.netflix.com/title/1");
    }

    #[tokio::test]
    async fn a_sleeping_tv_is_woken_first() {
        let (mut ours, tv) = duplex(4096);
        let fake = tokio::spawn(fake_tv(tv, false));
        open_on(&mut ours, "market://launch?id=a.b").await.unwrap();
        drop(ours);
        let got = fake.await.unwrap();
        assert_eq!(int_of(nested(&got[0], KEY_INJECT), 1), Some(KEYCODE_WAKEUP));
        assert_eq!(int_of(nested(&got[0], KEY_INJECT), 2), Some(DIRECTION_SHORT));
        assert!(has(&got[1], APP_LINK));
    }

    #[tokio::test]
    async fn a_refused_link_says_so() {
        let (mut ours, mut tv) = duplex(4096);
        tokio::spawn(async move {
            send(&mut tv, &single(START, 1, 1)).await.unwrap();
            receive(&mut tv).await.unwrap();
            send(&mut tv, &single(ERROR, 1, 1)).await.unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
        });
        assert!(open_on(&mut ours, "x").await.is_err());
    }

    #[tokio::test]
    #[ignore = "makes an RSA key, slow without optimization"]
    async fn our_identity_makes_a_certificate_tls_accepts() {
        let id = Identity::create().await.unwrap();
        let (n, e) = rsa_numbers(&id.der().unwrap()).unwrap();
        assert_eq!((n.len(), e.as_slice()), (256, &[1u8, 0, 1][..]));
        id.private_key().unwrap();
    }

    /// `TV=192.168.68.120 cargo test -p tvl-server --release tv_certificate -- --ignored --nocapture`:
    /// a TLS handshake with a real TV's pairing port, and reading its key. It sends nothing, so
    /// no code comes up on the screen.
    #[tokio::test]
    #[ignore = "needs a real TV"]
    async fn tv_certificate() {
        let ip: IpAddr = std::env::var("TV").unwrap().parse().unwrap();
        let id = Identity::create().await.unwrap();
        let stream = id.connect(ip, PAIRING_PORT).await.unwrap();
        let cert = stream.get_ref().1.peer_certificates().unwrap().first().unwrap().clone();
        let (n, e) = rsa_numbers(&cert).unwrap();
        println!("modulus {} bytes, exponent {e:?}, {:?}", n.len(), stream.get_ref().1.protocol_version());
    }
}
