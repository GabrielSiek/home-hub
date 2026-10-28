//! The Home's Live now row: live sports (the Sports tab's games and channels), the Twitch
//! channels followed and the YouTube channels subscribed to. A source that isn't set up or
//! fails is left out, so the others still show.

use std::time::Duration;

use serde::Serialize;
use tauri::AppHandle;

use crate::error::{Error, Result};
use crate::{sports, twitch, youtube};

/// How long the row waits for YouTube. A look through hundreds of subscriptions' feeds takes
/// longer; it carries on in the background and the next ask gets it.
const YOUTUBE_WAIT: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Serialize)]
pub struct LiveNow {
    pub sports: sports::Live,
    pub twitch: Vec<twitch::Stream>,
    pub youtube: Vec<youtube::Live>,
}

pub async fn now(app: &AppHandle) -> LiveNow {
    let (sports, twitch, youtube) = futures_util::join!(sports::live(app), followed(app), subscribed(app));
    LiveNow {
        sports: sports.unwrap_or_else(|e| {
            eprintln!("live: sports: {e}");
            sports::Live::default()
        }),
        twitch: twitch.unwrap_or_else(|e| {
            eprintln!("live: Twitch: {e}");
            Vec::new()
        }),
        youtube: youtube.unwrap_or_else(|e| {
            eprintln!("live: {e}");
            Vec::new()
        }),
    }
}

/// The followed Twitch channels that are live; none while signed out.
async fn followed(app: &AppHandle) -> Result<Vec<twitch::Stream>> {
    if !twitch::snapshot(app).link.signed_in {
        return Ok(Vec::new());
    }
    twitch::streams(app, "following", None).await
}

/// The subscriptions' live streams, or what they were at the last look while a new one runs long.
async fn subscribed(app: &AppHandle) -> Result<Vec<youtube::Live>> {
    let task = tauri::async_runtime::spawn({
        let app = app.clone();
        async move { youtube::live(&app).await }
    });
    match tokio::time::timeout(YOUTUBE_WAIT, task).await {
        Ok(done) => done.map_err(|e| Error::msg(e.to_string()))?,
        Err(_) => Ok(youtube::last(app)),
    }
}
