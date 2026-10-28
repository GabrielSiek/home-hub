//! Google Photos through the Picker API. Sign-in is OAuth with PKCE from a "Desktop app"
//! client, answered on this PC like Spotify's: Google's device flow (a code to type on the
//! phone) doesn't allow the Photos scope. Then each pick is a session: its link is shown as a
//! QR code, the photos are chosen on the phone, and once Google says they're set the launcher
//! downloads them. Google's links to them only last an hour, so they're kept on disk.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use futures_util::{stream, StreamExt};
use qrcode::render::svg;
use qrcode::QrCode;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tokio::net::TcpListener;

use super::{emit, new_id, rfc3339_ms, state, Album, Pick, Saved, Source, State, ITEMS_FILE};
use crate::error::{Error, Result};
use crate::history::now_ms;
use crate::launcher;
use tvl_kit::oauth::{challenge, loopback, random_string};
use crate::state::AppState;

pub const TOKEN_FILE: &str = "google.json";
const REDIRECT_PORT: u16 = 47952;
/// Desktop app clients take any port on 127.0.0.1 without listing it in Google Cloud.
const REDIRECT_URL: &str = "http://127.0.0.1:47952/google";
/// Running-app id of the sign-in page while it is open.
pub const SIGN_IN_APP: &str = "google-signin";
/// Running-app id of Google Photos opened on this PC, to pick there instead of on the phone.
pub const PICK_APP: &str = "google-photos-pick";
const AUTH: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/photospicker.mediaitems.readonly";
const API: &str = "https://photospicker.googleapis.com/v1";
/// Big enough for a 4K TV; Google keeps the aspect ratio.
const SIZE: &str = "=w3840-h2160";
const PARALLEL: usize = 6;
/// When Google doesn't say how long a pick may take.
const PICK_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    access: String,
    refresh: String,
    /// When `access` expires, unix ms.
    expires: i64,
}

impl Token {
    pub fn access(&self) -> &str {
        &self.access
    }

    /// Good for at least another minute.
    pub fn fresh(&self) -> bool {
        self.expires - now_ms() > 60_000
    }
}

impl State {
    fn set_token(&self, token: Option<Token>) -> Result<()> {
        match &token {
            Some(t) => fs::write(&self.token_path, serde_json::to_string_pretty(t)?)?,
            None if self.token_path.exists() => fs::remove_file(&self.token_path)?,
            None => {}
        }
        let mut inner = self.inner.lock().unwrap();
        inner.token = token;
        inner.error = None;
        Ok(())
    }

    fn cancelled(&self, gen: u64) -> bool {
        self.pick_gen.load(Ordering::SeqCst) != gen
    }
}

/// Shows how pick number `gen` is going, unless a newer one took over.
fn set_pick(app: &AppHandle, gen: u64, pick: Option<Pick>) {
    let st = state(app);
    {
        let mut inner = st.inner.lock().unwrap();
        if st.cancelled(gen) {
            return;
        }
        inner.pick = pick;
    }
    emit(app);
}

fn set_error(app: &AppHandle, error: &Error) {
    state(app).inner.lock().unwrap().error = Some(error.to_string());
    emit(app);
}

fn client(app: &AppHandle) -> Result<(String, String)> {
    app.state::<AppState>()
        .config
        .read()
        .unwrap()
        .settings
        .google_client()
        .ok_or_else(|| Error::msg("Add the Google Client ID and secret first, then Save"))
}

fn http(app: &AppHandle) -> reqwest::Client {
    app.state::<AppState>().http.clone()
}

// ---------- Sign-in ----------

pub async fn sign_in(app: &AppHandle) -> Result<()> {
    let token = consent(app, SCOPE).await?;
    state(app).set_token(Some(token))?;
    emit(app);
    Ok(())
}

/// Opens Google's consent page for `scope` in the launcher's browser and waits for it to
/// redirect back to this PC with the authorization code. YouTube's sign-in uses it too.
pub async fn consent(app: &AppHandle, scope: &str) -> Result<Token> {
    let (client_id, secret) = client(app)?;
    let listener = TcpListener::bind(("127.0.0.1", REDIRECT_PORT))
        .await
        .map_err(|e| Error::msg(format!("Port {REDIRECT_PORT} for the Google sign-in is busy: {e}")))?;

    let verifier = random_string(64);
    let expected_state = random_string(16);
    let enc = |s: &str| urlencoding::encode(s).into_owned();
    // offline + consent: a refresh token every time, so the TV never has to sign in again.
    let url = format!(
        "{AUTH}?response_type=code&client_id={}&scope={}&redirect_uri={}&code_challenge_method=S256\
         &code_challenge={}&state={expected_state}&access_type=offline&prompt=consent",
        enc(&client_id),
        enc(scope),
        enc(REDIRECT_URL),
        challenge(&verifier),
    );
    launcher::open_page(app, &url, SIGN_IN_APP)?;
    let result = loopback::wait_for_redirect(&listener, "/google", "Google", || launcher::is_running(app, SIGN_IN_APP), |q| {
        q.get("state") == Some(&expected_state) && (q.contains_key("code") || q.contains_key("error"))
    })
    .await;
    launcher::close_if(app, SIGN_IN_APP);
    let q = result?;
    if let Some(e) = q.get("error") {
        return Err(Error::msg(if e == "access_denied" {
            "Google sign-in cancelled".to_string()
        } else {
            format!("Google sign-in failed: {e}")
        }));
    }

    let form = [
        ("grant_type", "authorization_code"),
        ("code", q["code"].as_str()),
        ("redirect_uri", REDIRECT_URL),
        ("client_id", client_id.as_str()),
        ("client_secret", secret.as_str()),
        ("code_verifier", verifier.as_str()),
    ];
    let t = token_request(app, &form)
        .await?
        .map_err(|why| Error::msg(format!("Google sign-in: {why}")))?;
    let refresh = t
        .refresh_token
        .ok_or_else(|| Error::msg("Google sign-in: no refresh token came back; try again"))?;
    Ok(Token {
        access: t.access_token,
        refresh,
        expires: now_ms() + t.expires_in * 1000,
    })
}

pub fn sign_out(app: &AppHandle) -> Result<()> {
    state(app).set_token(None)?;
    emit(app);
    Ok(())
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
}

/// Ok(Err(why)) when Google turns the request down (a 4xx), e.g. a revoked refresh token.
async fn token_request(
    app: &AppHandle,
    form: &[(&str, &str)],
) -> Result<std::result::Result<TokenResponse, String>> {
    let r = http(app).post(TOKEN_URL).form(form).send().await?;
    let status = r.status();
    if status.is_success() {
        return Ok(Ok(r.json().await?));
    }
    if !status.is_client_error() {
        return Err(Error::msg(format!("Google accounts: {status}")));
    }
    let body: Value = r.json().await.unwrap_or_default();
    let why = body["error_description"].as_str().or(body["error"].as_str()).unwrap_or("");
    Ok(Err(format!("{why} ({status})")))
}

/// A valid access token, refreshed when it's about to expire.
async fn access_token(app: &AppHandle) -> Result<String> {
    let st = state(app);
    let current = || st.inner.lock().unwrap().token.clone().ok_or_else(|| Error::msg("Not signed in to Google"));
    let tok = current()?;
    if tok.expires - now_ms() > 60_000 {
        return Ok(tok.access);
    }
    let _one = st.refreshing.lock().await;
    let tok = current()?;
    // Refreshed by another download while this one waited.
    if tok.expires - now_ms() > 60_000 {
        return Ok(tok.access);
    }
    let t = match renew(app, &tok).await? {
        Ok(t) => t,
        Err(why) => {
            st.set_token(None)?;
            emit(app);
            return Err(Error::msg(format!("Google sign-in expired ({why}): sign in again")));
        }
    };
    let access = t.access.clone();
    st.set_token(Some(t))?;
    Ok(access)
}

/// A new access token for `tok`. Ok(Err(why)) when Google won't refresh it: revoked, a new
/// client, or a consent screen still in Testing (its tokens last a week).
pub async fn renew(app: &AppHandle, tok: &Token) -> Result<std::result::Result<Token, String>> {
    let (client_id, secret) = client(app)?;
    let form = [
        ("grant_type", "refresh_token"),
        ("refresh_token", tok.refresh.as_str()),
        ("client_id", client_id.as_str()),
        ("client_secret", secret.as_str()),
    ];
    Ok(token_request(app, &form).await?.map(|t| Token {
        access: t.access_token,
        refresh: t.refresh_token.unwrap_or_else(|| tok.refresh.clone()),
        expires: now_ms() + t.expires_in * 1000,
    }))
}

/// The response, or Google's reason for refusing it (e.g. the Picker API not enabled).
async fn ok(r: reqwest::Response) -> Result<reqwest::Response> {
    let status = r.status();
    if status.is_success() {
        return Ok(r);
    }
    let body: Value = r.json().await.unwrap_or_default();
    Err(Error::msg(match body["error"]["message"].as_str() {
        Some(m) if !m.is_empty() => format!("Google Photos: {m}"),
        _ => format!("Google Photos: {status}"),
    }))
}

/// A successful reply read as `T`. When it isn't what was expected, says which reply it was
/// and logs it, instead of reqwest's bare "error decoding response body".
async fn json<T: DeserializeOwned>(r: reqwest::Response, what: &str) -> Result<T> {
    let text = r.text().await?;
    serde_json::from_str(&text).map_err(|e| {
        eprintln!("[photos] unexpected {what} from Google: {e}\n{}", text.chars().take(2000).collect::<String>());
        Error::msg(format!("Google Photos sent an unexpected {what} ({e})"))
    })
}

// ---------- Picking ----------

/// A new picking session.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Session {
    id: String,
    picker_uri: String,
    polling_config: Option<Polling>,
}

/// A session checked while waiting. Only the flag is read: Google may leave fields out once
/// the photos are chosen (the picker link stops working after Done).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionStatus {
    #[serde(default)]
    media_items_set: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Polling {
    poll_interval: Option<String>,
    timeout_in: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MediaItem {
    #[serde(rename = "type", default)]
    kind: String,
    create_time: Option<String>,
    /// None hasn't been seen; such an item is skipped rather than failing the pick.
    media_file: Option<MediaFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MediaFile {
    base_url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MediaPage {
    #[serde(default)]
    media_items: Vec<MediaItem>,
    next_page_token: Option<String>,
}

/// A protobuf Duration as JSON gives it, e.g. "5s" or "1.5s".
fn parse_duration(s: &str) -> Option<Duration> {
    let secs: f64 = s.strip_suffix('s')?.parse().ok()?;
    (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs))
}

/// `text` as a QR code for the phone to scan, in inline SVG: a pick's link, a meeting's.
pub fn qr_svg(text: &str) -> Result<String> {
    let code = QrCode::new(text.as_bytes()).map_err(|e| Error::msg(format!("QR code: {e}")))?;
    let svg = code
        .render::<svg::Color>()
        .min_dimensions(360, 360)
        .dark_color(svg::Color("#0c0c14"))
        .light_color(svg::Color("#ffffff"))
        .build();
    // Drop the XML declaration: the SVG goes inside the page's HTML.
    Ok(svg.find("<svg").map(|i| svg[i..].to_string()).unwrap_or(svg))
}

/// Shows a QR code for choosing photos on the phone and saves the ones chosen as an album:
/// a new one, or `album` again with what was chosen this time. Signs in first if needed.
pub async fn pick(app: &AppHandle, album: Option<String>) -> Result<Album> {
    let st = state(app);
    let gen = st.pick_gen.fetch_add(1, Ordering::SeqCst) + 1;
    if st.inner.lock().unwrap().token.is_none() {
        sign_in(app).await?;
    }
    let result = run_pick(app, gen, album).await;
    set_pick(app, gen, None);
    match &result {
        Err(e) if !st.cancelled(gen) => set_error(app, e),
        _ => {}
    }
    result
}

/// Stops the pick in progress. Photos already saved for it are thrown away.
pub fn cancel_pick(app: &AppHandle) {
    let st = state(app);
    st.pick_gen.fetch_add(1, Ordering::SeqCst);
    st.inner.lock().unwrap().pick = None;
    launcher::close_if(app, PICK_APP);
    emit(app);
}

/// Opens the pick in progress in the launcher's browser, to choose photos on this PC instead.
pub fn pick_here(app: &AppHandle) -> Result<()> {
    let uri = match &state(app).inner.lock().unwrap().pick {
        Some(Pick::Choosing { uri, .. }) => uri.clone(),
        _ => return Err(Error::msg("Nothing is being picked")),
    };
    // /autoclose: Google Photos closes its page after Done.
    launcher::open_page(app, &format!("{uri}/autoclose"), PICK_APP)
}

async fn run_pick(app: &AppHandle, gen: u64, album: Option<String>) -> Result<Album> {
    let token = access_token(app).await?;
    let session: Session = json(
        ok(http(app).post(format!("{API}/sessions")).bearer_auth(&token).json(&json!({})).send().await?).await?,
        "new session",
    )
    .await?;
    let result = choose_and_save(app, gen, &session, album).await;
    launcher::close_if(app, PICK_APP);
    // The photos are saved (or not wanted): Google can let the session go.
    if let Ok(token) = access_token(app).await {
        let _ = http(app).delete(format!("{API}/sessions/{}", session.id)).bearer_auth(token).send().await;
    }
    result
}

/// Waits `d`, or less when the pick is cancelled meanwhile.
async fn wait(st: &State, gen: u64, d: Duration) -> Result<()> {
    let until = Instant::now() + d;
    while Instant::now() < until {
        if st.cancelled(gen) {
            return Err(Error::msg("Cancelled"));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Ok(())
}

async fn choose_and_save(app: &AppHandle, gen: u64, session: &Session, album: Option<String>) -> Result<Album> {
    let st = state(app);
    set_pick(
        app,
        gen,
        Some(Pick::Choosing {
            uri: session.picker_uri.clone(),
            qr: qr_svg(&session.picker_uri)?,
        }),
    );
    let polling = session.polling_config.as_ref();
    let interval = polling
        .and_then(|p| p.poll_interval.as_deref())
        .and_then(parse_duration)
        .unwrap_or(Duration::from_secs(5))
        .clamp(Duration::from_secs(2), Duration::from_secs(30));
    let deadline = Instant::now()
        + polling
            .and_then(|p| p.timeout_in.as_deref())
            .and_then(parse_duration)
            .unwrap_or(PICK_TIMEOUT);
    loop {
        wait(st, gen, interval).await?;
        if Instant::now() > deadline {
            return Err(Error::msg("Nothing was picked in time; try again"));
        }
        let s: SessionStatus = json(
            ok(http(app)
                .get(format!("{API}/sessions/{}", session.id))
                .bearer_auth(access_token(app).await?)
                .send()
                .await?)
            .await?,
            "session status",
        )
        .await?;
        if s.media_items_set {
            break;
        }
    }
    launcher::close_if(app, PICK_APP);

    let mut items = Vec::new();
    let mut page_token: Option<String> = None;
    loop {
        let mut query = vec![("sessionId", session.id.clone()), ("pageSize", "100".into())];
        if let Some(t) = page_token.take() {
            query.push(("pageToken", t));
        }
        let page: MediaPage = json(
            ok(http(app)
                .get(format!("{API}/mediaItems"))
                .bearer_auth(access_token(app).await?)
                .query(&query)
                .send()
                .await?)
            .await?,
            "list of photos",
        )
        .await?;
        items.extend(page.media_items);
        match page.next_page_token {
            Some(t) if !t.is_empty() => page_token = Some(t),
            _ => break,
        }
    }
    items.retain(|i| i.kind != "VIDEO" && i.media_file.is_some());
    if items.is_empty() {
        return Err(Error::msg("No photos were picked (videos don't show on the screensaver)"));
    }

    // Saved next to the album first, so a failed or cancelled pick leaves the old one whole.
    let id = album.clone().unwrap_or_else(new_id);
    fs::create_dir_all(&st.photos_dir)?;
    let part = st.photos_dir.join(format!("{id}.part"));
    if part.exists() {
        fs::remove_dir_all(&part)?;
    }
    fs::create_dir_all(&part)?;

    let total = items.len();
    set_pick(app, gen, Some(Pick::Saving { done: 0, total }));
    let mut saved = Vec::new();
    let mut failed = 0;
    let mut last_error = None;
    {
        let mut downloads = stream::iter(items.into_iter().enumerate())
            .map(|(i, item)| download(app.clone(), part.clone(), i, item))
            .buffer_unordered(PARALLEL);
        while let Some(r) = downloads.next().await {
            if st.cancelled(gen) {
                break;
            }
            match r {
                Ok(s) => saved.push(s),
                Err(e) => {
                    failed += 1;
                    last_error = Some(e);
                }
            }
            set_pick(app, gen, Some(Pick::Saving { done: saved.len() + failed, total }));
        }
    }
    if st.cancelled(gen) {
        let _ = fs::remove_dir_all(&part);
        return Err(Error::msg("Cancelled"));
    }
    if saved.is_empty() {
        let _ = fs::remove_dir_all(&part);
        return Err(last_error.unwrap_or_else(|| Error::msg("Couldn't save the photos")));
    }
    saved.sort_by(|a, b| a.file.cmp(&b.file));
    fs::write(part.join(ITEMS_FILE), serde_json::to_string(&saved)?)?;

    let dir = st.photos_dir.join(&id);
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::rename(&part, &dir)?;

    let count = saved.len();
    let album = st.update_albums(|albums| {
        if let Some(a) = albums.iter_mut().find(|a| a.id == id) {
            a.count = count;
            a.updated = now_ms();
            return Ok(a.clone());
        }
        let google = albums.iter().filter(|a| a.source == Source::Google).count();
        let a = Album {
            id: id.clone(),
            name: if google == 0 { "Google Photos".into() } else { format!("Google Photos {}", google + 1) },
            source: Source::Google,
            path: dir.clone(),
            count,
            enabled: true,
            updated: now_ms(),
        };
        albums.push(a.clone());
        Ok(a)
    })?;
    st.inner.lock().unwrap().error = None;
    emit(app);
    Ok(album)
}

/// Saves photo number `i`. Owns what it needs, so the downloads can run side by side.
async fn download(app: AppHandle, dir: PathBuf, i: usize, item: MediaItem) -> Result<Saved> {
    let base_url = item.media_file.map(|f| f.base_url).unwrap_or_default();
    let r = ok(http(&app)
        .get(format!("{base_url}{SIZE}"))
        .bearer_auth(access_token(&app).await?)
        // The shared client gives up after 10 s; a 4K photo on a slow line takes longer.
        .timeout(Duration::from_secs(90))
        .send()
        .await?)
    .await?;
    let ext = match r.headers().get("content-type").and_then(|v| v.to_str().ok()) {
        Some("image/png") => "png",
        Some("image/webp") => "webp",
        Some("image/gif") => "gif",
        // Sized copies of iPhone photos should come back as JPEG; the webview can't show HEIC.
        Some(t) if t.contains("heic") || t.contains("heif") => {
            return Err(Error::msg("Google Photos sent a HEIC photo, which the screensaver can't show"));
        }
        _ => "jpg",
    };
    let file = format!("{i:05}.{ext}");
    let bytes = r.bytes().await?;
    tokio::fs::write(dir.join(&file), &bytes).await?;
    Ok(Saved {
        file,
        taken: item.create_time.as_deref().and_then(rfc3339_ms),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_from_google() {
        assert_eq!(parse_duration("5s"), Some(Duration::from_secs(5)));
        assert_eq!(parse_duration("1.5s"), Some(Duration::from_millis(1500)));
        assert_eq!(parse_duration("5"), None);
    }

    #[test]
    fn session_from_json() {
        let s: Session = serde_json::from_value(json!({
            "id": "abc",
            "pickerUri": "https://photos.google.com/picker/abc",
            "pollingConfig": { "pollInterval": "3s", "timeoutIn": "1799.5s" },
            "expireTime": "2026-10-23T20:00:00Z"
        }))
        .unwrap();
        assert_eq!(s.picker_uri, "https://photos.google.com/picker/abc");
        assert_eq!(s.polling_config.unwrap().poll_interval.as_deref(), Some("3s"));
    }

    #[test]
    fn finished_session_without_picker_link() {
        let s: SessionStatus = serde_json::from_value(json!({ "id": "abc", "mediaItemsSet": true })).unwrap();
        assert!(s.media_items_set);
        let waiting: SessionStatus = serde_json::from_value(json!({ "id": "abc" })).unwrap();
        assert!(!waiting.media_items_set);
    }

    #[test]
    fn photo_list_reads_items_without_a_file() {
        let page: MediaPage = serde_json::from_value(json!({
            "mediaItems": [
                { "id": "1", "type": "PHOTO", "createTime": "2025-06-01T10:00:00Z",
                  "mediaFile": { "baseUrl": "https://lh3.googleusercontent.com/x", "mimeType": "image/heif", "filename": "IMG_1.HEIC" } },
                { "id": "2", "type": "PHOTO" }
            ]
        }))
        .unwrap();
        assert_eq!(page.media_items.len(), 2);
        assert!(page.media_items[0].media_file.is_some());
        assert!(page.media_items[1].media_file.is_none());
        assert!(page.next_page_token.is_none());
    }

    #[test]
    fn qr_is_inline_svg() {
        let svg = qr_svg("https://photos.google.com/picker/abc").unwrap();
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("#0c0c14"));
    }
}
