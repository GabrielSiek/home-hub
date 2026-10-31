//! Radarr and Sonarr, which share an API (v3): what they have, what they're fetching, and asking
//! for more. Only the fields the hub uses are read; they keep adding others.

use std::time::Duration;

use reqwest::{Client, Method, Response};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use tvl_kit::{Error, Result};

/// A lookup asks their metadata service, which can take a while.
const SLOW: Duration = Duration::from_secs(25);

#[derive(Clone)]
pub struct Arr {
    name: &'static str,
    base: String,
    key: String,
    http: Client,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Image {
    pub cover_type: String,
    pub remote_url: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MediaFile {
    pub id: u64,
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Movie {
    pub id: u64,
    pub title: String,
    pub year: u32,
    pub overview: String,
    pub tmdb_id: u64,
    pub has_file: bool,
    pub images: Vec<Image>,
    pub movie_file: Option<MediaFile>,
    pub added: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Statistics {
    pub episode_file_count: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Series {
    pub id: u64,
    pub title: String,
    pub year: u32,
    pub overview: String,
    pub tmdb_id: u64,
    pub tvdb_id: u64,
    pub images: Vec<Image>,
    pub statistics: Statistics,
    pub added: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Episode {
    pub id: u64,
    pub season_number: u32,
    pub episode_number: u32,
    pub has_file: bool,
    pub monitored: bool,
    pub episode_file_id: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QueueMovie {
    pub tmdb_id: u64,
    pub title: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QueueEpisode {
    pub season_number: u32,
    pub episode_number: u32,
}

/// Something on its way.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QueueItem {
    pub movie_id: u64,
    pub series_id: u64,
    pub episode_id: u64,
    pub title: String,
    pub size: f64,
    pub sizeleft: f64,
    pub timeleft: Option<String>,
    pub status: String,
    pub error_message: Option<String>,
    pub download_id: Option<String>,
    pub movie: Option<QueueMovie>,
    pub series: Option<QueueMovie>,
    pub episode: Option<QueueEpisode>,
}

impl QueueItem {
    /// Downloaded share, 0..1.
    pub fn progress(&self) -> f64 {
        if self.size > 0.0 {
            ((self.size - self.sizeleft) / self.size).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub fn eta_s(&self) -> Option<u64> {
        self.timeleft.as_deref().and_then(parse_timeleft)
    }
}

/// "02:03:04" and "1.02:03:04" (days) to seconds.
pub fn parse_timeleft(text: &str) -> Option<u64> {
    let (days, clock) = match text.split_once('.') {
        Some((d, rest)) => (d.parse::<u64>().ok()?, rest),
        None => (0, text),
    };
    let mut parts = clock.split(':').map(|p| p.parse::<u64>().ok());
    let (h, m, s) = (parts.next()??, parts.next()??, parts.next()??);
    Some(days * 86_400 + h * 3600 + m * 60 + s)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Queue {
    records: Vec<QueueItem>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RootFolder {
    pub path: String,
    pub free_space: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct HistoryEvent {
    download_id: Option<String>,
}

/// The torrents named in a title's history, each once.
fn torrents_in(events: Vec<HistoryEvent>) -> Vec<String> {
    let mut hashes: Vec<String> = events.into_iter().filter_map(|e| e.download_id).filter(|h| !h.is_empty()).collect();
    hashes.sort();
    hashes.dedup();
    hashes
}

/// The profile called `wanted` (any case), else the first.
pub fn pick_profile<'a>(profiles: &'a [Profile], wanted: &str) -> Option<&'a Profile> {
    profiles.iter().find(|p| p.name.eq_ignore_ascii_case(wanted.trim())).or(profiles.first())
}

/// What they say went wrong: `{"message"}`, or `[{"errorMessage"}]` for a refused body.
fn their_error(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    let text = v.get("message").or_else(|| v.get(0).and_then(|e| e.get("errorMessage")))?.as_str()?;
    Some(tvl_kit::log::clip(text, 200).to_string())
}

impl Arr {
    /// None when it isn't set up.
    pub fn new(name: &'static str, url: &str, key: &str) -> Option<Arr> {
        let (url, key) = (url.trim().trim_end_matches('/'), key.trim());
        (!url.is_empty() && !key.is_empty()).then(|| Arr { name, base: url.to_string(), key: key.to_string(), http: tvl_kit::http::api() })
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    async fn send(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Response> {
        let mut req = self.http.request(method, format!("{}/api/v3/{path}", self.base)).header("X-Api-Key", &self.key).timeout(SLOW);
        if let Some(b) = body {
            req = req.json(b);
        }
        let res = req.send().await.map_err(|_| Error::msg(format!("{} isn't answering", self.name)))?;
        let status = res.status();
        if status.is_success() {
            return Ok(res);
        }
        if status.as_u16() == 401 {
            return Err(Error::msg(format!("{} refused its API key", self.name)));
        }
        let said = res.text().await.ok().and_then(|b| their_error(&b));
        Err(Error::msg(match said {
            Some(s) => format!("{}: {s}", self.name),
            None => format!("{} answered {status}", self.name),
        }))
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        Ok(self.send(Method::GET, path, None).await?.json().await?)
    }

    async fn post<T: DeserializeOwned>(&self, path: &str, body: &Value) -> Result<T> {
        Ok(self.send(Method::POST, path, Some(body)).await?.json().await?)
    }

    /// A command they run in the background (a search).
    async fn command(&self, body: Value) -> Result<()> {
        self.send(Method::POST, "command", Some(&body)).await.map(drop)
    }

    pub async fn root_folders(&self) -> Result<Vec<RootFolder>> {
        self.get("rootfolder").await
    }

    pub async fn profiles(&self) -> Result<Vec<Profile>> {
        self.get("qualityprofile").await
    }

    // ---------- Radarr ----------

    pub async fn movies(&self) -> Result<Vec<Movie>> {
        self.get("movie").await
    }

    pub async fn movie(&self, tmdb_id: u64) -> Result<Option<Movie>> {
        let found: Vec<Movie> = self.get(&format!("movie?tmdbId={tmdb_id}")).await?;
        Ok(found.into_iter().find(|m| m.tmdb_id == tmdb_id))
    }

    pub async fn movie_queue(&self) -> Result<Vec<QueueItem>> {
        Ok(self.get::<Queue>("queue?pageSize=500&includeMovie=true").await?.records)
    }

    /// Adds a movie and starts looking for it.
    pub async fn add_movie(&self, tmdb_id: u64, profile: u64, root: &str) -> Result<()> {
        let mut movie: Value = self.get(&format!("movie/lookup/tmdb?tmdbId={tmdb_id}")).await?;
        let Some(fields) = movie.as_object_mut() else {
            return Err(Error::msg("Radarr doesn't know that movie"));
        };
        fields.insert("qualityProfileId".into(), json!(profile));
        fields.insert("rootFolderPath".into(), json!(root));
        fields.insert("monitored".into(), json!(true));
        fields.insert("minimumAvailability".into(), json!("released"));
        fields.insert("addOptions".into(), json!({ "searchForMovie": true }));
        self.send(Method::POST, "movie", Some(&movie)).await.map(drop)
    }

    pub async fn search_movie(&self, id: u64) -> Result<()> {
        self.command(json!({ "name": "MoviesSearch", "movieIds": [id] })).await
    }

    /// A movie's torrents. Its history goes with it, so ask before deleting it.
    pub async fn movie_torrents(&self, id: u64) -> Result<Vec<String>> {
        Ok(torrents_in(self.get(&format!("history/movie?movieId={id}")).await?))
    }

    pub async fn delete_movie(&self, id: u64) -> Result<()> {
        self.send(Method::DELETE, &format!("movie/{id}?deleteFiles=true"), None).await.map(drop)
    }

    // ---------- Sonarr ----------

    pub async fn all_series(&self) -> Result<Vec<Series>> {
        self.get("series").await
    }

    pub async fn episodes(&self, series: u64) -> Result<Vec<Episode>> {
        self.get(&format!("episode?seriesId={series}")).await
    }

    pub async fn episode_files(&self, series: u64) -> Result<Vec<MediaFile>> {
        self.get(&format!("episodefile?seriesId={series}")).await
    }

    pub async fn series_queue(&self) -> Result<Vec<QueueItem>> {
        Ok(self.get::<Queue>("queue?pageSize=500&includeSeries=true&includeEpisode=true").await?.records)
    }

    /// Adds a series with nothing monitored: what to fetch is chosen episode by episode, so asking
    /// for one season doesn't fetch ten.
    pub async fn add_series(&self, tvdb_id: u64, profile: u64, root: &str) -> Result<Series> {
        let mut found: Vec<Value> = self.get(&format!("series/lookup?term=tvdb:{tvdb_id}")).await?;
        if found.is_empty() {
            return Err(Error::msg("Sonarr doesn't know that series"));
        }
        let mut series = found.swap_remove(0);
        let Some(fields) = series.as_object_mut() else {
            return Err(Error::msg("Sonarr doesn't know that series"));
        };
        fields.insert("qualityProfileId".into(), json!(profile));
        // Sonarr 3 needs one; 4 ignores it.
        fields.insert("languageProfileId".into(), json!(1));
        fields.insert("rootFolderPath".into(), json!(root));
        fields.insert("monitored".into(), json!(true));
        fields.insert("seasonFolder".into(), json!(true));
        fields.insert("addOptions".into(), json!({ "monitor": "none", "searchForMissingEpisodes": false }));
        self.post("series", &series).await
    }

    /// Monitors these episodes and starts looking for them.
    pub async fn fetch_episodes(&self, ids: &[u64]) -> Result<()> {
        self.send(Method::PUT, "episode/monitor", Some(&json!({ "episodeIds": ids, "monitored": true }))).await?;
        self.command(json!({ "name": "EpisodeSearch", "episodeIds": ids })).await
    }

    /// A series' torrents. Its history goes with it, so ask before deleting it.
    pub async fn series_torrents(&self, id: u64) -> Result<Vec<String>> {
        Ok(torrents_in(self.get(&format!("history/series?seriesId={id}")).await?))
    }

    pub async fn delete_series(&self, id: u64) -> Result<()> {
        self.send(Method::DELETE, &format!("series/{id}?deleteFiles=true"), None).await.map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_left_reads_with_and_without_days() {
        assert_eq!(parse_timeleft("00:12:34"), Some(754));
        assert_eq!(parse_timeleft("1.02:00:01"), Some(93_601));
        assert_eq!(parse_timeleft("soon"), None);
        assert_eq!(parse_timeleft("12:34"), None);
    }

    #[test]
    fn progress_is_what_is_no_longer_left() {
        let q = QueueItem { size: 200.0, sizeleft: 50.0, ..Default::default() };
        assert_eq!(q.progress(), 0.75);
        assert_eq!(QueueItem::default().progress(), 0.0);
    }

    #[test]
    fn the_profile_named_wins_and_the_first_stands_in() {
        let list = [Profile { id: 1, name: "Any".into() }, Profile { id: 4, name: "HD-1080p".into() }];
        assert_eq!(pick_profile(&list, "hd-1080p").unwrap().id, 4);
        assert_eq!(pick_profile(&list, "nope").unwrap().id, 1);
        assert!(pick_profile(&[], "x").is_none());
    }

    #[test]
    fn a_torrent_named_twice_in_the_history_counts_once() {
        let e = |id: Option<&str>| HistoryEvent { download_id: id.map(String::from) };
        assert_eq!(torrents_in(vec![e(Some("B")), e(None), e(Some("A")), e(Some("B")), e(Some(""))]), ["A", "B"]);
    }

    #[test]
    fn their_errors_read_in_both_shapes() {
        assert_eq!(their_error(r#"{"message":"Nope"}"#).as_deref(), Some("Nope"));
        assert_eq!(their_error(r#"[{"propertyName":"Path","errorMessage":"Already added"}]"#).as_deref(), Some("Already added"));
        assert_eq!(their_error("<html>"), None);
    }
}
