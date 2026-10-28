//! Opening `/api/agent`: over `wss://` once paired, trusting only the home certificate authority
//! the server handed over then (never the PC's own list), else over `ws://`.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

pub type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

const OPEN_WITHIN: Duration = Duration::from_secs(10);

enum Failed {
    /// Nothing listens on that port.
    Refused,
    Other(String),
}

/// Opens the socket to the server at `host`. `secure` is the HTTPS port and the authority to
/// trust; the bool says whether it went over `wss://`.
pub async fn open(host: &str, http: u16, secure: Option<(u16, &str)>, token: Option<&str>) -> Result<(Socket, bool), String> {
    let ip = resolve(host, http).await?;
    if let Some((port, ca)) = secure {
        match open_at(SocketAddr::new(ip, port), Some(ca), token).await {
            Ok(ws) => return Ok((ws, true)),
            // The server stopped taking HTTPS: plain, and its welcome then says so. A certificate
            // the authority didn't sign is an error, never a reason to go plain.
            Err(Failed::Refused) => tracing::warn!("the server's HTTPS port is closed, connecting without it"),
            Err(Failed::Other(e)) => return Err(e),
        }
    }
    match open_at(SocketAddr::new(ip, http), None, token).await {
        Ok(ws) => Ok((ws, false)),
        Err(Failed::Refused) => Err("Nothing answers at that address".into()),
        Err(Failed::Other(e)) => Err(e),
    }
}

/// The address to connect to: by IP, so the certificate (made for the IP the server was reached
/// at) matches whatever name was typed. IPv4 first.
async fn resolve(host: &str, port: u16) -> Result<IpAddr, String> {
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port)).await.map_err(|_| format!("Can't find {host} on the network"))?.collect();
    addrs.iter().find(|a| a.is_ipv4()).or(addrs.first()).map(|a| a.ip()).ok_or_else(|| format!("Can't find {host} on the network"))
}

async fn open_at(addr: SocketAddr, ca: Option<&str>, token: Option<&str>) -> Result<Socket, Failed> {
    let scheme = if ca.is_some() { "wss" } else { "ws" };
    let mut req = format!("{scheme}://{addr}/api/agent").into_client_request().map_err(|e| Failed::Other(e.to_string()))?;
    if let Some(t) = token {
        let value = format!("Bearer {t}").parse().map_err(|_| Failed::Other("A damaged token: pair again".into()))?;
        req.headers_mut().insert(AUTHORIZATION, value);
    }
    let connector = match ca {
        Some(pem) => Some(Connector::Rustls(trusting(pem).map_err(Failed::Other)?)),
        None => None,
    };
    match tokio::time::timeout(OPEN_WITHIN, tokio_tungstenite::connect_async_tls_with_config(req, None, false, connector)).await {
        Err(_) => Err(Failed::Other("The server didn't answer in time".into())),
        Ok(Ok((ws, _))) => Ok(ws),
        Ok(Err(tokio_tungstenite::tungstenite::Error::Io(e))) if e.kind() == std::io::ErrorKind::ConnectionRefused => Err(Failed::Refused),
        Ok(Err(e)) => Err(Failed::Other(e.to_string())),
    }
}

/// TLS that trusts only the certificates in `pem`.
fn trusting(pem: &str) -> Result<Arc<rustls::ClientConfig>, String> {
    let damaged = |e: &dyn std::fmt::Debug| format!("The home certificate authority is damaged ({e:?}): pair again");
    let mut roots = rustls::RootCertStore::empty();
    for cert in der_certificates(pem)? {
        roots.add(cert).map_err(|e| damaged(&e))?;
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| damaged(&e))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

fn der_certificates(pem: &str) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, String> {
    use rustls::pki_types::pem::PemObject;
    let certs: Vec<_> = rustls::pki_types::CertificateDer::pem_slice_iter(pem.as_bytes())
        .collect::<Result<_, _>>()
        .map_err(|e| format!("The home certificate authority is damaged ({e:?}): pair again"))?;
    if certs.is_empty() {
        return Err("No home certificate authority: pair again".into());
    }
    Ok(certs)
}

/// SHA-256 of the authority's certificate, "3F:9A:…", as the server shows it.
pub fn fingerprint(pem: &str) -> Option<String> {
    let cert = der_certificates(pem).ok()?.into_iter().next()?;
    Some(Sha256::digest(cert.as_ref()).iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":"))
}
