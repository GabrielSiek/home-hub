//! Twitch through the Helix API: sign-in with the device code flow (a public client, so no
//! secret), and the lists the Twitch screens show. The launcher plays nothing itself: the UI
//! embeds Twitch's own player, and reads chat anonymously over IRC.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{Error, Result};
use crate::history::now_ms;
use crate::launcher;
use tvl_protocol::ServiceLink as Link;
use crate::state::AppState;

const TOKEN_FILE: &str = "twitch.json";
const ID: &str = "https://id.twitch.tv/oauth2";
const API: &str = "https://api.twitch.tv/helix";
const SCOPES: &str = "user:read:follows user:write:chat";
/// Running-app id of the activation page while it is open.
pub const SIGN_IN_APP: &str = "twitch-signin";
/// Twitch asks apps to check a user token at least once an hour.
const VALIDATE_EVERY: Duration = Duration::from_secs(3600);
const STREAMS_PER_LIST: u32 = 40;
const SEARCH_RESULTS: u32 = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Me {
    pub id: String,
    pub login: String,
    pub name: String,
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Saved {
    access: String,
    /// Single use: each refresh hands out a new one.
    refresh: String,
    /// When `access` expires, unix ms.
    expires: i64,
    me: Me,
}

/// Everything the UI shows about the account.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub link: Link,
    pub me: Option<Me>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stream {
    pub user_id: String,
    pub login: String,
    pub name: String,
    pub title: String,
    pub game_id: String,
    pub game: String,
    pub viewers: u64,
    /// RFC 3339.
    pub started_at: String,
    /// Still holds `{width}x{height}`, for the UI to fill in.
    pub thumbnail: String,
    pub tags: Vec<String>,
    pub mature: bool,
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Category {
    pub id: String,
    pub name: String,
    /// Still holds `{width}x{height}`, for the UI to fill in.
    pub box_art: String,
}

/// A search result: live or not.
#[derive(Debug, Clone, Serialize)]
pub struct Channel {
    pub id: String,
    pub login: String,
    pub name: String,
    pub game: String,
    pub title: String,
    pub live: bool,
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResults {
    pub channels: Vec<Channel>,
    pub categories: Vec<Category>,
}

/// The channel being watched.
#[derive(Debug, Clone, Serialize)]
pub struct ChannelInfo {
    pub id: String,
    pub login: String,
    pub name: String,
    pub avatar: Option<String>,
    /// None while offline.
    pub stream: Option<Stream>,
    pub following: bool,
}

#[derive(Default)]
struct Inner {
    link: Link,
    saved: Option<Saved>,
    /// Profile pictures by user id.
    avatars: HashMap<String, Option<String>>,
}

pub struct State {
    token_path: PathBuf,
    inner: Mutex<Inner>,
    /// Bumped to stop the background task (new Client ID, sign-in, sign-out).
    gen: AtomicU64,
    /// One refresh at a time: the refresh token works only once.
    refreshing: tokio::sync::Mutex<()>,
}

impl State {
    pub fn new(data_dir: &Path) -> Self {
        let token_path = data_dir.join(TOKEN_FILE);
        let saved = fs::read_to_string(&token_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok());
        Self {
            token_path,
            inner: Mutex::new(Inner {
                saved,
                ..Default::default()
            }),
            gen: AtomicU64::new(0),
            refreshing: tokio::sync::Mutex::new(()),
        }
    }

    fn saved(&self) -> Result<Saved> {
        self.inner
            .lock()
            .unwrap()
            .saved
            .clone()
            .ok_or_else(|| Error::msg("Not signed in to Twitch"))
    }

    fn set_saved(&self, saved: Option<Saved>) -> Result<()> {
        match &saved {
            Some(s) => fs::write(&self.token_path, serde_json::to_string_pretty(s)?)?,
            None if self.token_path.exists() => fs::remove_file(&self.token_path)?,
            None => {}
        }
        self.inner.lock().unwrap().saved = saved;
        Ok(())
    }
}

fn state(app: &AppHandle) -> &State {
    &app.state::<AppState>().inner().twitch
}

fn client_id(app: &AppHandle) -> Option<String> {
    app.state::<AppState>().config.read().unwrap().settings.twitch_client_id()
}

fn need_client_id(app: &AppHandle) -> Result<String> {
    client_id(app).ok_or_else(|| Error::msg("Add the Twitch Client ID in Settings first"))
}

fn http(app: &AppHandle) -> reqwest::Client {
    app.state::<AppState>().http.clone()
}

/// Configured and signed in are read live: the background task only reports whether the
/// token checks out, which takes a round trip to Twitch after every start.
pub fn snapshot(app: &AppHandle) -> Snapshot {
    let configured = client_id(app).is_some();
    let inner = state(app).inner.lock().unwrap();
    let signed_in = inner.saved.is_some();
    Snapshot {
        link: Link {
            configured,
            signed_in,
            connected: configured && signed_in && inner.link.connected,
            error: inner.link.error.clone(),
        },
        me: inner.saved.as_ref().map(|s| s.me.clone()),
    }
}

fn set_link(app: &AppHandle, link: Link) {
    state(app).inner.lock().unwrap().link = link;
    let _ = app.emit("twitch", snapshot(app));
}

// ---------- Sign-in ----------

#[derive(Deserialize)]
struct DeviceCode {
    device_code: String,
    expires_in: u64,
    interval: u64,
    verification_uri: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    /// Missing on some refreshes; the 401 retry covers it.
    expires_in: Option<i64>,
}

/// Twitch's `message` from an error reply, else the status.
async fn reason(r: reqwest::Response) -> String {
    let status = r.status();
    let body: Value = r.json().await.unwrap_or_default();
    body["message"].as_str().map(str::to_string).unwrap_or_else(|| status.to_string())
}

/// Opens Twitch's activation page in the launcher's browser with the code filled in, and
/// waits for Authorize. The browser profile is the one the Twitch web app uses, so usually
/// the account is already signed in.
pub async fn sign_in(app: &AppHandle) -> Result<()> {
    let client_id = need_client_id(app)?;
    let r = http(app)
        .post(format!("{ID}/device"))
        .form(&[("client_id", client_id.as_str()), ("scopes", SCOPES)])
        .send()
        .await?;
    if !r.status().is_success() {
        return Err(Error::msg(format!("Twitch sign-in: {}", reason(r).await)));
    }
    let code: DeviceCode = r.json().await?;
    launcher::open_page(app, &code.verification_uri, SIGN_IN_APP)?;
    let result = wait_for_token(app, &client_id, &code).await;
    launcher::close_if(app, SIGN_IN_APP);
    let t = result?;
    let me = fetch_me(app, &client_id, &t.access_token).await?;
    state(app).set_saved(Some(Saved {
        access: t.access_token,
        refresh: t.refresh_token,
        expires: now_ms() + t.expires_in.unwrap_or(3600) * 1000,
        me,
    }))?;
    start(app);
    Ok(())
}

async fn wait_for_token(app: &AppHandle, client_id: &str, code: &DeviceCode) -> Result<TokenResponse> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(code.expires_in);
    let mut every = Duration::from_secs(code.interval.max(1));
    loop {
        tokio::time::sleep(every).await;
        if tokio::time::Instant::now() > deadline {
            return Err(Error::msg("Twitch sign-in timed out"));
        }
        if !launcher::is_running(app, SIGN_IN_APP) {
            return Err(Error::msg("Twitch sign-in closed"));
        }
        let form = [
            ("client_id", client_id),
            ("scopes", SCOPES),
            ("device_code", code.device_code.as_str()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ];
        let r = http(app).post(format!("{ID}/token")).form(&form).send().await?;
        if r.status().is_success() {
            return Ok(r.json().await?);
        }
        match reason(r).await.as_str() {
            "authorization_pending" => {}
            "slow_down" => every += Duration::from_secs(5),
            m if m.contains("denied") => return Err(Error::msg("Twitch sign-in cancelled")),
            m => return Err(Error::msg(format!("Twitch sign-in failed: {m}"))),
        }
    }
}

async fn fetch_me(app: &AppHandle, client_id: &str, access: &str) -> Result<Me> {
    let v: Value = http(app)
        .get(format!("{API}/users"))
        .bearer_auth(access)
        .header("Client-Id", client_id)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let u = &v["data"][0];
    Ok(Me {
        id: str_of(&u["id"]),
        login: str_of(&u["login"]),
        name: str_of(&u["display_name"]),
        avatar: non_empty(&u["profile_image_url"]),
    })
}

pub async fn sign_out(app: &AppHandle) -> Result<()> {
    if let (Some(client_id), Ok(saved)) = (client_id(app), state(app).saved()) {
        // Best effort: the token is forgotten here either way.
        let form = [("client_id", client_id.as_str()), ("token", saved.access.as_str())];
        let _ = http(app).post(format!("{ID}/revoke")).form(&form).send().await;
    }
    state(app).set_saved(None)?;
    start(app);
    Ok(())
}

/// A valid access token, refreshed when it's about to expire (or `force`d after a 401).
async fn access_token(app: &AppHandle, force: bool) -> Result<String> {
    let s = state(app);
    let saved = s.saved()?;
    if !force && saved.expires - now_ms() > 60_000 {
        return Ok(saved.access);
    }
    let stale = saved.access;
    let _one = s.refreshing.lock().await;
    let saved = s.saved()?;
    // Refreshed by another call while this one waited.
    if saved.expires - now_ms() > 60_000 && (!force || saved.access != stale) {
        return Ok(saved.access);
    }
    let client_id = need_client_id(app)?;
    let form = [
        ("grant_type", "refresh_token"),
        ("refresh_token", saved.refresh.as_str()),
        ("client_id", client_id.as_str()),
    ];
    let r = http(app).post(format!("{ID}/token")).form(&form).send().await?;
    if r.status().is_client_error() {
        // Unused for 30 days, revoked, or another Client ID: only a new sign-in helps.
        let why = reason(r).await;
        s.set_saved(None)?;
        return Err(Error::msg(format!("Twitch sign-in expired ({why}): sign in again in Settings")));
    }
    let t: TokenResponse = r.error_for_status()?.json().await?;
    let access = t.access_token.clone();
    s.set_saved(Some(Saved {
        access: t.access_token,
        refresh: t.refresh_token,
        expires: now_ms() + t.expires_in.unwrap_or(3600) * 1000,
        me: saved.me,
    }))?;
    Ok(access)
}

// ---------- Helix ----------

/// One Helix call; an empty reply comes back as Null.
async fn call(app: &AppHandle, method: Method, path: &str, query: &[(&str, String)], body: Option<&Value>) -> Result<Value> {
    let client_id = need_client_id(app)?;
    let mut force = false;
    loop {
        let at = access_token(app, force).await?;
        let mut req = http(app)
            .request(method.clone(), format!("{API}{path}"))
            .bearer_auth(&at)
            .header("Client-Id", &client_id)
            .query(query);
        if let Some(b) = body {
            req = req.json(b);
        }
        let r = req.send().await?;
        let status = r.status();
        if status == StatusCode::UNAUTHORIZED && !force {
            force = true;
            continue;
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(Error::msg("Twitch: too many requests, try again in a moment"));
        }
        if !status.is_success() {
            return Err(Error::msg(format!("Twitch: {}", reason(r).await)));
        }
        let text = r.text().await?;
        return Ok(serde_json::from_str(&text).unwrap_or_default());
    }
}

async fn get(app: &AppHandle, path: &str, query: &[(&str, String)]) -> Result<Value> {
    call(app, Method::GET, path, query, None).await
}

fn str_of(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn non_empty(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_string)
}

fn data(v: &Value) -> impl Iterator<Item = &Value> {
    v["data"].as_array().into_iter().flatten()
}

fn tags(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t.as_str().map(str::to_string))
        .collect()
}

/// A stream from /streams or /streams/followed; the avatar is filled in later.
fn parse_stream(v: &Value) -> Stream {
    Stream {
        user_id: str_of(&v["user_id"]),
        login: str_of(&v["user_login"]),
        name: str_of(&v["user_name"]),
        title: str_of(&v["title"]),
        game_id: str_of(&v["game_id"]),
        game: str_of(&v["game_name"]),
        viewers: v["viewer_count"].as_u64().unwrap_or(0),
        started_at: str_of(&v["started_at"]),
        thumbnail: str_of(&v["thumbnail_url"]),
        tags: tags(&v["tags"]),
        mature: v["is_mature"].as_bool().unwrap_or(false),
        avatar: None,
    }
}

/// Box art as a size template. /search/categories hands out a fixed 52×72 URL instead.
fn box_art_template(url: &str) -> String {
    if url.contains("{width}") {
        return url.to_string();
    }
    let Some(dash) = url.rfind('-') else { return url.to_string() };
    let rest = &url[dash + 1..];
    let Some(dot) = rest.find('.') else { return url.to_string() };
    let size = &rest[..dot];
    let is_size = size.split_once('x').is_some_and(|(w, h)| {
        !w.is_empty() && !h.is_empty() && w.bytes().chain(h.bytes()).all(|b| b.is_ascii_digit())
    });
    if !is_size {
        return url.to_string();
    }
    format!("{}-{{width}}x{{height}}{}", &url[..dash], &rest[dot..])
}

fn parse_category(v: &Value) -> Category {
    Category {
        id: str_of(&v["id"]),
        name: str_of(&v["name"]),
        box_art: box_art_template(v["box_art_url"].as_str().unwrap_or_default()),
    }
}

/// Profile pictures for `ids`, looked up 100 at a time and remembered.
async fn avatars(app: &AppHandle, ids: &[String]) -> Result<HashMap<String, Option<String>>> {
    let missing: Vec<String> = {
        let inner = state(app).inner.lock().unwrap();
        let mut m: Vec<String> = ids.iter().filter(|id| !inner.avatars.contains_key(*id)).cloned().collect();
        m.sort();
        m.dedup();
        m
    };
    for chunk in missing.chunks(100) {
        let query: Vec<(&str, String)> = chunk.iter().map(|id| ("id", id.clone())).collect();
        let v = get(app, "/users", &query).await?;
        let mut inner = state(app).inner.lock().unwrap();
        if inner.avatars.len() > 2000 {
            inner.avatars.clear();
        }
        for id in chunk {
            inner.avatars.insert(id.clone(), None);
        }
        for u in data(&v) {
            inner.avatars.insert(str_of(&u["id"]), non_empty(&u["profile_image_url"]));
        }
    }
    let inner = state(app).inner.lock().unwrap();
    Ok(ids
        .iter()
        .map(|id| (id.clone(), inner.avatars.get(id).cloned().flatten()))
        .collect())
}

async fn with_avatars(app: &AppHandle, mut streams: Vec<Stream>) -> Result<Vec<Stream>> {
    let ids: Vec<String> = streams.iter().map(|s| s.user_id.clone()).collect();
    // Pictures are a nicety: a failed lookup still shows the streams.
    if let Ok(found) = avatars(app, &ids).await {
        for s in &mut streams {
            s.avatar = found.get(&s.user_id).cloned().flatten();
        }
    }
    Ok(streams)
}

/// Live streams: "following" (channels the account follows), "top", or "game" (`game_id`).
pub async fn streams(app: &AppHandle, source: &str, game_id: Option<&str>) -> Result<Vec<Stream>> {
    let first = ("first", STREAMS_PER_LIST.to_string());
    let v = match source {
        "following" => {
            let me = state(app).saved()?.me.id;
            get(app, "/streams/followed", &[("user_id", me), ("first", "100".into())]).await?
        }
        "top" => get(app, "/streams", &[first]).await?,
        "game" => {
            let id = game_id.ok_or_else(|| Error::msg("No category given"))?;
            get(app, "/streams", &[("game_id", id.to_string()), first]).await?
        }
        other => return Err(Error::msg(format!("unknown stream list '{other}'"))),
    };
    with_avatars(app, data(&v).map(parse_stream).collect()).await
}

/// The most watched categories right now.
pub async fn categories(app: &AppHandle) -> Result<Vec<Category>> {
    let v = get(app, "/games/top", &[("first", STREAMS_PER_LIST.to_string())]).await?;
    Ok(data(&v).map(parse_category).collect())
}

pub async fn search(app: &AppHandle, query: &str, live_only: bool) -> Result<SearchResults> {
    let n = SEARCH_RESULTS.to_string();
    let channel_query = [
        ("query", query.to_string()),
        ("first", n.clone()),
        ("live_only", live_only.to_string()),
    ];
    let category_query = [("query", query.to_string()), ("first", n)];
    let (c, g) = tokio::try_join!(
        get(app, "/search/channels", &channel_query),
        get(app, "/search/categories", &category_query),
    )?;
    let channels = data(&c)
        .map(|v| Channel {
            id: str_of(&v["id"]),
            login: str_of(&v["broadcaster_login"]),
            name: str_of(&v["display_name"]),
            game: str_of(&v["game_name"]),
            title: str_of(&v["title"]),
            live: v["is_live"].as_bool().unwrap_or(false),
            avatar: non_empty(&v["thumbnail_url"]),
        })
        .collect();
    Ok(SearchResults {
        channels,
        categories: data(&g).map(parse_category).collect(),
    })
}

/// The channel `login`, its stream when live, and whether the account follows it.
pub async fn channel(app: &AppHandle, login: &str) -> Result<ChannelInfo> {
    let users = get(app, "/users", &[("login", login.to_string())]).await?;
    let u = data(&users)
        .next()
        .ok_or_else(|| Error::msg(format!("No Twitch channel called {login}")))?;
    let id = str_of(&u["id"]);
    let me = state(app).saved()?.me.id;
    let stream_query = [("user_id", id.clone())];
    let follow_query = [("user_id", me), ("broadcaster_id", id.clone())];
    let (live, followed) = tokio::try_join!(
        get(app, "/streams", &stream_query),
        get(app, "/channels/followed", &follow_query),
    )?;
    let avatar = non_empty(&u["profile_image_url"]);
    let stream = data(&live).next().map(|v| Stream {
        avatar: avatar.clone(),
        ..parse_stream(v)
    });
    let following = data(&followed).next().is_some();
    Ok(ChannelInfo {
        id,
        login: str_of(&u["login"]),
        name: str_of(&u["display_name"]),
        avatar,
        stream,
        following,
    })
}

/// Sends `message` to the chat of `broadcaster_id` as the signed-in account.
pub async fn send_chat(app: &AppHandle, broadcaster_id: &str, message: &str) -> Result<()> {
    let message = message.trim();
    if message.is_empty() {
        return Ok(());
    }
    let me = state(app).saved()?.me.id;
    let body = json!({ "broadcaster_id": broadcaster_id, "sender_id": me, "message": message });
    let v = call(app, Method::POST, "/chat/messages", &[], Some(&body)).await?;
    let sent = &v["data"][0];
    if sent["is_sent"].as_bool() == Some(false) {
        let why = sent["drop_reason"]["message"].as_str().unwrap_or("the channel didn't accept it");
        return Err(Error::msg(format!("Message not sent: {why}")));
    }
    Ok(())
}

// ---------- Background task ----------

/// (Re)starts the background task, e.g. after a sign-in or a new Client ID.
pub fn start(app: &AppHandle) {
    let gen = state(app).gen.fetch_add(1, Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn(run(app.clone(), gen));
}

fn alive(app: &AppHandle, gen: u64) -> bool {
    state(app).gen.load(Ordering::SeqCst) == gen
}

/// Checks the token with Twitch's /validate, refreshing it once on a 401.
async fn validate(app: &AppHandle) -> Result<()> {
    for force in [false, true] {
        let at = access_token(app, force).await?;
        let r = http(app)
            .get(format!("{ID}/validate"))
            .header("Authorization", format!("OAuth {at}"))
            .send()
            .await?;
        if r.status().is_success() {
            return Ok(());
        }
        if r.status() != StatusCode::UNAUTHORIZED {
            return Err(Error::msg(format!("Twitch: {}", r.status())));
        }
    }
    state(app).set_saved(None)?;
    Err(Error::msg("Twitch sign-in expired: sign in again in Settings"))
}

/// Validates the token on start and every hour after.
async fn run(app: AppHandle, gen: u64) {
    let mut pause = Duration::from_secs(30);
    loop {
        if !alive(&app, gen) {
            return;
        }
        let configured = client_id(&app).is_some();
        let signed_in = state(&app).inner.lock().unwrap().saved.is_some();
        if !configured || !signed_in {
            set_link(
                &app,
                Link {
                    configured,
                    signed_in,
                    ..Link::default()
                },
            );
            return;
        }
        let result = validate(&app).await;
        if !alive(&app, gen) {
            return;
        }
        let signed_in = state(&app).inner.lock().unwrap().saved.is_some();
        let wait = match result {
            Ok(()) => {
                pause = Duration::from_secs(30);
                set_link(
                    &app,
                    Link {
                        configured,
                        signed_in,
                        connected: true,
                        error: None,
                    },
                );
                VALIDATE_EVERY
            }
            Err(e) => {
                set_link(
                    &app,
                    Link {
                        configured,
                        signed_in,
                        connected: false,
                        error: Some(e.to_string()),
                    },
                );
                let p = pause;
                pause = (pause * 2).min(Duration::from_secs(30 * 60));
                p
            }
        };
        // Signed out by the check: the next pass reports it and stops.
        if signed_in {
            tokio::time::sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_box_art_becomes_a_template() {
        assert_eq!(
            box_art_template("https://static-cdn.jtvnw.net/ttv-boxart/33214-52x72.jpg"),
            "https://static-cdn.jtvnw.net/ttv-boxart/33214-{width}x{height}.jpg"
        );
        assert_eq!(
            box_art_template("https://static-cdn.jtvnw.net/ttv-boxart/509658-{width}x{height}.jpg"),
            "https://static-cdn.jtvnw.net/ttv-boxart/509658-{width}x{height}.jpg"
        );
        assert_eq!(
            box_art_template("https://static-cdn.jtvnw.net/ttv-boxart/Just%20Chatting-x.jpg"),
            "https://static-cdn.jtvnw.net/ttv-boxart/Just%20Chatting-x.jpg"
        );
    }

    #[test]
    fn stream_from_helix() {
        let v = json!({
            "user_id": "1", "user_login": "speedrun_rafa", "user_name": "Speedrun_Rafa",
            "game_id": "1234", "game_name": "Hades II", "title": "Any% WR attempts",
            "viewer_count": 12400, "started_at": "2026-10-23T18:00:00Z",
            "thumbnail_url": "https://x/live_user_speedrun_rafa-{width}x{height}.jpg",
            "tags": ["Português", "Speedrun"], "is_mature": false
        });
        let s = parse_stream(&v);
        assert_eq!(s.login, "speedrun_rafa");
        assert_eq!(s.viewers, 12400);
        assert_eq!(s.tags, vec!["Português", "Speedrun"]);
        assert!(s.thumbnail.contains("{width}"));
        let none = parse_stream(&json!({ "tags": null }));
        assert!(none.tags.is_empty());
    }
}
