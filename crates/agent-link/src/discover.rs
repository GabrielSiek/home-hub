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
    let home = home_broadcast().await;
    for port in ports {
        let _ = sock.send_to(PROBE, (Ipv4Addr::BROADCAST, *port)).await;
        // Windows sends 255.255.255.255 out of one adapter only, the lowest metric, which on a PC
        // with WSL, Hyper-V or a VPN isn't the home network's.
        if let Some(b) = home {
            let _ = sock.send_to(PROBE, (b, *port)).await;
        }
        // A server on this same PC (a broadcast doesn't always come back to it).
        let _ = sock.send_to(PROBE, (Ipv4Addr::LOCALHOST, *port)).await;
    }
    let deadline = tokio::time::Instant::now() + within;
    let mut buf = [0u8; 512];
    loop {
        // Windows reports the probe to this PC, where no server listens, as a reset on the next
        // read: that's no answer, the others may still come.
        let Ok((n, peer)) = tokio::time::timeout_at(deadline, sock.recv_from(&mut buf)).await.ok()? else { continue };
        if let Some(found) = read_answer(&buf[..n], peer.ip()) {
            return Some(found);
        }
    }
}

/// The broadcast address of the network the default route leaves by, taken as a /24 (home
/// routers' usual size). Connecting a UDP socket sends nothing; it only picks the local address.
async fn home_broadcast() -> Option<Ipv4Addr> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await.ok()?;
    sock.connect((Ipv4Addr::new(1, 1, 1, 1), 53)).await.ok()?;
    match sock.local_addr().ok()?.ip() {
        IpAddr::V4(ip) => subnet_broadcast(ip),
        IpAddr::V6(_) => None,
    }
}

fn subnet_broadcast(ip: Ipv4Addr) -> Option<Ipv4Addr> {
    let [a, b, c, _] = ip.octets();
    ip.is_private().then_some(Ipv4Addr::new(a, b, c, 255))
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

    #[test]
    fn the_home_network_is_probed_by_its_own_broadcast() {
        assert_eq!(subnet_broadcast(Ipv4Addr::new(192, 168, 68, 106)), Some(Ipv4Addr::new(192, 168, 68, 255)));
        assert_eq!(subnet_broadcast(Ipv4Addr::new(54, 232, 189, 113)), None, "not a home address");
    }

    /// Finds the house's real server: `cargo test -p tvl-agent-link real_probe -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn real_probe() {
        println!("home broadcast: {:?}", home_broadcast().await);
        let found = probe(&[47962], Duration::from_secs(3)).await;
        println!("found: {found:?}");
        assert!(found.is_some());
    }
}
