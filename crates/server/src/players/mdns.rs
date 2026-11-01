//! Finding things on the home network by DNS-SD: one question to the mDNS group, asked from a
//! passing port so the answers come straight back (RFC 6762 §6.7). Nothing stays listening.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::net::UdpSocket;
use tvl_kit::Result;

const GROUP: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(224, 0, 0, 251)), 5353);
const QUERY_PTR: u16 = 12;
const CLASS_IN: u16 = 1;
const TYPE_TXT: u16 = 16;
const TYPE_SRV: u16 = 33;

/// One service that answered.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub ip: IpAddr,
    pub port: Option<u16>,
    pub txt: HashMap<String, String>,
}

fn question(service: &str) -> Vec<u8> {
    let mut q = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in service.split('.') {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&QUERY_PTR.to_be_bytes());
    q.extend_from_slice(&CLASS_IN.to_be_bytes());
    q
}

/// Where the name at `at` ends: labels up to a zero, or a pointer to the rest.
fn skip_name(m: &[u8], mut at: usize) -> Option<usize> {
    loop {
        let len = *m.get(at)?;
        if len == 0 {
            return Some(at + 1);
        }
        if len & 0xc0 == 0xc0 {
            return Some(at + 2);
        }
        at += 1 + len as usize;
    }
}

fn be16(m: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*m.get(at)?, *m.get(at + 1)?]))
}

fn parse_txt(data: &[u8]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut at = 0;
    while let Some(&len) = data.get(at) {
        let Some(entry) = data.get(at + 1..at + 1 + len as usize) else { break };
        if let Some((k, v)) = String::from_utf8_lossy(entry).split_once('=') {
            out.insert(k.to_string(), v.to_string());
        }
        at += 1 + len as usize;
    }
    out
}

/// The SRV port and TXT entries of one reply. The names aren't read: each reply is one device's.
fn parse(m: &[u8]) -> Option<(Option<u16>, HashMap<String, String>)> {
    let questions = be16(m, 4)?;
    let records = u32::from(be16(m, 6)?) + u32::from(be16(m, 8)?) + u32::from(be16(m, 10)?);
    let mut at = 12;
    for _ in 0..questions {
        at = skip_name(m, at)? + 4;
    }
    let (mut port, mut txt) = (None, HashMap::new());
    for _ in 0..records {
        at = skip_name(m, at)?;
        let kind = be16(m, at)?;
        let len = be16(m, at + 8)? as usize;
        let data = m.get(at + 10..at + 10 + len)?;
        match kind {
            TYPE_TXT => txt.extend(parse_txt(data)),
            TYPE_SRV => port = be16(data, 4),
            _ => {}
        }
        at += 10 + len;
    }
    Some((port, txt))
}

/// Everything offering `service` (e.g. `_googlecast._tcp.local`) that answers within `wait`.
pub async fn browse(service: &str, wait: Duration) -> Result<Vec<Found>> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
    sock.send_to(&question(service), GROUP).await?;
    let deadline = tokio::time::Instant::now() + wait;
    let mut found: Vec<Found> = Vec::new();
    let mut buf = [0u8; 9000];
    while let Ok(Ok((n, peer))) = tokio::time::timeout_at(deadline, sock.recv_from(&mut buf)).await {
        let Some((port, txt)) = parse(&buf[..n]) else { continue };
        if !found.iter().any(|f| f.ip == peer.ip()) {
            found.push(Found { ip: peer.ip(), port, txt });
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_is_one_ptr() {
        let q = question("_x._tcp.local");
        assert_eq!(&q[4..6], [0, 1]);
        assert_eq!(&q[12..16], [2, b'_', b'x', 4]);
        assert_eq!(&q[q.len() - 4..], [0, 12, 0, 1]);
    }

    #[test]
    fn reads_port_and_txt_past_the_echoed_question() {
        let q = question("_x._tcp.local");
        let mut m = q.clone();
        m[6..8].copy_from_slice(&[0, 2]);
        // TXT, its name a pointer to the question's.
        let txt = b"\x05fn=TV\x04id=7";
        m.extend_from_slice(&[0xc0, 12, 0, 16, 0, 1, 0, 0, 0, 120, 0, txt.len() as u8]);
        m.extend_from_slice(txt);
        // SRV: priority, weight, port 8009, target.
        let srv = [0, 0, 0, 0, 0x1f, 0x49, 0];
        m.extend_from_slice(&[0xc0, 12, 0, 33, 0, 1, 0, 0, 0, 120, 0, srv.len() as u8]);
        m.extend_from_slice(&srv);
        let (port, txt) = parse(&m).unwrap();
        assert_eq!(port, Some(8009));
        assert_eq!(txt["fn"], "TV");
        assert_eq!(txt["id"], "7");
    }

    #[test]
    fn a_cut_reply_is_ignored() {
        let q = question("_x._tcp.local");
        let mut m = q.clone();
        m[6..8].copy_from_slice(&[0, 1]);
        m.extend_from_slice(&[0xc0, 12, 0, 16]);
        assert!(parse(&m).is_none());
    }
}
