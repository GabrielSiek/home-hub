//! Who may talk to the server, checked before any handler runs.
//!
//! The old launcher let in any device at home, which stays, but it trusted any web page such a
//! device had open too: a page elsewhere could POST to it (it parsed any body as JSON), listen on
//! its event socket, or rebind its own domain name to the launcher's address and read everything.
//! Now a request must name the server by one of its own names (Host), come from one of its own
//! pages if it comes from a page at all (Origin), and say it's JSON (Content-Type), which a page
//! elsewhere can't send without the browser asking first.

use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Only devices at home (and on a private tunnel such as Tailscale) get in.
pub async fn home_network_only(ConnectInfo(addr): ConnectInfo<SocketAddr>, req: Request, next: Next) -> Response {
    if is_private(addr.ip()) {
        next.run(req).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
}

pub fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            // 100.64.0.0/10: carrier-grade NAT, which Tailscale hands out.
            v4.is_private() || v4.is_loopback() || v4.is_link_local() || (a == 100 && (b & 0xc0) == 64)
        }
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            v6.is_loopback()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || v6.to_ipv4_mapped().is_some_and(|v4| is_private(IpAddr::V4(v4)))
        }
    }
}

/// The names the server answers to: localhost, this PC's name (and `.local`), and server.toml's.
pub struct Names(HashSet<String>);

impl Names {
    pub fn new(configured: &[String]) -> Self {
        let mut names: HashSet<String> = configured.iter().map(|n| n.to_ascii_lowercase()).collect();
        names.insert("localhost".into());
        if let Some(host) = crate::tls::hostname() {
            names.insert(format!("{host}.local"));
            names.insert(host);
        }
        Self(names)
    }

    /// A Host header naming this server: one of its names, or an address at home (a page that
    /// rebinds its own domain to the server's address still says its domain here).
    fn allows(&self, host: &str) -> bool {
        let name = host_name(host);
        if let Ok(ip) = name.parse::<IpAddr>() {
            return is_private(ip);
        }
        self.0.contains(&name)
    }
}

/// "tvl.local:47960" → "tvl.local"; "[fe80::1]:47960" → "fe80::1". Lower case.
fn host_name(host: &str) -> String {
    let host = host.trim();
    let name = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        host.rsplit_once(':').filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit())).map_or(host, |(h, _)| h)
    };
    name.to_ascii_lowercase()
}

/// A page's Origin ("http://192.168.68.106:47960") names the same host and port as the request.
fn same_origin(origin: &str, host: &str) -> bool {
    let Some((_, authority)) = origin.split_once("://") else { return false };
    authority.trim_end_matches('/').eq_ignore_ascii_case(host.trim())
}

/// What a POST to `path` may carry.
fn body_type_ok(path: &str, content_type: Option<&str>) -> bool {
    let Some(ct) = content_type else { return false };
    let ct = ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if path == "/api/assistant_transcribe" {
        return matches!(ct.as_str(), "audio/wav" | "audio/x-wav" | "audio/wave" | "application/octet-stream");
    }
    ct == "application/json"
}

pub enum Refusal {
    /// 421: the request names another host.
    Host,
    /// 403: sent by a page that isn't one of the server's.
    Origin,
    /// 415: a POST that doesn't say it's JSON.
    BodyType,
}

/// The checks, apart from axum so they can be tested on their own.
pub fn check_request(names: &Names, method: &Method, path: &str, headers: &HeaderMap) -> Result<(), Refusal> {
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    if !names.allows(host) {
        return Err(Refusal::Host);
    }
    let upgrade = headers.get(header::UPGRADE).is_some();
    if *method == Method::POST || upgrade {
        // No Origin: not a page (a TV PC's socket, a script). A page always sends one on a
        // cross-site POST or a WebSocket.
        if let Some(origin) = headers.get(header::ORIGIN) {
            let origin = origin.to_str().unwrap_or("null");
            if !same_origin(origin, host) {
                return Err(Refusal::Origin);
            }
        }
    }
    if *method == Method::POST && path.starts_with("/api/") {
        let ct = headers.get(header::CONTENT_TYPE).and_then(|c| c.to_str().ok());
        if !body_type_ok(path, ct) {
            return Err(Refusal::BodyType);
        }
    }
    Ok(())
}

pub async fn check(State(names): State<Arc<Names>>, req: Request, next: Next) -> Response {
    match check_request(&names, req.method(), req.uri().path(), req.headers()) {
        Ok(()) => next.run(req).await,
        Err(Refusal::Host) => (StatusCode::MISDIRECTED_REQUEST, "Not this server's name").into_response(),
        Err(Refusal::Origin) => (StatusCode::FORBIDDEN, "Not from this server's pages").into_response(),
        Err(Refusal::BodyType) => (StatusCode::UNSUPPORTED_MEDIA_TYPE, "Send JSON").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(header::HeaderName, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(k.clone(), HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn only_home_addresses_get_in() {
        for ip in ["192.168.68.112", "10.0.0.5", "172.16.3.4", "127.0.0.1", "169.254.1.2", "100.101.102.103", "::1", "fe80::1", "fd7a:115c::1"] {
            assert!(is_private(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "100.128.0.1", "2001:4860::8888", "172.32.0.1"] {
            assert!(!is_private(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn hosts_are_the_servers_names_or_home_addresses() {
        let names = Names::new(&["tvl.local".into()]);
        for host in ["192.168.68.106:47960", "tvl.local:47960", "TVL.LOCAL", "localhost:47970", "[fe80::1]:47960", "127.0.0.1"] {
            assert!(names.allows(host), "{host}");
        }
        // A rebound domain still names itself; a public address isn't home.
        for host in ["evil.example:47960", "8.8.8.8:47960", "", "tvl.local.evil.example"] {
            assert!(!names.allows(host), "{host}");
        }
    }

    #[test]
    fn pages_elsewhere_cannot_post_or_listen() {
        let names = Names::new(&[]);
        let ok = |m: Method, path: &str, h: &[(header::HeaderName, &str)]| check_request(&names, &m, path, &headers(h)).is_ok();
        let host = (header::HOST, "192.168.68.106:47960");
        let json = (header::CONTENT_TYPE, "application/json; charset=utf-8");
        // The companion's own page.
        assert!(ok(Method::POST, "/api/smarthome_control", &[host.clone(), (header::ORIGIN, "http://192.168.68.106:47960"), json.clone()]));
        // A TV PC's socket or a script: no Origin.
        assert!(ok(Method::GET, "/api/agent", &[host.clone(), (header::UPGRADE, "websocket")]));
        // Another site's page.
        assert!(!ok(Method::POST, "/api/smarthome_control", &[host.clone(), (header::ORIGIN, "https://evil.example"), json.clone()]));
        assert!(!ok(Method::GET, "/api/events", &[host.clone(), (header::ORIGIN, "https://evil.example"), (header::UPGRADE, "websocket")]));
        assert!(!ok(Method::POST, "/api/x", &[host.clone(), (header::ORIGIN, "null"), json.clone()]));
        // A "simple" cross-site POST the browser sends without asking first.
        assert!(!ok(Method::POST, "/api/smarthome_control", &[host.clone(), (header::CONTENT_TYPE, "text/plain")]));
        assert!(!ok(Method::POST, "/api/smarthome_control", std::slice::from_ref(&host)));
        // Recorded audio goes to the transcription address only.
        assert!(ok(Method::POST, "/api/assistant_transcribe", &[host.clone(), (header::CONTENT_TYPE, "audio/wav")]));
        assert!(!ok(Method::POST, "/api/clock_get", &[host.clone(), (header::CONTENT_TYPE, "audio/wav")]));
        // Pages and files are plain GETs.
        assert!(ok(Method::GET, "/assets/index-abc.js", &[host]));
    }
}
