//! Finding the server on the home network: a UDP probe (`tvl_protocol::wire::PROBE`) sent to the
//! broadcast address and to this PC itself, answered by the server with its ports.

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;
use tvl_protocol::wire::PROBE;

/// A server that answered the probe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Found {
    /// Its address, as the answer came from.
    pub host: String,
    pub http: u16,
    #[serde(default)]
    pub https: Option<u16>,
    /// Its PC's name, for Settings.
    #[serde(default)]
    pub name: Option<String>,
}

/// The first server that answers on any of `ports` within `within`.
pub async fn probe(ports: &[u16], within: Duration) -> Option<Found> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await.ok()?;
    sock.set_broadcast(true).ok()?;
    for port in ports {
        let _ = sock.send_to(PROBE, (Ipv4Addr::BROADCAST, *port)).await;
        // A server on this same PC (a broadcast doesn't always come back to it).
        let _ = sock.send_to(PROBE, (Ipv4Addr::LOCALHOST, *port)).await;
    }
    let deadline = tokio::time::Instant::now() + within;
    let mut buf = [0u8; 512];
    loop {
        let (n, peer) = tokio::time::timeout_at(deadline, sock.recv_from(&mut buf)).await.ok()?.ok()?;
        if let Some(found) = read_answer(&buf[..n], peer.ip()) {
            return Some(found);
        }
    }
}

fn read_answer(bytes: &[u8], from: IpAddr) -> Option<Found> {
    #[derive(Deserialize)]
    struct Answer {
        tvl: u32,
        http: u16,
        #[serde(default)]
        https: Option<u16>,
        #[serde(default)]
        name: Option<String>,
    }
    let a: Answer = serde_json::from_slice(bytes).ok()?;
    (a.tvl > 0).then(|| Found { host: from.to_string(), http: a.http, https: a.https, name: a.name })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_name_the_server_by_where_they_came_from() {
        let from: IpAddr = "192.168.68.20".parse().unwrap();
        let found = read_answer(br#"{"tvl":1,"http":47960,"https":47961,"name":"TVL"}"#, from).unwrap();
        assert_eq!(found, Found { host: "192.168.68.20".into(), http: 47960, https: Some(47961), name: Some("TVL".into()) });
        assert!(read_answer(b"TVL?", from).is_none(), "another TV PC's probe isn't an answer");
    }
}
