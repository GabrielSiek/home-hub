//! The local end of an OAuth sign-in: the provider's page, open in a TV PC's browser, redirects to
//! http://127.0.0.1:<port>/<path>?… and this answers it.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::parse_query;
use crate::error::{Error, Result};

const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Serves redirects to `path` until one passes `accept`, and returns its query. Gives up when
/// `still_open` says the sign-in page was closed (e.g. with Home on the controller), or after a
/// few minutes. `service` names the provider in messages.
pub async fn wait_for_redirect(
    listener: &TcpListener,
    path: &str,
    service: &str,
    still_open: impl Fn() -> bool,
    accept: impl Fn(&HashMap<String, String>) -> bool,
) -> Result<HashMap<String, String>> {
    let deadline = Instant::now() + SIGN_IN_TIMEOUT;
    let prefix = format!("{path}?");
    loop {
        if Instant::now() > deadline {
            return Err(Error::msg(format!("{service} sign-in timed out")));
        }
        if !still_open() {
            return Err(Error::msg(format!("{service} sign-in closed")));
        }
        let Ok(Ok((mut sock, _))) = tokio::time::timeout(Duration::from_millis(500), listener.accept()).await else {
            continue;
        };
        let mut buf = vec![0u8; 8192];
        let n = match tokio::time::timeout(Duration::from_secs(5), sock.read(&mut buf)).await {
            Ok(Ok(n)) => n,
            _ => continue,
        };
        let request = String::from_utf8_lossy(&buf[..n]);
        let target = request.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("");
        let found = target.strip_prefix(prefix.as_str()).map(parse_query).filter(|q| accept(q));
        let (status, page) = match &found {
            Some(q) if q.contains_key("error") => ("200 OK", format!("{service} sign-in cancelled.")),
            Some(_) => ("200 OK", format!("Signed in to {service}. Going back to the TV Launcher…")),
            None => ("404 Not Found", format!("Not the {service} sign-in.")),
        };
        let html = format!(
            "<!doctype html><meta charset=utf-8><title>TV Launcher</title>\
             <body style=\"background:#0c0c14;color:#f2f2f3;font:28px sans-serif;display:grid;place-items:center;height:100vh;margin:0\">{page}"
        );
        let reply = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
            html.len()
        );
        let _ = sock.write_all(reply.as_bytes()).await;
        let _ = sock.shutdown().await;
        if let Some(q) = found {
            return Ok(q);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn get(addr: std::net::SocketAddr, target: &str) -> String {
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        s.write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes()).await.unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    }

    #[tokio::test]
    async fn the_matching_redirect_ends_the_wait() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let waiting = tokio::spawn(async move {
            wait_for_redirect(&listener, "/google", "Google", || true, |q| q.get("state").map(String::as_str) == Some("s1")).await
        });
        // A stray request (the favicon) and a stale sign-in are answered and ignored.
        assert!(get(addr, "/favicon.ico").await.starts_with("HTTP/1.1 404"));
        assert!(get(addr, "/google?state=old&code=x").await.starts_with("HTTP/1.1 404"));
        assert!(get(addr, "/google?state=s1&code=abc").await.contains("Signed in to Google"));
        let q = waiting.await.unwrap().unwrap();
        assert_eq!(q["code"], "abc");
    }

    #[tokio::test]
    async fn a_closed_sign_in_page_stops_the_wait() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let e = wait_for_redirect(&listener, "/spotify", "Spotify", || false, |_| true).await.unwrap_err();
        assert_eq!(e.to_string(), "Spotify sign-in closed");
    }
}
