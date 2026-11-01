//! Spotify through the Web API: sign-in (OAuth with PKCE, so no client secret), the player state
//! polled in the background, and control. The server plays nothing itself: every Connect device on
//! the account is a player (the Spotify app on a TV PC, phones, Echos), and this module only tells
//! Spotify which one. So the music is followed with every TV PC off.

mod library;
mod pages;
mod plays;
mod popular;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Notify;

pub use library::{playlist, search, shelf, Item, Playlist, SearchResults};
use library::play_body;
pub use pages::{artist, follow, oldest_episodes, show, Artist, Episode, Show};
pub use plays::TopTrack;
pub use popular::popular;

use tvl_kit::oauth::{challenge, query_of, random_string};
use tvl_kit::time::now_ms;
use tvl_kit::{Error, Result};
use tvl_protocol::{ScreenKind, ServiceLink as Link};

use crate::ctx::Ctx;
use crate::router::Caller;

const TOKEN_FILE: &str = "spotify.json";
const REDIRECT_PORT: u16 = 47951;
const REDIRECT_PATH: &str = "/spotify";
/// Has to be listed as a Redirect URI of the Spotify app (developer.spotify.com).
pub const REDIRECT_URL: &str = "http://127.0.0.1:47951/spotify";
const ACCOUNTS: &str = "https://accounts.spotify.com";
const API: &str = "https://api.spotify.com/v1";
const SCOPES: &str = "user-read-playback-state user-modify-playback-state user-read-currently-playing \
     user-read-recently-played playlist-read-private playlist-read-collaborative user-library-read \
     user-library-modify user-follow-read user-follow-modify user-top-read user-read-playback-position";
/// How long a player gets to show up as a device after starting it.
const START_WAIT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Token {
    access: String,
    refresh: String,
    /// When `access` expires, unix ms.
    expires: i64,
    /// The account's user id, for the Liked Songs context.
    user: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    Off,
    Context,
    Track,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Track {
    pub uri: String,
    pub title: String,
    /// The artists, or the show for a podcast episode.
    pub artist: String,
    pub album: String,
    pub year: Option<String>,
    pub duration_ms: u64,
    pub image: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Device {
    pub name: String,
    /// Spotify's device type: Computer, Smartphone, Speaker…
    pub kind: String,
    pub volume: Option<u8>,
    /// The Spotify app on a TV PC.
    pub local: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Player {
    pub playing: bool,
    pub progress_ms: u64,
    /// None when Spotify can't say (an ad, a local file it won't describe).
    pub track: Option<Track>,
    pub device: Option<Device>,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub context_uri: Option<String>,
    /// What it's playing from: a playlist, album or artist name.
    pub context: Option<String>,
    /// The track is in Liked Songs; None when unknown.
    pub liked: Option<bool>,
    pub next: Option<Track>,
}

/// Everything the UI shows about Spotify.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub link: Link,
    /// None: nothing is playing or paused on any device.
    pub player: Option<Player>,
}

/// A playback change asked for from the UI.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Control {
    /// Resume.
    Play,
    Pause,
    Next,
    Previous,
    Seek(u64),
    Shuffle(bool),
    Repeat(Repeat),
    Volume(u8),
    Like(bool),
    /// Move playback to the Spotify app on the caller's TV PC.
    Transfer,
}

/// The user's top songs of a time range, with their artists' URIs, and when fetched.
type TopSongs = (i64, Vec<(Vec<String>, TopTrack)>);

#[derive(Default)]
struct Inner {
    link: Link,
    token: Option<Token>,
    player: Option<Player>,
    /// Context names already looked up, by URI ("" when the lookup failed).
    names: HashMap<String, String>,
    /// Liked Songs membership by track URI; None when the check failed.
    liked: HashMap<String, Option<bool>>,
    /// Up next, for the track URI it was fetched for.
    next: Option<(String, Option<Track>)>,
    /// Recently played tiles for what was played from, by context URI.
    tiles: HashMap<String, Item>,
    /// The last snapshot sent, to send one only when something changed.
    sent: Option<Value>,
    /// A sign-in started from a phone: its state and PKCE verifier, until the address is pasted.
    waiting: Option<(String, String)>,
    top_tracks: HashMap<&'static str, TopSongs>,
    /// Everyone's most played songs by artist URI, and when fetched.
    popular: HashMap<String, (i64, Vec<TopTrack>)>,
}

pub struct State {
    token_path: PathBuf,
    inner: Mutex<Inner>,
    /// Bumped to stop the background task (new Client ID, sign-in, sign-out).
    gen: AtomicU64,
    /// Polls right away, after a command.
    wake: Notify,
    /// One token refresh at a time: Spotify may hand out a new refresh token each time.
    refreshing: tokio::sync::Mutex<()>,
    /// Songs the user played, for the artist screen's play counts.
    plays: Mutex<plays::Log>,
}

impl State {
    pub fn new(data_dir: &Path) -> Self {
        let token_path = data_dir.join(TOKEN_FILE);
        let token = tvl_kit::store::load(&token_path);
        Self {
            token_path,
            inner: Mutex::new(Inner {
                token,
                ..Default::default()
            }),
            gen: AtomicU64::new(0),
            wake: Notify::new(),
            refreshing: tokio::sync::Mutex::new(()),
            plays: Mutex::new(plays::Log::load(data_dir)),
        }
    }

    fn token(&self) -> Result<Token> {
        self.inner
            .lock()
            .token
            .clone()
            .ok_or_else(|| Error::msg("Not signed in to Spotify"))
    }

    fn set_token(&self, token: Option<Token>) -> Result<()> {
        match &token {
            Some(t) => tvl_kit::store::save(&self.token_path, t, true)?,
            None if self.token_path.exists() => std::fs::remove_file(&self.token_path)?,
            None => {}
        }
        self.inner.lock().token = token;
        Ok(())
    }
}

fn state(app: &Ctx) -> &State {
    &app.spotify
}

fn client_id(app: &Ctx) -> Option<String> {
    Some(app.settings.get().spotify_client_id).filter(|id| !id.is_empty())
}

fn http() -> reqwest::Client {
    tvl_kit::http::api()
}

pub fn snapshot(app: &Ctx) -> Snapshot {
    let inner = state(app).inner.lock();
    Snapshot {
        link: inner.link.clone(),
        player: inner.player.clone(),
    }
}

/// Tells every screen, only when something changed: a song playing moves on with each poll, which
/// keeps them in step; nothing playing says nothing.
fn emit(app: &Ctx) {
    let now = snapshot(app);
    let Ok(value) = serde_json::to_value(&now) else { return };
    {
        let mut inner = state(app).inner.lock();
        if inner.sent.as_ref() == Some(&value) {
            return;
        }
        inner.sent = Some(value);
    }
    app.bus.emit("spotify", &now);
}

// ---------- Sign-in ----------

/// Spotify's consent page, with what the answer has to come back with.
fn consent(client_id: &str) -> (String, String, String) {
    let verifier = random_string(64);
    let expected_state = random_string(16);
    let enc = |s: &str| urlencoding::encode(s).into_owned();
    let url = format!(
        "{ACCOUNTS}/authorize?response_type=code&client_id={}&scope={}&redirect_uri={}\
         &code_challenge_method=S256&code_challenge={}&state={expected_state}",
        enc(client_id),
        enc(SCOPES),
        enc(REDIRECT_URL),
        challenge(&verifier),
    );
    (url, verifier, expected_state)
}

/// Signs in through TV PC `device`: its browser opens the consent page and catches the redirect.
/// The profile is the one the Spotify web app uses, so usually Agree is all it takes.
pub async fn sign_in(app: &Ctx, device: &str) -> Result<()> {
    let client_id = client_id(app).ok_or_else(|| Error::msg("Add the Spotify Client ID first, then Save"))?;
    let (url, verifier, expected_state) = consent(&client_id);
    let args = json!({
        "url": url,
        "port": REDIRECT_PORT,
        "path": REDIRECT_PATH,
        "service": "Spotify",
        "state": expected_state,
    });
    let answer = app
        .agents
        .call(device, "oauth.loopback", args, None, 5 * 60_000, false)
        .await
        .map_err(|e| Error::msg(format!("The TV PC couldn't open the sign-in: {e:?}")))?;
    if answer["state"].as_str() != Some(expected_state.as_str()) {
        return Err(Error::msg("The sign-in answered for another request: try again"));
    }
    finish_sign_in(app, &client_id, answer["code"].as_str(), answer["error"].as_str(), &verifier).await
}

/// The consent page for a phone, with no TV PC to catch the redirect: its browser can't open the
/// 127.0.0.1 address Spotify sends it to, so that address comes back through `paste`.
pub fn sign_in_link(app: &Ctx) -> Result<String> {
    let client_id = client_id(app).ok_or_else(|| Error::msg("Add the Spotify Client ID first, then Save"))?;
    let (url, verifier, expected_state) = consent(&client_id);
    state(app).inner.lock().waiting = Some((expected_state, verifier));
    Ok(url)
}

/// Finishes the sign-in `sign_in_link` started, from the address the phone couldn't open.
pub async fn paste(app: &Ctx, address: &str) -> Result<()> {
    let q = query_of(address.trim());
    let client_id = client_id(app).ok_or_else(|| Error::msg("The Spotify Client ID isn't set"))?;
    let verifier = {
        let mut inner = state(app).inner.lock();
        match inner.waiting.take() {
            Some((expected, verifier)) if q.get("state") == Some(&expected) => verifier,
            waiting => {
                inner.waiting = waiting;
                return Err(Error::msg("That isn't the address of this sign-in: start it again"));
            }
        }
    };
    finish_sign_in(app, &client_id, q.get("code").map(String::as_str), q.get("error").map(String::as_str), &verifier).await
}

async fn finish_sign_in(app: &Ctx, client_id: &str, code: Option<&str>, error: Option<&str>, verifier: &str) -> Result<()> {
    if let Some(e) = error {
        return Err(Error::msg(if e == "access_denied" {
            "Spotify sign-in cancelled".to_string()
        } else {
            format!("Spotify sign-in failed: {e}")
        }));
    }
    let code = code.ok_or_else(|| Error::msg("Spotify sent no code: try again"))?;
    let form = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", REDIRECT_URL),
        ("client_id", client_id),
        ("code_verifier", verifier),
    ];
    let t = token_request(&form)
        .await?
        .map_err(|why| Error::msg(format!("Spotify sign-in: {why}")))?;
    let access = t.access_token.clone();
    let me: Value = http()
        .get(format!("{API}/me"))
        .bearer_auth(&access)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    state(app).set_token(Some(Token {
        access,
        refresh: t.refresh_token.unwrap_or_default(),
        expires: now_ms() + t.expires_in * 1000,
        user: me["id"].as_str().unwrap_or_default().to_string(),
    }))?;
    start(app);
    Ok(())
}

pub fn sign_out(app: &Ctx) -> Result<()> {
    state(app).set_token(None)?;
    start(app);
    Ok(())
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
}

/// Ok(Err(why)) when Spotify turns the request down (a 4xx), e.g. a revoked refresh token.
async fn token_request(form: &[(&str, &str)]) -> Result<std::result::Result<TokenResponse, String>> {
    let r = http().post(format!("{ACCOUNTS}/api/token")).form(form).send().await?;
    let status = r.status();
    if status.is_success() {
        return Ok(Ok(r.json().await?));
    }
    if !status.is_client_error() {
        return Err(Error::msg(format!("Spotify accounts: {status}")));
    }
    let body: Value = r.json().await.unwrap_or_default();
    let why = body["error_description"].as_str().or(body["error"].as_str()).unwrap_or("");
    Ok(Err(format!("{why} ({status})")))
}

/// A valid access token, refreshed when it's about to expire (or `force`d after a 401).
async fn access_token(app: &Ctx, force: bool) -> Result<String> {
    let s = state(app);
    let tok = s.token()?;
    if !force && tok.expires - now_ms() > 60_000 {
        return Ok(tok.access);
    }
    let stale = tok.access;
    let _one = s.refreshing.lock().await;
    let tok = s.token()?;
    // Refreshed by another call while this one waited.
    if tok.expires - now_ms() > 60_000 && (!force || tok.access != stale) {
        return Ok(tok.access);
    }
    let client_id = client_id(app).ok_or_else(|| Error::msg("The Spotify Client ID isn't set"))?;
    let form = [
        ("grant_type", "refresh_token"),
        ("refresh_token", tok.refresh.as_str()),
        ("client_id", client_id.as_str()),
    ];
    let t = match token_request(&form).await? {
        Ok(t) => t,
        // Revoked, or the Client ID changed: only a new sign-in helps.
        Err(why) => {
            s.set_token(None)?;
            return Err(Error::msg(format!("Spotify sign-in expired ({why}): sign in again in Settings")));
        }
    };
    let access = t.access_token.clone();
    s.set_token(Some(Token {
        access: t.access_token,
        refresh: t.refresh_token.unwrap_or(tok.refresh),
        expires: now_ms() + t.expires_in * 1000,
        user: tok.user,
    }))?;
    Ok(access)
}

// ---------- Web API ----------

/// A failed call, with Spotify's reason when it gave one (e.g. NO_ACTIVE_DEVICE).
struct ApiError {
    status: StatusCode,
    reason: String,
    message: String,
    /// Seconds to wait, on 429.
    retry_after: Option<u64>,
}

impl From<ApiError> for Error {
    fn from(e: ApiError) -> Self {
        match e.status {
            StatusCode::FORBIDDEN if e.message.to_lowercase().contains("premium") => {
                Error::msg("Spotify: controlling playback needs Spotify Premium")
            }
            // A sign-in from before the launcher asked for this (following, top artists).
            StatusCode::FORBIDDEN if e.message.to_lowercase().contains("scope") => {
                Error::msg("Spotify needs a new sign-in for this: sign out and in again in Settings")
            }
            StatusCode::TOO_MANY_REQUESTS => Error::msg("Spotify: too many requests, trying again shortly"),
            _ if e.message.is_empty() => Error::msg(format!("Spotify: {}", e.status)),
            _ => Error::msg(format!("Spotify: {}", e.message)),
        }
    }
}

/// One Web API call. Ok(None) for an empty reply (204, or a command's 200 with no body).
async fn call(
    app: &Ctx,
    method: Method,
    path: &str,
    query: &[(&str, String)],
    body: Option<&Value>,
) -> Result<std::result::Result<Option<Value>, ApiError>> {
    let mut force = false;
    loop {
        let at = access_token(app, force).await?;
        let mut req = http()
            .request(method.clone(), format!("{API}{path}"))
            .bearer_auth(&at)
            .query(query);
        req = match body {
            Some(b) => req.json(b),
            // Spotify wants a length on bodiless PUTs and POSTs.
            None if method != Method::GET => req.header("Content-Length", "0"),
            None => req,
        };
        let r = req.send().await?;
        let status = r.status();
        if status == StatusCode::UNAUTHORIZED && !force {
            force = true;
            continue;
        }
        let retry_after = r
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok());
        let text = r.text().await?;
        let json: Option<Value> = serde_json::from_str(&text).ok();
        if status.is_success() {
            return Ok(Ok(json));
        }
        let err = json.as_ref().map(|j| &j["error"]);
        return Ok(Err(ApiError {
            status,
            reason: err.and_then(|e| e["reason"].as_str()).unwrap_or_default().to_string(),
            message: err
                .and_then(|e| e["message"].as_str().or(e.as_str()))
                .unwrap_or_default()
                .to_string(),
            retry_after,
        }));
    }
}

/// A GET whose reply is needed.
async fn get(app: &Ctx, path: &str, query: &[(&str, String)]) -> Result<Value> {
    Ok(call(app, Method::GET, path, query, None).await??.unwrap_or_default())
}

/// A player command. With nothing active to play on, it's sent to the Spotify app on the caller's
/// TV PC, started if need be.
async fn command(app: &Ctx, caller: &Caller, method: Method, path: &str, query: &[(&str, String)], body: Option<&Value>) -> Result<()> {
    match call(app, method.clone(), path, query, body).await? {
        Ok(_) => Ok(()),
        Err(e) if e.status == StatusCode::NOT_FOUND && e.reason == "NO_ACTIVE_DEVICE" => {
            let device = callers_device(app, caller).await?;
            let mut query = query.to_vec();
            query.push(("device_id", device));
            call(app, method, path, &query, body).await??;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

// ---------- The TV PCs' Spotify apps ----------

/// The TV PC whose Spotify app a Connect device is: the app names its device after the computer,
/// and the TV PC says that name when it connects.
fn tv_of(app: &Ctx, name: &str, kind: &str) -> Option<String> {
    if !kind.eq_ignore_ascii_case("computer") {
        return None;
    }
    let tvs = app.devices.list();
    tvs.into_iter()
        .find(|s| s.kind == ScreenKind::Tv && s.caps.as_ref().and_then(|c| c.spotify_device.as_deref()).is_some_and(|d| d.eq_ignore_ascii_case(name.trim())))
        .map(|s| s.id)
}

async fn find_tv_device(app: &Ctx, tv: &str) -> Result<Option<String>> {
    let v = get(app, "/me/player/devices", &[]).await?;
    Ok(v["devices"].as_array().into_iter().flatten().find_map(|d| {
        let here = tv_of(app, d["name"].as_str()?, d["type"].as_str()?).is_some_and(|id| id == tv) && !d["is_restricted"].as_bool().unwrap_or(false);
        here.then(|| d["id"].as_str().map(str::to_string)).flatten()
    }))
}

/// The device id of the Spotify app on TV PC `tv`, which starts it (minimized) when it isn't
/// running and comes back in front once it shows up.
async fn tv_device(app: &Ctx, tv: &str) -> Result<String> {
    if let Some(id) = find_tv_device(app, tv).await? {
        return Ok(id);
    }
    let off = |e| Error::msg(format!("The TV PC couldn't start Spotify: {e:?}"));
    app.agents.call(tv, "spotify.start", json!({}), None, 10_000, false).await.map_err(off)?;
    let deadline = tokio::time::Instant::now() + START_WAIT;
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if let Some(id) = find_tv_device(app, tv).await? {
            let _ = app.agents.call(tv, "spotify.started", json!({}), None, 5_000, false).await;
            return Ok(id);
        }
    }
    Err(Error::msg("Spotify didn't show up on the TV PC. Open the Spotify app once and sign in to the same account"))
}

/// The TV PC the caller means: its own, or the one it controls while that's on.
fn callers_tv(app: &Ctx, caller: &Caller) -> Option<String> {
    caller.agent.clone().or_else(|| app.devices.target(caller.screen.as_deref(), caller.target.as_deref()).ok())
}

async fn callers_device(app: &Ctx, caller: &Caller) -> Result<String> {
    let tv = callers_tv(app, caller).ok_or_else(|| Error::msg("Nothing to play on: choose a device"))?;
    tv_device(app, &tv).await
}

// ---------- Player state ----------

fn names(v: &Value) -> String {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["name"].as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A track or podcast episode from the Web API.
fn parse_track(v: &Value) -> Option<Track> {
    let episode = v["type"].as_str() == Some("episode");
    let (artist, album_v) = if episode {
        (v["show"]["name"].as_str().unwrap_or_default().to_string(), &v["show"])
    } else {
        (names(&v["artists"]), &v["album"])
    };
    let images = if episode && v["images"].is_array() { &v["images"] } else { &album_v["images"] };
    Some(Track {
        uri: v["uri"].as_str()?.to_string(),
        title: v["name"].as_str().unwrap_or_default().to_string(),
        artist,
        album: album_v["name"].as_str().unwrap_or_default().to_string(),
        year: album_v["release_date"].as_str().and_then(|d| d.get(..4)).map(str::to_string),
        duration_ms: v["duration_ms"].as_u64().unwrap_or(0),
        image: library::image(images, 640),
    })
}

fn parse_player(v: &Value) -> Player {
    let d = &v["device"];
    Player {
        playing: v["is_playing"].as_bool().unwrap_or(false),
        progress_ms: v["progress_ms"].as_u64().unwrap_or(0),
        track: parse_track(&v["item"]),
        device: d["name"].as_str().map(|name| {
            let kind = d["type"].as_str().unwrap_or_default();
            Device {
                name: name.to_string(),
                kind: kind.to_string(),
                volume: d["volume_percent"].as_u64().map(|p| p.min(100) as u8),
                local: false,
            }
        }),
        shuffle: v["shuffle_state"].as_bool().unwrap_or(false),
        repeat: match v["repeat_state"].as_str() {
            Some("context") => Repeat::Context,
            Some("track") => Repeat::Track,
            _ => Repeat::Off,
        },
        context_uri: v["context"]["uri"].as_str().map(str::to_string),
        context: None,
        liked: None,
        next: None,
    }
}

/// The name of a playlist, album, artist or show being played from; cached.
async fn context_name(app: &Ctx, uri: &str, track: Option<&Track>) -> Option<String> {
    if uri.ends_with(":collection") {
        return Some("Liked songs".into());
    }
    if let Some(n) = state(app).inner.lock().names.get(uri) {
        return Some(n.clone()).filter(|n| !n.is_empty());
    }
    let mut parts = uri.split(':').skip(1);
    let (kind, id) = (parts.next()?, parts.next()?);
    let name = match kind {
        "album" if track.is_some_and(|t| !t.album.is_empty()) => track.map(|t| t.album.clone()),
        "playlist" | "album" | "artist" | "show" => {
            let query = [("fields", "name".to_string())];
            let query: &[(&str, String)] = if kind == "playlist" { &query } else { &[] };
            get(app, &format!("/{kind}s/{id}"), query)
                .await
                .ok()
                .and_then(|v| v["name"].as_str().map(str::to_string))
        }
        _ => None,
    };
    state(app)
        .inner
        .lock()
        .names
        .insert(uri.to_string(), name.clone().unwrap_or_default());
    name
}

/// Whether the track is in Liked Songs; cached per track.
async fn liked(app: &Ctx, uri: &str) -> Option<bool> {
    if let Some(l) = state(app).inner.lock().liked.get(uri) {
        return *l;
    }
    let l = get(app, "/me/library/contains", &[("uris", uri.to_string())])
        .await
        .ok()
        .and_then(|v| v[0].as_bool());
    let mut inner = state(app).inner.lock();
    if inner.liked.len() > 500 {
        inner.liked.clear();
    }
    inner.liked.insert(uri.to_string(), l);
    l
}

/// The first track in the queue after `uri`; fetched again when the track changes.
async fn up_next(app: &Ctx, uri: &str) -> Option<Track> {
    if let Some((for_uri, next)) = &state(app).inner.lock().next {
        if for_uri == uri {
            return next.clone();
        }
    }
    let next = get(app, "/me/player/queue", &[]).await.ok().and_then(|v| parse_track(&v["queue"][0]));
    state(app).inner.lock().next = Some((uri.to_string(), next.clone()));
    next
}

/// Fetches the player state. A 429 comes back as the seconds to wait.
async fn poll(app: &Ctx) -> Result<std::result::Result<(), u64>> {
    let query = [("additional_types", "episode".to_string())];
    let v = match call(app, Method::GET, "/me/player", &query, None).await? {
        Ok(v) => v,
        Err(e) if e.status == StatusCode::TOO_MANY_REQUESTS => return Ok(Err(e.retry_after.unwrap_or(30))),
        Err(e) => return Err(e.into()),
    };
    let mut player = v.as_ref().map(parse_player);
    if let Some(p) = &mut player {
        if let Some(d) = &mut p.device {
            d.local = tv_of(app, &d.name, &d.kind).is_some();
        }
        if let Some(uri) = p.context_uri.clone() {
            p.context = context_name(app, &uri, p.track.as_ref()).await;
        }
        if let Some(uri) = p.track.as_ref().map(|t| t.uri.clone()) {
            p.liked = liked(app, &uri).await;
            p.next = up_next(app, &uri).await;
        }
    }
    state(app).inner.lock().player = player;
    Ok(Ok(()))
}

// ---------- Background task ----------

/// (Re)starts the background task, e.g. after a sign-in or a new Client ID.
pub fn start(app: &Ctx) {
    let gen = state(app).gen.fetch_add(1, Ordering::SeqCst) + 1;
    tokio::spawn(run(app.clone(), gen));
}

fn alive(app: &Ctx, gen: u64) -> bool {
    state(app).gen.load(Ordering::SeqCst) == gen && !app.shutdown.is_cancelled()
}

/// How long until the next poll: often while a screen is open to show it, rarely with none, when
/// it only keeps the plays counted.
fn interval(app: &Ctx) -> Duration {
    if !app.devices.list().iter().any(|s| s.online) {
        return Duration::from_secs(30);
    }
    let playing = state(app).inner.lock().player.as_ref().is_some_and(|p| p.playing);
    Duration::from_secs(if playing { 3 } else { 6 })
}

async fn run(app: Ctx, gen: u64) {
    let mut pause = Duration::from_secs(5);
    loop {
        if !alive(&app, gen) {
            return;
        }
        let configured = client_id(&app).is_some();
        let signed_in = state(&app).inner.lock().token.is_some();
        if !configured || !signed_in {
            {
                let mut inner = state(&app).inner.lock();
                inner.player = None;
                inner.link = Link {
                    configured,
                    signed_in,
                    ..Link::default()
                };
            }
            emit(&app);
            return;
        }

        let result = poll(&app).await;
        if !alive(&app, gen) {
            return;
        }
        if result.is_ok() {
            let app = app.clone();
            tokio::spawn(async move {
                let _ = plays::sync(&app, plays::SYNC_IDLE).await;
            });
        }
        let wait = {
            let mut inner = state(&app).inner.lock();
            let signed_in = inner.token.is_some();
            match result {
                Ok(Ok(())) => {
                    inner.link = Link {
                        configured,
                        signed_in,
                        connected: true,
                        error: None,
                    };
                    pause = Duration::from_secs(5);
                    None
                }
                Ok(Err(secs)) => Some(Duration::from_secs(secs.clamp(1, 600))),
                Err(e) => {
                    inner.link = Link {
                        configured,
                        signed_in,
                        connected: false,
                        error: Some(e.to_string()),
                    };
                    let p = pause;
                    pause = (pause * 2).min(Duration::from_secs(120));
                    Some(p)
                }
            }
        };
        emit(&app);
        let wait = wait.unwrap_or_else(|| interval(&app));
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            // Spotify takes a moment to reflect a command.
            _ = state(&app).wake.notified() => tokio::time::sleep(Duration::from_millis(700)).await,
            _ = app.shutdown.cancelled() => return,
        }
    }
}

/// After a command: show the expected result at once, and poll soon for the real one.
fn after_command(app: &Ctx, f: impl FnOnce(&mut Player)) {
    {
        let mut inner = state(app).inner.lock();
        if let Some(p) = inner.player.as_mut() {
            f(p);
        }
        // The queue changes with skips and shuffle.
        inner.next = None;
    }
    emit(app);
    state(app).wake.notify_one();
}

// ---------- Commands ----------

pub async fn control(app: &Ctx, caller: &Caller, c: Control) -> Result<()> {
    let q = |k: &'static str, v: String| vec![(k, v)];
    match c {
        Control::Play => command(app, caller, Method::PUT, "/me/player/play", &[], None).await?,
        Control::Pause => command(app, caller, Method::PUT, "/me/player/pause", &[], None).await?,
        Control::Next => command(app, caller, Method::POST, "/me/player/next", &[], None).await?,
        Control::Previous => command(app, caller, Method::POST, "/me/player/previous", &[], None).await?,
        Control::Seek(ms) => command(app, caller, Method::PUT, "/me/player/seek", &q("position_ms", ms.to_string()), None).await?,
        Control::Shuffle(on) => command(app, caller, Method::PUT, "/me/player/shuffle", &q("state", on.to_string()), None).await?,
        Control::Repeat(r) => {
            let r = serde_json::to_value(r)?.as_str().unwrap_or("off").to_string();
            command(app, caller, Method::PUT, "/me/player/repeat", &q("state", r), None).await?
        }
        Control::Volume(v) => {
            command(app, caller, Method::PUT, "/me/player/volume", &q("volume_percent", v.min(100).to_string()), None).await?
        }
        Control::Like(on) => {
            let uri = state(app)
                .inner
                .lock()
                .player
                .as_ref()
                .and_then(|p| p.track.as_ref())
                .map(|t| t.uri.clone())
                .ok_or_else(|| Error::msg("Nothing is playing"))?;
            let method = if on { Method::PUT } else { Method::DELETE };
            call(app, method, "/me/library", &q("uris", uri.clone()), None).await??;
            state(app).inner.lock().liked.insert(uri, Some(on));
        }
        Control::Transfer => {
            let device = callers_device(app, caller).await?;
            let body = json!({ "device_ids": [device], "play": true });
            call(app, Method::PUT, "/me/player", &[], Some(&body)).await??;
        }
    }
    after_command(app, |p| match c {
        Control::Play => p.playing = true,
        Control::Pause => p.playing = false,
        Control::Seek(ms) => p.progress_ms = ms,
        Control::Shuffle(on) => p.shuffle = on,
        Control::Repeat(r) => p.repeat = r,
        Control::Volume(v) => {
            if let Some(d) = p.device.as_mut() {
                d.volume = Some(v.min(100));
            }
        }
        Control::Like(on) => p.liked = Some(on),
        Control::Next | Control::Previous | Control::Transfer => {}
    });
    Ok(())
}

/// A Spotify Connect device the music can move to, for the companion's device picker.
#[derive(Debug, Clone, Serialize)]
pub struct ConnectDevice {
    /// Spotify's device id; or `tv:<id>` for a TV PC whose app isn't running, or `cast:<id>` for a
    /// smart TV, which moving the music there starts.
    pub id: String,
    pub name: String,
    /// Spotify's device type: Computer, Smartphone, Tablet, Speaker…
    pub kind: String,
    pub active: bool,
    /// The Spotify app on the TV PC the caller controls.
    pub local: bool,
    pub volume: Option<u8>,
}

const TV_DEVICE: &str = "tv:";
const CAST_DEVICE: &str = "cast:";

/// The Connect devices signed in to the account (Echos, and phones and tablets while their Spotify
/// app is open), the TV PCs that are on, and the smart TVs on the network.
pub async fn devices(app: &Ctx, caller: &Caller) -> Result<Vec<ConnectDevice>> {
    let mine = callers_tv(app, caller);
    let v = get(app, "/me/player/devices", &[]).await?;
    let mut list: Vec<ConnectDevice> = v["devices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|d| !d["is_restricted"].as_bool().unwrap_or(false))
        .filter_map(|d| {
            let name = d["name"].as_str()?.to_string();
            let kind = d["type"].as_str().unwrap_or_default().to_string();
            Some(ConnectDevice {
                id: d["id"].as_str()?.to_string(),
                local: mine.is_some() && tv_of(app, &name, &kind) == mine,
                active: d["is_active"].as_bool().unwrap_or(false),
                volume: d["volume_percent"].as_u64().map(|n| n.min(100) as u8),
                name,
                kind,
            })
        })
        .collect();
    for tv in app.devices.list().into_iter().filter(|s| s.kind == ScreenKind::Tv && s.online) {
        let Some(named) = tv.caps.as_ref().and_then(|c| c.spotify_device.clone()) else { continue };
        if !list.iter().any(|d| d.name.eq_ignore_ascii_case(named.trim())) {
            list.insert(0, ConnectDevice {
                local: mine.as_deref() == Some(tv.id.as_str()),
                id: format!("{TV_DEVICE}{}", tv.id),
                name: tv.name,
                kind: "Computer".into(),
                active: false,
                volume: None,
            });
        }
    }
    for tv in crate::players::list(app) {
        if !list.iter().any(|d| d.name.eq_ignore_ascii_case(&tv.name)) {
            list.push(ConnectDevice {
                id: format!("{CAST_DEVICE}{}", tv.id),
                name: tv.name,
                kind: "TV".into(),
                active: false,
                local: false,
                volume: None,
            });
        }
    }
    Ok(list)
}

/// Spotify's id for a device of the picker, starting what has to run first.
async fn spotify_id(app: &Ctx, id: &str) -> Result<String> {
    if let Some(tv) = id.strip_prefix(TV_DEVICE) {
        return tv_device(app, tv).await;
    }
    match id.strip_prefix(CAST_DEVICE) {
        Some(tv) => cast_device(app, tv).await,
        None => Ok(id.to_string()),
    }
}

fn connect_devices(v: &Value) -> impl Iterator<Item = (&str, &str)> {
    let list = v["devices"].as_array().into_iter().flatten();
    list.filter_map(|d| Some((d["id"].as_str()?, d["name"].as_str()?)))
}

/// Opens Spotify on smart TV `id` and waits for its app to join the account's devices: under the
/// TV's own name, or failing that as the one device that wasn't there before.
async fn cast_device(app: &Ctx, id: &str) -> Result<String> {
    let name = crate::players::list(app).into_iter().find(|p| p.id == id).map(|p| p.name).unwrap_or_default();
    let before: Vec<String> = connect_devices(&get(app, "/me/player/devices", &[]).await?).map(|(id, _)| id.to_string()).collect();
    crate::players::open_spotify(app, id).await?;
    let deadline = tokio::time::Instant::now() + START_WAIT;
    while tokio::time::Instant::now() < deadline {
        let v = get(app, "/me/player/devices", &[]).await?;
        let found = connect_devices(&v)
            .find(|(_, n)| n.eq_ignore_ascii_case(&name))
            .or_else(|| connect_devices(&v).find(|(id, _)| !before.iter().any(|b| b == id)));
        if let Some((id, _)) = found {
            return Ok(id.to_string());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err(Error::msg("Spotify opened on the TV but didn't join: check it's signed in to the same account there"))
}

/// Moves playback to device `id` and keeps it playing.
pub async fn transfer(app: &Ctx, id: &str) -> Result<()> {
    let id = spotify_id(app, id).await?;
    let body = json!({ "device_ids": [id], "play": true });
    call(app, Method::PUT, "/me/player", &[], Some(&body)).await??;
    after_command(app, |_| {});
    Ok(())
}

/// What plays after the current song: Spotify's queue, then its autoplay.
pub async fn queue(app: &Ctx) -> Result<Vec<Track>> {
    let v = get(app, "/me/player/queue", &[]).await?;
    Ok(v["queue"].as_array().into_iter().flatten().filter_map(parse_track).take(30).collect())
}

/// Where something asked to play goes: the device named; for a TV PC, its own Spotify app; else
/// whatever is playing now, so a phone can start a playlist on the Echo already in use.
async fn destination(app: &Ctx, caller: &Caller, device: Option<&str>) -> Result<Vec<(&'static str, String)>> {
    let id = match device {
        Some(id) => Some(spotify_id(app, id).await?),
        None if caller.agent.is_some() => Some(callers_device(app, caller).await?),
        None => None,
    };
    Ok(id.map(|id| ("device_id", id)).into_iter().collect())
}

/// Plays `uri` on `device`: a playlist, album, artist, show or Liked Songs from the start, or a
/// track or episode, inside `context` when given (so the album or playlist carries on), from
/// `position_ms` in when given (an episode listened to halfway).
pub async fn play(app: &Ctx, caller: &Caller, uri: &str, context: Option<&str>, position_ms: Option<u64>, device: Option<&str>) -> Result<()> {
    let query = destination(app, caller, device).await?;
    let mut body = play_body(uri, context);
    if let Some(ms) = position_ms {
        body["position_ms"] = json!(ms);
    }
    command(app, caller, Method::PUT, "/me/player/play", &query, Some(&body)).await?;
    after_command(app, |p| p.playing = true);
    Ok(())
}

/// Plays songs one after another on `device`, from the first: e.g. the user's most played of an
/// artist.
pub async fn play_tracks(app: &Ctx, caller: &Caller, uris: &[String], device: Option<&str>) -> Result<()> {
    if uris.is_empty() {
        return Err(Error::msg("Nothing to play"));
    }
    let query = destination(app, caller, device).await?;
    let body = json!({ "uris": uris });
    command(app, caller, Method::PUT, "/me/player/play", &query, Some(&body)).await?;
    after_command(app, |p| p.playing = true);
    Ok(())
}

/// The Liked Songs context of the signed-in account.
fn liked_songs_uri(app: &Ctx) -> Result<String> {
    Ok(format!("spotify:user:{}:collection", state(app).token()?.user))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_from_web_api() {
        let v = json!({
            "is_playing": true,
            "progress_ms": 84000,
            "shuffle_state": true,
            "repeat_state": "context",
            "context": { "uri": "spotify:playlist:abc" },
            "device": { "name": "TEST-PC", "type": "Smartphone", "volume_percent": 70 },
            "item": {
                "type": "track",
                "uri": "spotify:track:1",
                "name": "The Less I Know the Better",
                "duration_ms": 216000,
                "artists": [{ "name": "Tame Impala" }],
                "album": {
                    "name": "Currents",
                    "release_date": "2015-07-17",
                    "images": [
                        { "url": "big", "width": 640, "height": 640 },
                        { "url": "small", "width": 64, "height": 64 }
                    ]
                }
            }
        });
        let p = parse_player(&v);
        assert!(p.playing && p.shuffle);
        assert_eq!(p.repeat, Repeat::Context);
        assert_eq!(p.context_uri.as_deref(), Some("spotify:playlist:abc"));
        let t = p.track.unwrap();
        assert_eq!(t.artist, "Tame Impala");
        assert_eq!(t.year.as_deref(), Some("2015"));
        assert_eq!(t.image.as_deref(), Some("big"));
        let d = p.device.unwrap();
        assert_eq!(d.volume, Some(70));
        assert!(!d.local, "only the poll, which knows the TV PCs, says so");
    }

    #[test]
    fn the_consent_page_asks_for_what_the_redirect_answers() {
        let (url, verifier, expected) = consent("my-id");
        assert!(url.starts_with("https://accounts.spotify.com/authorize?"));
        assert!(url.contains("client_id=my-id"));
        assert!(url.contains(&format!("state={expected}")));
        assert!(url.contains(&format!("code_challenge={}", challenge(&verifier))));
        assert!(!url.contains(&verifier), "the verifier stays on the server");
    }

    #[test]
    fn episode_shows_its_show() {
        let v = json!({
            "type": "episode",
            "uri": "spotify:episode:9",
            "name": "Ep 1",
            "duration_ms": 1000,
            "images": [{ "url": "ep", "width": 300, "height": 300 }],
            "show": { "name": "A Podcast", "images": [] }
        });
        let t = parse_track(&v).unwrap();
        assert_eq!(t.artist, "A Podcast");
        assert_eq!(t.image.as_deref(), Some("ep"));
    }

    #[test]
    fn control_json_shape() {
        let c: Control = serde_json::from_str(r#"{"type":"seek","value":5000}"#).unwrap();
        assert_eq!(c, Control::Seek(5000));
        let c: Control = serde_json::from_str(r#"{"type":"repeat","value":"track"}"#).unwrap();
        assert_eq!(c, Control::Repeat(Repeat::Track));
        let c: Control = serde_json::from_str(r#"{"type":"next"}"#).unwrap();
        assert_eq!(c, Control::Next);
    }
}
