//! Playing from the house's library. To the rest of the launcher it's one more service, "library":
//! the same details screen, curtain, progress, Continue watching and My list. Only the last step
//! differs: the file comes from the home server and plays in the launcher's own player (mpv/).

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use super::{end, event, now_secs, season_counts, session, take_sample, Session};
use crate::error::{Error, Result};
use crate::mpv::{self, Opening, PlayerState};
use crate::state::AppState;
use crate::tmdb;

/// The service id of the library.
pub const SERVICE: &str = "library";

/// A saved position this close to the start or the end isn't worth going back to.
const RESUME_FROM: f64 = 30.0;
const RESUME_UNTIL: f64 = 120.0;

/// Sent the moment a video with nothing after it ends; once, whatever mpv repeats.
static CLOSING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Deserialize)]
struct Subtitle {
    path: String,
    lang: String,
}

#[derive(Debug, Clone, Copy, Deserialize)]
struct Next {
    season: u32,
    episode: u32,
}

/// `library_stream`'s answer.
#[derive(Debug, Deserialize)]
struct Stream {
    path: String,
    season: u32,
    episode: u32,
    #[serde(default)]
    subtitles: Vec<Subtitle>,
    next: Option<Next>,
}

fn label(season: u32, episode: u32) -> String {
    format!("S{season} · E{episode}")
}

/// Where to start: the place the store has for this very movie or episode, when it's worth it.
fn resume_at(p: Option<&super::Progress>, season: u32, episode: u32) -> f64 {
    p.filter(|p| !p.finished && p.season == season && p.episode == episode)
        .filter(|p| p.position >= RESUME_FROM && (p.duration <= 0.0 || p.duration - p.position >= RESUME_UNTIL))
        .map_or(0.0, |p| p.position)
}

async fn ask_stream(app: &AppHandle, media_type: &str, id: u64, season: Option<u32>, episode: Option<u32>) -> Result<Stream> {
    let args = json!({ "mediaType": media_type, "id": id, "season": season, "episode": episode });
    let v = crate::agent::server_call(app, "library_stream", args).await.map_err(Error::msg)?;
    Ok(serde_json::from_value(v)?)
}

/// `http://<server><path>`: the file comes over plain HTTP from the address this TV PC already
/// reaches the server at. Inside the house only, and the path holds a one-off token.
fn file_url(host: &str, port: u16, path: &str) -> String {
    let host = if host.contains(':') { format!("[{host}]") } else { host.to_string() };
    format!("http://{host}:{port}{path}")
}

/// Plays `id` from the library: an episode, or (none given) where the user is.
pub async fn play(app: &AppHandle, media_type: &str, id: u64, season: Option<u32>, episode: Option<u32>) -> Result<()> {
    let opened = now_secs();
    let st = app.state::<AppState>();
    let settings = st.config.read().unwrap().settings.clone();
    end(app);
    CLOSING.store(false, Ordering::SeqCst);

    let details = tmdb::details(&st.http, &settings, media_type, id).await?;
    let series = media_type == "tv";
    let saved = st.watch.store.lock().unwrap().get(SERVICE, media_type, id).cloned();
    // Unfinished: the episode the user is on. Finished or new: the first one here (the server picks).
    let wanted = season.zip(episode).or_else(|| {
        saved.as_ref().filter(|p| series && !p.finished && p.season > 0).map(|p| (p.season, p.episode))
    });

    let seasons = if series { season_counts(&details) } else { Vec::new() };
    let (first_s, first_e) = wanted.unwrap_or((0, 0));
    let mut session = Session::new(
        SERVICE.to_string(),
        media_type.to_string(),
        id,
        details.title.title.clone(),
        details.title.poster.clone(),
        details.title.backdrop.clone(),
        seasons,
        first_s,
        first_e,
    );
    session.runtime = details.runtime.unwrap_or(0) as f64 * 60.0;
    session.opened_at = opened;
    let _ = app.emit("watch", event("opening", &session));
    let failed = |e: Error, s: &Session| {
        let _ = app.emit("watch", event("closed", s));
        e
    };

    let (s, e) = wanted.unzip();
    let stream = match ask_stream(app, media_type, id, s, e).await {
        Ok(found) => found,
        // The saved episode is gone from the library: the first one that's there.
        Err(_) if season.is_none() && wanted.is_some() => match ask_stream(app, media_type, id, None, None).await {
            Ok(found) => found,
            Err(e) => return Err(failed(e, &session)),
        },
        Err(e) => return Err(failed(e, &session)),
    };
    session.season = stream.season;
    session.episode = stream.episode;
    let start = resume_at(saved.as_ref(), stream.season, stream.episode);

    let Some((host, port)) = crate::agent::link(app).and_then(|l| l.server_http()) else {
        return Err(failed(Error::msg("The home server's address isn't known yet"), &session));
    };
    if !mpv::installed(app) {
        let _ = app.emit("watch", event("preparing", &session));
    }
    let opening = Opening {
        url: file_url(&host, port, &stream.path),
        start,
        subtitles: stream.subtitles.into_iter().map(|s| (file_url(&host, port, &s.path), s.lang)).collect(),
        title: details.title.title.clone(),
        subtitle: series.then(|| label(stream.season, stream.episode)),
        next: stream.next.map(|n| label(n.season, n.episode)),
        language: settings.language.clone(),
    };
    *st.watch.next.lock().unwrap() = stream.next.map(|n| (n.season, n.episode));
    *st.watch.live.lock().unwrap() = Some(session);
    if let Err(err) = mpv::start(app, opening).await {
        end(app);
        return Err(err);
    }
    Ok(())
}

/// The next episode, from the player: its button, or the end of this one.
pub async fn play_next(app: &AppHandle) -> Result<()> {
    let st = app.state::<AppState>();
    let next = *st.watch.next.lock().unwrap();
    let what = st.watch.live.lock().unwrap().as_ref().map(|s| (s.media_type.clone(), s.id));
    match (what, next) {
        (Some((media_type, id)), Some((s, e))) => play(app, &media_type, id, Some(s), Some(e)).await,
        _ => Ok(()),
    }
}

/// How mpv is doing, as a sample of the session. The label keeps the episode what was opened: a
/// long seek back must not read as the next episode starting.
pub fn on_state(app: &AppHandle, p: &PlayerState) {
    let st = app.state::<AppState>();
    let (series, season, episode) = {
        let live = st.watch.live.lock().unwrap();
        let Some(s) = live.as_ref() else { return };
        (s.media_type == "tv", s.season, s.episode)
    };
    let sample = session::Sample {
        video: true,
        player: true,
        t: Some(p.position),
        d: Some(p.duration),
        paused: p.paused,
        ended: p.ended,
        label: series.then(|| format!("S{season}E{episode}")),
        ..Default::default()
    };
    let (outcome, started) = take_sample(app, sample);
    if let Some(ev) = started {
        let _ = app.emit("watch", ev);
    }
    let over = p.ended && p.next.is_none();
    if (over || outcome == session::Outcome::Close) && !CLOSING.swap(true, Ordering::SeqCst) {
        super::close_soon(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(position: f64, duration: f64, finished: bool) -> super::super::Progress {
        serde_json::from_value(json!({
            "service": SERVICE, "media_type": "tv", "id": 1, "title": "X", "poster": null, "backdrop": null,
            "season": 1, "episode": 2, "position": position, "duration": duration, "share": null,
            "confirmed": true, "finished": finished, "updated_at": 0, "episodes": {}, "seasons": [],
        }))
        .unwrap()
    }

    #[test]
    fn resuming_goes_back_to_the_same_episode_only() {
        assert_eq!(resume_at(Some(&at(600.0, 3000.0, false)), 1, 2), 600.0);
        assert_eq!(resume_at(Some(&at(600.0, 3000.0, false)), 1, 3), 0.0);
        assert_eq!(resume_at(None, 1, 2), 0.0);
    }

    #[test]
    fn the_very_start_and_end_arent_worth_resuming() {
        assert_eq!(resume_at(Some(&at(10.0, 3000.0, false)), 1, 2), 0.0);
        assert_eq!(resume_at(Some(&at(2950.0, 3000.0, false)), 1, 2), 0.0);
        assert_eq!(resume_at(Some(&at(600.0, 3000.0, true)), 1, 2), 0.0);
    }

    #[test]
    fn addresses_take_ipv6_hosts() {
        assert_eq!(file_url("192.168.68.20", 47960, "/api/media/x"), "http://192.168.68.20:47960/api/media/x");
        assert_eq!(file_url("fe80::1", 47960, "/x"), "http://[fe80::1]:47960/x");
    }
}
