//! The HTTP clients, made once: `api()` for web APIs (answers within seconds), `download()` for
//! files that take minutes. The old launcher reached one shared client through seven copies of
//! the same helper and built a new one for each LAN command.
//!
//! Errors never carry the request's address (see `Error::Http`).

use std::sync::OnceLock;
use std::time::Duration;

use reqwest::{RequestBuilder, Response, StatusCode};

use crate::error::{Error, Result};

const USER_AGENT: &str = concat!("TVLauncher/", env!("CARGO_PKG_VERSION"));

static API: OnceLock<reqwest::Client> = OnceLock::new();
static DOWNLOAD: OnceLock<reqwest::Client> = OnceLock::new();

/// For web APIs: 10 s for the whole request.
pub fn api() -> reqwest::Client {
    API.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|e| {
                tracing::error!("HTTP client settings refused, using the defaults: {e}");
                reqwest::Client::new()
            })
    })
    .clone()
}

/// For large files: no limit on the whole transfer, but a stalled one gives up after a minute.
pub fn download() -> reqwest::Client {
    DOWNLOAD
        .get_or_init(|| {
            reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .connect_timeout(Duration::from_secs(15))
                .read_timeout(Duration::from_secs(60))
                .build()
                .unwrap_or_else(|e| {
                    tracing::error!("download client settings refused, using the defaults: {e}");
                    reqwest::Client::new()
                })
        })
        .clone()
}

/// The longest a server's "try again in…" is honoured; past it the error is returned.
const LONGEST_WAIT: Duration = Duration::from_secs(30);

/// Sends a request that's safe to repeat (a read), up to `tries` times: again after a network
/// failure, a 5xx, or a 429, waiting what the server asks (`Retry-After`) or a growing pause with
/// a random part, so many screens don't retry in step. Never for requests that change something.
pub async fn send_with_retry(req: RequestBuilder, tries: u32) -> Result<Response> {
    let mut pause = Duration::from_millis(500);
    let mut attempt = 1;
    loop {
        let this = req.try_clone().ok_or_else(|| Error::msg("this request can't be repeated"))?;
        let last = attempt >= tries;
        match this.send().await {
            Ok(r) if !again(r.status()) || last => return Ok(r),
            Ok(r) => {
                let wait = retry_after(&r).unwrap_or_else(|| jitter(pause));
                if wait > LONGEST_WAIT {
                    return Ok(r);
                }
                tokio::time::sleep(wait).await;
            }
            Err(e) if last || !(e.is_connect() || e.is_timeout()) => return Err(e.into()),
            Err(_) => tokio::time::sleep(jitter(pause)).await,
        }
        pause *= 2;
        attempt += 1;
    }
}

fn again(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// `Retry-After` in seconds (the HTTP-date form isn't used by the APIs here).
fn retry_after(r: &Response) -> Option<Duration> {
    let secs: u64 = r.headers().get(reqwest::header::RETRY_AFTER)?.to_str().ok()?.trim().parse().ok()?;
    Some(Duration::from_secs(secs))
}

/// `d` plus up to a quarter more.
fn jitter(d: Duration) -> Duration {
    let mut b = [0u8; 2];
    let _ = getrandom::fill(&mut b);
    let quarter = d.as_millis() as u64 / 4;
    let extra = if quarter == 0 { 0 } else { u64::from(u16::from_le_bytes(b)) % quarter };
    d + Duration::from_millis(extra)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Answers each connection with the next canned response.
    async fn serve(replies: Vec<&'static str>) -> (String, tokio::task::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/x", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut served = 0;
            for reply in replies {
                let (mut s, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf).await;
                s.write_all(reply.as_bytes()).await.unwrap();
                served += 1;
            }
            served
        });
        (url, task)
    }

    #[tokio::test]
    async fn a_busy_server_is_asked_again_after_its_wait() {
        let (url, task) = serve(vec![
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
        ])
        .await;
        let r = send_with_retry(reqwest::Client::new().get(&url), 3).await.unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(r.text().await.unwrap(), "ok");
        assert_eq!(task.await.unwrap(), 2);
    }

    #[tokio::test]
    async fn the_last_try_returns_what_the_server_said() {
        let (url, _task) = serve(vec![
            "HTTP/1.1 503 Service Unavailable\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 503 Service Unavailable\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ])
        .await;
        let r = send_with_retry(reqwest::Client::new().get(&url), 2).await.unwrap();
        assert_eq!(r.status(), 503);
    }

    #[test]
    fn jitter_adds_at_most_a_quarter() {
        for _ in 0..50 {
            let j = jitter(Duration::from_millis(400));
            assert!(j >= Duration::from_millis(400) && j < Duration::from_millis(500), "{j:?}");
        }
    }
}
