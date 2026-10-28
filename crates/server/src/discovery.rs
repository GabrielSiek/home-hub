//! Finding the server and waking TV PCs, over UDP on the home network.
//!
//! A TV PC that doesn't know the server's address broadcasts "TVL?" to the probe port; the server
//! answers with its ports. (DNS-SD would do the same, but Windows' own responder holds UDP 5353.)

use std::net::SocketAddr;

use serde_json::json;
use tokio::net::UdpSocket;
use tvl_kit::{Error, Result};

use crate::ctx::Ctx;

pub use tvl_protocol::wire::PROBE;

/// Answers probes until the server stops, with the ports the server listens on.
pub fn start_probe(ctx: &Ctx, http: u16, https: Option<u16>) {
    let Some(port) = ctx.config.probe_port else { return };
    let ctx = ctx.clone();
    tvl_kit::task::spawn_supervised("probe", ctx.shutdown.clone(), move || {
        let ctx = ctx.clone();
        async move {
            let sock = match UdpSocket::bind(SocketAddr::new(ctx.config.bind, port)).await {
                Ok(s) => s,
                Err(e) => return tracing::error!("TV PCs can't find the server by themselves (UDP {port}): {e}"),
            };
            let reply = json!({
                "tvl": tvl_protocol::PROTOCOL_VERSION,
                "http": http,
                "https": https,
                "name": crate::tls::hostname(),
            })
            .to_string();
            let mut buf = [0u8; 64];
            loop {
                let Ok((n, peer)) = sock.recv_from(&mut buf).await else { continue };
                if &buf[..n] == PROBE && crate::http::security::is_private(peer.ip()) {
                    let _ = sock.send_to(reply.as_bytes(), peer).await;
                }
            }
        }
    });
}

/// "00:11:22:aa:bb:cc" or "00-11-22-AA-BB-CC".
fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let parts: Vec<u8> = s.split([':', '-']).map(|p| u8::from_str_radix(p, 16).ok()).collect::<Option<_>>()?;
    parts.try_into().ok()
}

/// The Wake-on-LAN magic packet: six 0xFF, then the address sixteen times.
fn magic_packet(mac: [u8; 6]) -> Vec<u8> {
    let mut p = vec![0xFF; 6];
    for _ in 0..16 {
        p.extend_from_slice(&mac);
    }
    p
}

/// Wakes the PC with network address `mac`. Works only on a wired PC with Wake-on-LAN on; a laptop
/// on Wi-Fi doesn't wake.
pub async fn wake(mac: &str) -> Result<()> {
    let mac = parse_mac(mac).ok_or_else(|| Error::msg("Not a network address"))?;
    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    sock.set_broadcast(true)?;
    sock.send_to(&magic_packet(mac), "255.255.255.255:9").await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_packets_repeat_the_address() {
        let mac = parse_mac("00-11-22-AA-bb:cc").unwrap();
        assert_eq!(mac, [0x00, 0x11, 0x22, 0xaa, 0xbb, 0xcc]);
        let p = magic_packet(mac);
        assert_eq!(p.len(), 102);
        assert_eq!(&p[..6], &[0xFF; 6]);
        assert_eq!(&p[96..], &mac);
        assert!(parse_mac("00:11:22").is_none());
        assert!(parse_mac("zz:11:22:33:44:55").is_none());
    }
}
