//! qBittorrent's Web UI API: speeds and states of what's downloading, and pausing it. Radarr and
//! Sonarr drive the downloads themselves; the hub only looks, pauses when asked, and clears what's
//! left of a title that was removed.

use std::collections::HashMap;

use parking_lot::Mutex;
use reqwest::{Client, Response, StatusCode};
use serde::Deserialize;
use tvl_kit::{Error, Result};

/// "No idea" in its `eta` field: 100 days.
const NO_ETA: u64 = 8_640_000;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Torrent {
    pub hash: String,
    pub name: String,
    pub state: String,
    pub progress: f64,
    pub dlspeed: u64,
    pub upspeed: u64,
    pub eta: u64,
    pub size: u64,
}

impl Torrent {
    pub fn eta_s(&self) -> Option<u64> {
        (self.eta > 0 && self.eta < NO_ETA).then_some(self.eta)
    }

    pub fn paused(&self) -> bool {
        self.state.starts_with("paused") || self.state.starts_with("stopped")
    }
}

/// Torrents by hash, upper case as Radarr and Sonarr write it.
pub type Torrents = HashMap<String, Torrent>;

/// A sign-in that set no cookie and wasn't refused: the Web UI trusts this address. It answers "Ok."
/// to that, or nothing from WebAPI 2.14; a refusal is "Fails.", or a 401 from then on.
fn needs_no_sign_in(status: StatusCode, body: &str) -> bool {
    status.is_success() && !body.trim().eq_ignore_ascii_case("fails.")
}

pub struct Qbit<'a> {
    base: String,
    user: String,
    password: String,
    http: Client,
    /// The session cookie, kept between calls.
    session: &'a Mutex<Option<String>>,
}

impl<'a> Qbit<'a> {
    /// None when it isn't set up.
    pub fn new(url: &str, user: &str, password: &str, session: &'a Mutex<Option<String>>) -> Option<Self> {
        let url = url.trim().trim_end_matches('/');
        (!url.is_empty()).then(|| Self {
            base: url.to_string(),
            user: user.to_string(),
            password: password.to_string(),
            http: tvl_kit::http::api(),
            session,
        })
    }

    async fn login(&self) -> Result<String> {
        let res = self
            .http
            .post(format!("{}/api/v2/auth/login", self.base))
            .header("Referer", &self.base)
            .form(&[("username", self.user.as_str()), ("password", self.password.as_str())])
            .send()
            .await
            .map_err(|_| Error::msg("qBittorrent isn't answering"))?;
        let status = res.status();
        // A session cookie with any name: newer versions put the port in it.
        let cookie = res
            .headers()
            .get(reqwest::header::SET_COOKIE)
            .and_then(|c| c.to_str().ok())
            .and_then(|c| c.split(';').next())
            .map(str::to_string);
        let body = res.text().await.unwrap_or_default();
        match cookie {
            Some(c) => Ok(c),
            None if needs_no_sign_in(status, &body) => Ok(String::new()),
            None => Err(Error::msg("qBittorrent refused its username or password")),
        }
    }

    async fn cookie(&self) -> Result<String> {
        if let Some(c) = self.session.lock().clone() {
            return Ok(c);
        }
        let c = self.login().await?;
        *self.session.lock() = Some(c.clone());
        Ok(c)
    }

    /// One request, signing in again if its session ended.
    async fn send(&self, path: &str, form: Option<&[(&str, &str)]>) -> Result<Response> {
        for fresh in [false, true] {
            if fresh {
                self.session.lock().take();
            }
            let cookie = self.cookie().await?;
            let url = format!("{}/api/v2/{path}", self.base);
            let req = match form {
                Some(f) => self.http.post(url).form(f),
                None => self.http.get(url),
            };
            let res = req.header(reqwest::header::COOKIE, cookie).send().await.map_err(|_| Error::msg("qBittorrent isn't answering"))?;
            if res.status() != StatusCode::FORBIDDEN {
                return Ok(res);
            }
        }
        Err(Error::msg("qBittorrent refused its username or password"))
    }

    pub async fn torrents(&self) -> Result<Torrents> {
        let list: Vec<Torrent> = self.send("torrents/info", None).await?.json().await?;
        Ok(list.into_iter().map(|t| (t.hash.to_ascii_uppercase(), t)).collect())
    }

    /// Takes torrents out, and their data with them.
    pub async fn delete(&self, hashes: &[String]) -> Result<()> {
        let hashes = hashes.join("|");
        let res = self.send("torrents/delete", Some(&[("hashes", hashes.as_str()), ("deleteFiles", "true")])).await?;
        if res.status().is_success() {
            Ok(())
        } else {
            Err(Error::msg(format!("qBittorrent answered {}", res.status())))
        }
    }

    /// Version 5 renamed pause and resume to stop and start.
    pub async fn set_paused(&self, hash: &str, paused: bool) -> Result<()> {
        let (new, old) = if paused { ("torrents/stop", "torrents/pause") } else { ("torrents/start", "torrents/resume") };
        let form = [("hashes", hash)];
        let mut res = self.send(new, Some(&form)).await?;
        if res.status() == StatusCode::NOT_FOUND {
            res = self.send(old, Some(&form)).await?;
        }
        if res.status().is_success() {
            Ok(())
        } else {
            Err(Error::msg(format!("qBittorrent answered {}", res.status())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_eta_is_none() {
        let t = |eta| Torrent { eta, ..Default::default() };
        assert_eq!(t(120).eta_s(), Some(120));
        assert_eq!(t(NO_ETA).eta_s(), None);
        assert_eq!(t(0).eta_s(), None);
    }

    #[test]
    fn a_trusted_address_signs_in_without_a_cookie_on_every_version() {
        assert!(needs_no_sign_in(StatusCode::OK, "Ok."));
        assert!(needs_no_sign_in(StatusCode::OK, ""));
        assert!(needs_no_sign_in(StatusCode::NO_CONTENT, ""));
        assert!(!needs_no_sign_in(StatusCode::OK, "Fails."));
        assert!(!needs_no_sign_in(StatusCode::UNAUTHORIZED, ""));
        assert!(!needs_no_sign_in(StatusCode::FORBIDDEN, ""));
    }

    #[test]
    fn both_names_of_paused_count() {
        let t = |state: &str| Torrent { state: state.into(), ..Default::default() };
        assert!(t("pausedDL").paused());
        assert!(t("stoppedDL").paused());
        assert!(!t("downloading").paused());
    }
}
