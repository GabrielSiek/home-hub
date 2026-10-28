//! Photos for the screensaver: albums picked in Google Photos and folders on this PC.
//!
//! Google's Library API can't read a user's albums any more (March 2025), so Google albums
//! come through the Picker API: the TV shows a QR code, the photos are chosen on the phone,
//! and the launcher downloads them once into `photos\<album id>\`. Picking again refreshes an
//! album. Folders are read again each time the screensaver starts, so new files show up.

mod folder;
mod google;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

pub use google::{cancel_pick, consent, pick, pick_here, renew, sign_in, sign_out, Token};

use crate::error::{Error, Result};
use crate::history::now_ms;
use tvl_protocol::ServiceLink as Link;
use crate::state::AppState;

const INDEX_FILE: &str = "albums.json";
/// Downloaded Google albums, one folder each.
const PHOTOS_DIR: &str = "photos";
/// A Google album's photos, in its folder.
const ITEMS_FILE: &str = "photos.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Google,
    Folder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    pub id: String,
    pub name: String,
    pub source: Source,
    /// The folder picked, or the album's download folder.
    pub path: PathBuf,
    pub count: usize,
    /// Shown by the screensaver.
    pub enabled: bool,
    /// When added or last picked again, unix ms.
    pub updated: i64,
}

/// One photo for the slideshow.
#[derive(Debug, Clone, Serialize)]
pub struct Photo {
    pub path: PathBuf,
    /// The album's name.
    pub album: String,
    /// When it was taken, unix ms. Google albums only.
    pub taken: Option<i64>,
}

/// A downloaded photo, as listed in a Google album's `photos.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Saved {
    file: String,
    taken: Option<i64>,
}

/// A Google Photos pick in progress.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum Pick {
    /// Waiting for photos to be chosen at `uri`; `qr` is that link as an SVG QR code.
    Choosing { uri: String, qr: String },
    /// Saving the chosen photos to this PC.
    Saving { done: usize, total: usize },
}

/// Everything Settings shows about photos.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub albums: Vec<Album>,
    pub google: Link,
    pub pick: Option<Pick>,
}

#[derive(Default)]
struct Inner {
    albums: Vec<Album>,
    token: Option<google::Token>,
    /// The last Google failure, until something works again.
    error: Option<String>,
    pick: Option<Pick>,
}

pub struct State {
    index_path: PathBuf,
    photos_dir: PathBuf,
    token_path: PathBuf,
    inner: Mutex<Inner>,
    /// Bumped to cancel the pick in progress.
    pick_gen: AtomicU64,
    /// One token refresh at a time.
    refreshing: tokio::sync::Mutex<()>,
}

impl State {
    pub fn new(data_dir: &Path) -> Self {
        let index_path = data_dir.join(INDEX_FILE);
        let token_path = data_dir.join(google::TOKEN_FILE);
        let read = |p: &Path| fs::read_to_string(p).ok();
        Self {
            inner: Mutex::new(Inner {
                albums: read(&index_path).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
                token: read(&token_path).and_then(|s| serde_json::from_str(&s).ok()),
                ..Default::default()
            }),
            index_path,
            photos_dir: data_dir.join(PHOTOS_DIR),
            token_path,
            pick_gen: AtomicU64::new(0),
            refreshing: tokio::sync::Mutex::new(()),
        }
    }

    fn save_albums(&self, albums: &[Album]) -> Result<()> {
        fs::write(&self.index_path, serde_json::to_string_pretty(albums)?)?;
        Ok(())
    }

    /// Changes the albums and writes them to disk.
    fn update_albums<T>(&self, f: impl FnOnce(&mut Vec<Album>) -> Result<T>) -> Result<T> {
        let mut inner = self.inner.lock().unwrap();
        let out = f(&mut inner.albums)?;
        self.save_albums(&inner.albums)?;
        Ok(out)
    }
}

fn state(app: &AppHandle) -> &State {
    &app.state::<AppState>().inner().photos
}

pub fn snapshot(app: &AppHandle) -> Snapshot {
    let configured = app.state::<AppState>().config.read().unwrap().settings.google_client().is_some();
    let inner = state(app).inner.lock().unwrap();
    let signed_in = inner.token.is_some();
    Snapshot {
        albums: inner.albums.clone(),
        google: Link {
            configured,
            signed_in,
            connected: signed_in && inner.error.is_none(),
            error: inner.error.clone(),
        },
        pick: inner.pick.clone(),
    }
}

fn emit(app: &AppHandle) {
    let _ = app.emit("photos", snapshot(app));
}

/// Lets the webview load photos from the folders added (the data folder is always allowed).
pub fn start(app: &AppHandle) {
    let folders: Vec<PathBuf> = state(app)
        .inner
        .lock()
        .unwrap()
        .albums
        .iter()
        .filter(|a| a.source == Source::Folder)
        .map(|a| a.path.clone())
        .collect();
    for f in folders {
        let _ = app.asset_protocol_scope().allow_directory(f, true);
    }
}

/// A short random id, safe as a folder name.
fn new_id() -> String {
    let mut bytes = [0u8; 6];
    getrandom::fill(&mut bytes).expect("the OS random source works");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn add_folder(app: &AppHandle, path: &str) -> Result<Album> {
    let path = PathBuf::from(path.trim().trim_matches('"'));
    if !path.is_dir() {
        return Err(Error::msg(format!("No folder at {}", path.display())));
    }
    let count = folder::scan(&path).len();
    if count == 0 {
        return Err(Error::msg("No photos in that folder (JPEG, PNG, WebP, AVIF or GIF)"));
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let album = state(app).update_albums(|albums| {
        if albums.iter().any(|a| a.source == Source::Folder && a.path == path) {
            return Err(Error::msg("That folder is already added"));
        }
        let album = Album {
            id: new_id(),
            name,
            source: Source::Folder,
            path: path.clone(),
            count,
            enabled: true,
            updated: now_ms(),
        };
        albums.push(album.clone());
        Ok(album)
    })?;
    let _ = app.asset_protocol_scope().allow_directory(&path, true);
    emit(app);
    Ok(album)
}

pub fn set_enabled(app: &AppHandle, id: &str, enabled: bool) -> Result<()> {
    state(app).update_albums(|albums| {
        let a = albums.iter_mut().find(|a| a.id == id).ok_or_else(|| Error::msg("No such album"))?;
        a.enabled = enabled;
        Ok(())
    })?;
    emit(app);
    Ok(())
}

/// Forgets an album. A Google album's downloaded photos are deleted; a folder is left as it is.
pub fn remove(app: &AppHandle, id: &str) -> Result<()> {
    let st = state(app);
    let album = st.update_albums(|albums| {
        let i = albums.iter().position(|a| a.id == id).ok_or_else(|| Error::msg("No such album"))?;
        Ok(albums.remove(i))
    })?;
    // Only ever a folder the launcher made itself.
    if album.source == Source::Google && album.path.starts_with(&st.photos_dir) && album.path.is_dir() {
        fs::remove_dir_all(&album.path)?;
    }
    emit(app);
    Ok(())
}

/// Every photo of the albums switched on. Folders are read again, so their counts stay current.
pub async fn slideshow(app: &AppHandle) -> Result<Vec<Photo>> {
    let albums: Vec<Album> = state(app).inner.lock().unwrap().albums.iter().filter(|a| a.enabled).cloned().collect();
    let lists = tauri::async_runtime::spawn_blocking(move || {
        albums.into_iter().map(|a| { let photos = list(&a); (a.id, photos) }).collect::<Vec<_>>()
    })
    .await
    .map_err(|e| Error::msg(e.to_string()))?;

    let changed = state(app).update_albums(|albums| {
        let mut changed = false;
        for (id, photos) in &lists {
            if let Some(a) = albums.iter_mut().find(|a| &a.id == id && a.count != photos.len()) {
                a.count = photos.len();
                changed = true;
            }
        }
        Ok(changed)
    })?;
    if changed {
        emit(app);
    }
    Ok(lists.into_iter().flat_map(|(_, photos)| photos).collect())
}

fn list(album: &Album) -> Vec<Photo> {
    let photo = |path: PathBuf, taken: Option<i64>| Photo {
        path,
        album: album.name.clone(),
        taken,
    };
    match album.source {
        Source::Folder => folder::scan(&album.path).into_iter().map(|p| photo(p, None)).collect(),
        Source::Google => fs::read_to_string(album.path.join(ITEMS_FILE))
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<Saved>>(&s).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|s| (album.path.join(&s.file), s.taken))
            .filter(|(p, _)| p.is_file())
            .map(|(p, t)| photo(p, t))
            .collect(),
    }
}

/// Unix ms from an RFC 3339 time such as `2024-05-01T12:34:56.789Z` or `…+02:00`.
pub(crate) fn rfc3339_ms(s: &str) -> Option<i64> {
    let (date, rest) = s.split_once(['T', 't', ' '])?;
    let mut d = date.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (y, mo, da) = (d.next()??, d.next()??, d.next()??);
    let zone_at = rest.find(['Z', 'z', '+', '-']).unwrap_or(rest.len());
    let (time, zone) = rest.split_at(zone_at);
    let mut t = time.splitn(3, ':');
    let h: i64 = t.next()?.parse().ok()?;
    let mi: i64 = t.next()?.parse().ok()?;
    let sec: f64 = t.next().unwrap_or("0").parse().ok()?;
    let offset_min = match zone {
        "" | "Z" | "z" => 0,
        z => {
            let sign = if z.starts_with('-') { -1 } else { 1 };
            let (zh, zm) = z[1..].split_once(':')?;
            sign * (zh.parse::<i64>().ok()? * 60 + zm.parse::<i64>().ok()?)
        }
    };
    let secs = days_from_civil(y, mo, da) * 86_400 + h * 3600 + mi * 60 - offset_min * 60;
    Some(secs * 1000 + (sec * 1000.0).round() as i64)
}

/// Days since 1970-01-01 for a date in the proleptic Gregorian calendar (H. Hinnant).
pub(crate) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_times() {
        assert_eq!(rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(rfc3339_ms("2000-01-01T00:00:00Z"), Some(946_684_800_000));
        assert_eq!(rfc3339_ms("2000-01-01T02:00:00+02:00"), Some(946_684_800_000));
        assert_eq!(rfc3339_ms("1999-12-31T21:00:00.250-03:00"), Some(946_684_800_250));
        assert_eq!(rfc3339_ms("2024-02-29T12:00:00.123456789Z"), Some(1_709_208_000_123));
        assert_eq!(rfc3339_ms("not a time"), None);
    }

    #[test]
    fn google_album_lists_its_saved_photos() {
        let dir = std::env::temp_dir().join(format!("tvl-album-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("00000.jpg"), b"x").unwrap();
        let saved = vec![
            Saved { file: "00000.jpg".into(), taken: Some(5) },
            Saved { file: "00001.jpg".into(), taken: None }, // deleted since
        ];
        fs::write(dir.join(ITEMS_FILE), serde_json::to_string(&saved).unwrap()).unwrap();
        let album = Album {
            id: "a".into(),
            name: "Trip".into(),
            source: Source::Google,
            path: dir.clone(),
            count: 2,
            enabled: true,
            updated: 0,
        };
        let photos = list(&album);
        assert_eq!(photos.len(), 1);
        assert_eq!(photos[0].album, "Trip");
        assert_eq!(photos[0].taken, Some(5));
        fs::remove_dir_all(&dir).unwrap();
    }
}
