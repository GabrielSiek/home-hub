//! YouTube for the Home's Live now row: the live streams of the channels the account
//! subscribes to. Reading the subscriptions takes the account, so this signs in to Google with
//! read-only YouTube access, through the Desktop app client Google Photos uses.
//!
//! - Subscriptions: `subscriptions.list` (1 quota unit per 50), every six hours.
//! - What each channel put up lately: its RSS feed (no quota), the 15 newest videos, every ten
//!   minutes. A live stream is among them even when it was scheduled weeks before (checked
//!   2026-10-25 on CazéTV: its live match was listed first).
//! - Which are live: `videos.list` (1 unit per 50). A video that's neither live nor scheduled
//!   never will be, so each is asked about once; the live and scheduled ones every few minutes.
//!   A search per channel (`search.list`, 100 units) would use up the free 10,000 a day at once.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use futures_util::{stream, StreamExt};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{Error, Result};
use crate::history::now_ms;
use crate::launcher;
use crate::photos::{self, Token};
use tvl_protocol::ServiceLink as Link;
use crate::state::AppState;

const TOKEN_FILE: &str = "youtube.json";
const SCOPE: &str = "https://www.googleapis.com/auth/youtube.readonly";
const API: &str = "https://www.googleapis.com/youtube/v3";
const FEED: &str = "https://www.youtube.com/feeds/videos.xml?channel_id=";
/// The YouTube TV app, at a video.
const WATCH: &str = "https://www.youtube.com/tv#/watch?v=";
/// Running-app id of the plain watch page, when the YouTube app was taken out of apps.toml.
const WATCH_PAGE: &str = "youtube-watch";
const SUBS_MS: i64 = 6 * 3_600_000;
const FEEDS_MS: i64 = 10 * 60_000;
const LIVE_MS: i64 = 3 * 60_000;
/// After a failed look, what was live is still shown this long.
const STALE_MS: i64 = 15 * 60_000;
/// Feeds read at once.
const PARALLEL: usize = 8;
/// Subscriptions read at most (20 pages).
const MAX_SUBS: usize = 1000;

/// YouTube's video categories, which have kept their ids for years.
const CATEGORIES: &[(&str, &str)] = &[
    ("1", "Film & Animation"),
    ("2", "Autos & Vehicles"),
    ("10", "Music"),
    ("15", "Pets & Animals"),
    ("17", "Sports"),
    ("19", "Travel & Events"),
    ("20", "Gaming"),
    ("22", "People & Blogs"),
    ("23", "Comedy"),
    ("24", "Entertainment"),
    ("25", "News & Politics"),
    ("26", "Howto & Style"),
    ("27", "Education"),
    ("28", "Science & Technology"),
    ("29", "Nonprofits & Activism"),
];

/// A live stream on a channel subscribed to.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Live {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub avatar: Option<String>,
    /// YouTube's category: "Gaming", "Music", "Sports".
    pub category: Option<String>,
    /// Watching now.
    pub viewers: u64,
    /// When it went live, unix ms.
    pub started: Option<i64>,
    pub thumb: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct Channel {
    id: String,
    title: String,
    avatar: Option<String>,
}

#[derive(Default)]
struct Inner {
    token: Option<Token>,
    error: Option<String>,
    /// The channels subscribed to, and when they were read.
    subs: Option<(i64, Vec<Channel>)>,
    feeds_at: i64,
    /// Videos to ask about, with their channel's id: new in a feed, live or scheduled.
    watch: HashMap<String, String>,
    /// Videos that are neither live nor scheduled, so never will be.
    done: HashSet<String>,
    /// What was live at the last look, and when.
    live: Option<(i64, Vec<Live>)>,
}

pub struct State {
    token_path: PathBuf,
    inner: Mutex<Inner>,
    /// One look at the channels at a time; the next caller gets its answer.
    looking: tokio::sync::Mutex<()>,
}

impl State {
    pub fn new(data_dir: &Path) -> Self {
        let token_path = data_dir.join(TOKEN_FILE);
        let token = fs::read_to_string(&token_path).ok().and_then(|s| serde_json::from_str(&s).ok());
        Self {
            token_path,
            inner: Mutex::new(Inner { token, ..Default::default() }),
            looking: tokio::sync::Mutex::new(()),
        }
    }

    fn set_token(&self, token: Option<Token>) -> Result<()> {
        match &token {
            Some(t) => fs::write(&self.token_path, serde_json::to_string_pretty(t)?)?,
            None if self.token_path.exists() => fs::remove_file(&self.token_path)?,
            None => {}
        }
        self.inner.lock().unwrap().token = token;
        Ok(())
    }
}

fn state(app: &AppHandle) -> &State {
    &app.state::<AppState>().inner().youtube
}

fn http(app: &AppHandle) -> reqwest::Client {
    app.state::<AppState>().http.clone()
}

pub fn snapshot(app: &AppHandle) -> Link {
    let configured = app.state::<AppState>().config.read().unwrap().settings.google_client().is_some();
    let inner = state(app).inner.lock().unwrap();
    let signed_in = inner.token.is_some();
    Link {
        configured,
        signed_in,
        connected: signed_in && inner.error.is_none() && inner.live.is_some(),
        error: inner.error.clone(),
    }
}

fn emit(app: &AppHandle) {
    let _ = app.emit("youtube", snapshot(app));
}

// ---------- Sign-in ----------

pub async fn sign_in(app: &AppHandle) -> Result<()> {
    let token = photos::consent(app, SCOPE).await?;
    let st = state(app);
    st.set_token(Some(token))?;
    forget(st);
    emit(app);
    Ok(())
}

pub fn sign_out(app: &AppHandle) -> Result<()> {
    let st = state(app);
    st.set_token(None)?;
    forget(st);
    emit(app);
    Ok(())
}

/// Another account (or none): its subscriptions and what was live on them go.
fn forget(st: &State) {
    let mut inner = st.inner.lock().unwrap();
    inner.error = None;
    inner.subs = None;
    inner.feeds_at = 0;
    inner.watch.clear();
    inner.live = None;
}

/// A valid access token, refreshed when it's about to expire. Only `look` asks, one at a time.
async fn access_token(app: &AppHandle) -> Result<String> {
    let st = state(app);
    let tok = st.inner.lock().unwrap().token.clone().ok_or_else(|| Error::msg("Not signed in to YouTube"))?;
    if tok.fresh() {
        return Ok(tok.access().to_string());
    }
    match photos::renew(app, &tok).await? {
        Ok(t) => {
            let access = t.access().to_string();
            st.set_token(Some(t))?;
            Ok(access)
        }
        Err(why) => {
            st.set_token(None)?;
            forget(st);
            emit(app);
            Err(Error::msg(format!("YouTube sign-in expired ({why}): sign in again in Settings")))
        }
    }
}

// ---------- Live streams ----------

/// What was live at the last look, however long ago.
pub fn last(app: &AppHandle) -> Vec<Live> {
    state(app).inner.lock().unwrap().live.as_ref().map(|(_, l)| l.clone()).unwrap_or_default()
}

/// The subscriptions' live streams, the most watched first; none while signed out.
pub async fn live(app: &AppHandle) -> Result<Vec<Live>> {
    let st = state(app);
    if st.inner.lock().unwrap().token.is_none() {
        return Ok(Vec::new());
    }
    let _one = st.looking.lock().await;
    let now = now_ms();
    let last = st.inner.lock().unwrap().live.clone();
    if let Some((at, list)) = &last {
        if now - at < LIVE_MS {
            return Ok(list.clone());
        }
    }
    match look(app, now).await {
        Ok(list) => {
            let changed = {
                let mut inner = st.inner.lock().unwrap();
                let changed = inner.error.is_some() || inner.live.is_none();
                inner.error = None;
                inner.live = Some((now, list.clone()));
                changed
            };
            if changed {
                emit(app);
            }
            Ok(list)
        }
        Err(e) => {
            st.inner.lock().unwrap().error = Some(e.to_string());
            emit(app);
            match last {
                Some((at, list)) if now - at < STALE_MS => Ok(list),
                _ => Err(e),
            }
        }
    }
}

async fn look(app: &AppHandle, now: i64) -> Result<Vec<Live>> {
    let token = access_token(app).await?;
    let st = state(app);
    let known = st.inner.lock().unwrap().subs.clone();
    let subs = match known {
        Some((at, subs)) if now - at < SUBS_MS => subs,
        _ => {
            let subs = subscriptions(app, &token).await?;
            let mut inner = st.inner.lock().unwrap();
            // Unsubscribed since: their videos aren't asked about any more.
            inner.watch.retain(|_, c| subs.iter().any(|s| s.id == *c));
            inner.subs = Some((now, subs.clone()));
            subs
        }
    };
    if now - st.inner.lock().unwrap().feeds_at >= FEEDS_MS {
        let found = feeds(app, &subs).await;
        let mut inner = st.inner.lock().unwrap();
        inner.feeds_at = now;
        for (video, channel) in found {
            if !inner.done.contains(&video) {
                inner.watch.entry(video).or_insert(channel);
            }
        }
    }
    let ids: Vec<String> = st.inner.lock().unwrap().watch.keys().cloned().collect();
    let mut seen: HashMap<String, Option<Live>> = HashMap::new();
    for chunk in ids.chunks(50) {
        let url = format!("{API}/videos?part=snippet,liveStreamingDetails&maxResults=50&id={}", chunk.join(","));
        seen.extend(parse_videos(&get(app, &token, &url).await?, &subs));
    }
    let mut live = Vec::new();
    let mut inner = st.inner.lock().unwrap();
    for id in ids {
        match seen.remove(&id) {
            Some(Some(l)) => live.push(l),
            // Scheduled: asked about again next time.
            Some(None) => {}
            // Neither, or gone (deleted, made private).
            None => {
                inner.watch.remove(&id);
                inner.done.insert(id);
            }
        }
    }
    live.sort_by_key(|l| std::cmp::Reverse(l.viewers));
    Ok(live)
}

/// One Data API call as the account.
async fn get(app: &AppHandle, token: &str, url: &str) -> Result<Value> {
    let r = http(app).get(url).bearer_auth(token).send().await?;
    let status = r.status();
    let v: Value = r.json().await.unwrap_or_default();
    if status.is_success() {
        return Ok(v);
    }
    // "YouTube Data API v3 has not been used in project … before or it is disabled."
    Err(Error::msg(match v["error"]["message"].as_str() {
        Some(m) if !m.is_empty() => format!("YouTube: {m}"),
        _ => format!("YouTube: {status}"),
    }))
}

async fn subscriptions(app: &AppHandle, token: &str) -> Result<Vec<Channel>> {
    let mut out = Vec::new();
    let mut page: Option<String> = None;
    loop {
        let token_part = page.map(|p| format!("&pageToken={p}")).unwrap_or_default();
        let url = format!("{API}/subscriptions?part=snippet&mine=true&maxResults=50{token_part}");
        let (more, next) = parse_subscriptions(&get(app, token, &url).await?);
        out.extend(more);
        match next {
            Some(n) if out.len() < MAX_SUBS => page = Some(n),
            _ => break,
        }
    }
    Ok(out)
}

/// The videos in every channel's feed, with the channel's id. A feed that doesn't answer is
/// skipped until next time.
async fn feeds(app: &AppHandle, subs: &[Channel]) -> Vec<(String, String)> {
    let http = http(app);
    // Owned ids: a closure over `&Channel` here fails the command future's lifetime check.
    let ids: Vec<String> = subs.iter().map(|c| c.id.clone()).collect();
    let lists: Vec<Vec<(String, String)>> = stream::iter(ids.into_iter().map(|id| {
        let http = http.clone();
        async move {
            let r = http.get(format!("{FEED}{id}")).send().await.ok()?.error_for_status().ok()?;
            let xml = r.text().await.ok()?;
            Some(feed_videos(&xml).into_iter().map(|v| (v, id.clone())).collect())
        }
    }))
    .buffer_unordered(PARALLEL)
    .filter_map(|x| async move { x })
    .collect()
    .await;
    lists.into_iter().flatten().collect()
}

fn parse_subscriptions(v: &Value) -> (Vec<Channel>, Option<String>) {
    let subs = v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| {
            let sn = &i["snippet"];
            Some(Channel {
                id: sn["resourceId"]["channelId"].as_str()?.to_string(),
                title: sn["title"].as_str().unwrap_or_default().to_string(),
                avatar: ["medium", "default"]
                    .iter()
                    .find_map(|k| sn["thumbnails"][k]["url"].as_str())
                    .map(str::to_string),
            })
        })
        .collect();
    (subs, v["nextPageToken"].as_str().map(str::to_string))
}

/// The video ids in a channel's RSS feed.
fn feed_videos(xml: &str) -> Vec<String> {
    xml.split("<yt:videoId>")
        .skip(1)
        .filter_map(|s| s.split_once("</yt:videoId>").map(|(id, _)| id.trim().to_string()))
        .filter(|id| !id.is_empty())
        .collect()
}

/// What `videos.list` said of each video it knows: live (Some) or scheduled (None). Those it
/// left out are neither.
fn parse_videos(v: &Value, subs: &[Channel]) -> Vec<(String, Option<Live>)> {
    v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| {
            let id = x["id"].as_str()?.to_string();
            let sn = &x["snippet"];
            match sn["liveBroadcastContent"].as_str()? {
                "live" => {}
                "upcoming" => return Some((id, None)),
                _ => return None,
            }
            let ls = &x["liveStreamingDetails"];
            let channel_id = sn["channelId"].as_str().unwrap_or_default();
            let sub = subs.iter().find(|s| s.id == channel_id);
            let live = Live {
                id: id.clone(),
                title: sn["title"].as_str().unwrap_or_default().to_string(),
                channel: sn["channelTitle"]
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| sub.map(|s| s.title.clone()))
                    .unwrap_or_default(),
                avatar: sub.and_then(|s| s.avatar.clone()),
                category: sn["categoryId"]
                    .as_str()
                    .and_then(|c| CATEGORIES.iter().find(|(id, _)| *id == c))
                    .map(|(_, name)| name.to_string()),
                viewers: ls["concurrentViewers"].as_str().and_then(|n| n.parse().ok()).unwrap_or(0),
                started: ls["actualStartTime"].as_str().and_then(photos::rfc3339_ms),
                thumb: ["maxres", "standard", "high", "medium"]
                    .iter()
                    .find_map(|k| sn["thumbnails"][k]["url"].as_str())
                    .map(str::to_string),
            };
            Some((id, Some(live)))
        })
        .collect()
}

/// Plays a video in the YouTube TV app, or on the plain watch page when that app was taken out.
pub fn watch(app: &AppHandle, id: &str) -> Result<()> {
    launcher::launch_web(app, "youtube", &format!("{WATCH}{id}"), None)
        .or_else(|_| launcher::open_page(app, &format!("https://www.youtube.com/watch?v={id}"), WATCH_PAGE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feed() {
        let xml = r#"<feed><yt:channelId>UCZiYbVptd3PVPf4f6eR6UaQ</yt:channelId><published>2019-02-02T21:42:12+00:00</published>
<entry><id>yt:video:UEV1NElLuMA</id><yt:videoId>UEV1NElLuMA</yt:videoId><published>2026-10-05T17:59:42+00:00</published></entry>
<entry><id>yt:video:SlIASGFE4Xw</id><yt:videoId>SlIASGFE4Xw</yt:videoId></entry></feed>"#;
        assert_eq!(feed_videos(xml), ["UEV1NElLuMA", "SlIASGFE4Xw"]);
        assert!(feed_videos("<html></html>").is_empty());
    }

    #[test]
    fn subscriptions_page() {
        let v: Value = serde_json::from_str(
            r#"{"nextPageToken":"CDIQAA","items":[
              {"snippet":{"title":"CazéTV","resourceId":{"kind":"youtube#channel","channelId":"UCZiYbVptd3PVPf4f6eR6UaQ"},
                "thumbnails":{"default":{"url":"https://yt3.ggpht.com/a=s88"},"medium":{"url":"https://yt3.ggpht.com/a=s240"}}}},
              {"snippet":{"title":"No picture","resourceId":{"channelId":"UC2"},"thumbnails":{}}}
            ]}"#,
        )
        .unwrap();
        let (subs, next) = parse_subscriptions(&v);
        assert_eq!(next.as_deref(), Some("CDIQAA"));
        assert_eq!(subs[0].id, "UCZiYbVptd3PVPf4f6eR6UaQ");
        assert_eq!(subs[0].avatar.as_deref(), Some("https://yt3.ggpht.com/a=s240"));
        assert_eq!((subs[1].title.as_str(), subs[1].avatar.as_deref()), ("No picture", None));
    }

    #[test]
    fn videos() {
        let subs = [Channel {
            id: "UCZiYbVptd3PVPf4f6eR6UaQ".into(),
            title: "CazéTV".into(),
            avatar: Some("https://yt3.ggpht.com/a=s240".into()),
        }];
        let v: Value = serde_json::from_str(
            r#"{"items":[
              {"id":"UEV1NElLuMA","snippet":{"channelId":"UCZiYbVptd3PVPf4f6eR6UaQ","channelTitle":"CazéTV","categoryId":"17",
                "title":"AO VIVO: BARCELONA X REAL MADRID","liveBroadcastContent":"live",
                "thumbnails":{"high":{"url":"https://i.ytimg.com/vi/UEV1NElLuMA/hqdefault_live.jpg"}}},
               "liveStreamingDetails":{"actualStartTime":"2026-10-25T17:29:12Z","concurrentViewers":"243247"}},
              {"id":"t4pf4qZ0c14","snippet":{"liveBroadcastContent":"upcoming"},"liveStreamingDetails":{"scheduledStartTime":"2026-10-28T18:30:00Z"}},
              {"id":"jZXkDVmpoU0","snippet":{"liveBroadcastContent":"none"}}
            ]}"#,
        )
        .unwrap();
        let found = parse_videos(&v, &subs);
        assert_eq!(found.len(), 2);
        let live = found[0].1.as_ref().unwrap();
        assert_eq!((live.viewers, live.category.as_deref()), (243_247, Some("Sports")));
        assert_eq!(live.avatar.as_deref(), Some("https://yt3.ggpht.com/a=s240"));
        assert_eq!(live.started, photos::rfc3339_ms("2026-10-25T17:29:12Z"));
        assert_eq!(found[1], ("t4pf4qZ0c14".to_string(), None));
    }
}
