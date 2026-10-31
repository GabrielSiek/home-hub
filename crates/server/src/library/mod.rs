//! The house's own library: movies and series kept on the server's disks, for what the streaming
//! services don't have. Radarr and Sonarr find and organise them, qBittorrent fetches them; the hub
//! shows what's there and what's on its way, asks for more, and hands a TV PC the file to play.
//!
//! Titles go by their TMDB id, as everywhere else in the hub, so a title's page is the same whether
//! it comes from a streaming service or from here.

pub mod arr;
pub mod qbit;
pub mod stream;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tvl_kit::{Error, Result};

use crate::ctx::Ctx;
use arr::{Arr, Episode, Image, MediaFile, Movie, QueueItem, Series};
use qbit::{Qbit, Torrents};
use stream::Grants;

/// A new series' episodes appear as Sonarr reads it, a moment after it's added.
const EPISODES_WAIT: u32 = 20;

#[derive(Default)]
pub struct Library {
    pub grants: Grants,
    qbit_session: Mutex<Option<String>>,
}

/// What's set up right now, from the household's settings.
struct Services<'a> {
    radarr: Option<Arr>,
    sonarr: Option<Arr>,
    qbit: Option<Qbit<'a>>,
    quality: String,
}

fn services(ctx: &Ctx) -> Services<'_> {
    let h = ctx.settings.get();
    Services {
        radarr: Arr::new("Radarr", &h.radarr_url, &h.radarr_api_key),
        sonarr: Arr::new("Sonarr", &h.sonarr_url, &h.sonarr_api_key),
        qbit: Qbit::new(&h.qbittorrent_url, &h.qbittorrent_username, &h.qbittorrent_password, &ctx.library.qbit_session),
        quality: h.library_quality,
    }
}

impl Services<'_> {
    fn radarr(&self) -> Result<&Arr> {
        self.radarr.as_ref().ok_or_else(|| Error::msg("Radarr isn't set up: Settings › Movies & TV"))
    }

    fn sonarr(&self) -> Result<&Arr> {
        self.sonarr.as_ref().ok_or_else(|| Error::msg("Sonarr isn't set up: Settings › Movies & TV"))
    }

    /// A removed title's torrents: each is a second name for its files, so the disk isn't freed
    /// while they stay. Radarr and Sonarr have forgotten them by now. The title is gone either way.
    async fn drop_torrents(&self, hashes: Result<Vec<String>>) {
        let Some(qbit) = &self.qbit else { return };
        let done = match hashes {
            Ok(hashes) if hashes.is_empty() => return,
            Ok(hashes) => qbit.delete(&hashes.iter().map(|h| h.to_ascii_lowercase()).collect::<Vec<_>>()).await,
            Err(e) => Err(e),
        };
        if let Err(e) = done {
            tracing::warn!("library: a removed title's torrents stayed: {e}");
        }
    }

    /// Speeds are extra: without qBittorrent the queues still say how far things got.
    async fn torrents(&self) -> Torrents {
        match &self.qbit {
            Some(q) => q.torrents().await.unwrap_or_else(|e| {
                tracing::warn!("library: {e}");
                Torrents::default()
            }),
            None => Torrents::default(),
        }
    }
}

// ---------- What's there ----------

/// A title as the screens show one (`Title` in the TV's api.ts).
#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub id: u64,
    pub media_type: &'static str,
    pub title: String,
    pub year: Option<String>,
    pub overview: String,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    /// When it was added; newest first in the list.
    pub added: String,
}

fn art(images: &[Image], kind: &str) -> Option<String> {
    images.iter().find(|i| i.cover_type == kind).and_then(|i| i.remote_url.clone())
}

fn year(y: u32) -> Option<String> {
    (y > 0).then(|| y.to_string())
}

#[derive(Debug, Default, Serialize)]
pub struct Listing {
    /// Radarr or Sonarr is set up.
    pub configured: bool,
    pub titles: Vec<Item>,
    /// Space left where new titles go, in bytes.
    pub free_bytes: Option<u64>,
    /// What couldn't be read.
    pub problems: Vec<String>,
}

/// Everything that can be played.
pub async fn list(ctx: &Ctx) -> Listing {
    let s = services(ctx);
    let mut out = Listing { configured: s.radarr.is_some() || s.sonarr.is_some(), ..Default::default() };
    let (movies, series) = tokio::join!(
        async {
            match &s.radarr {
                Some(r) => r.movies().await.map(Some),
                None => Ok(None),
            }
        },
        async {
            match &s.sonarr {
                Some(r) => r.all_series().await.map(Some),
                None => Ok(None),
            }
        },
    );
    match movies {
        Ok(list) => out.titles.extend(list.into_iter().flatten().filter(|m| m.has_file && m.tmdb_id > 0).map(|m| Item {
            id: m.tmdb_id,
            media_type: "movie",
            year: year(m.year),
            poster: art(&m.images, "poster"),
            backdrop: art(&m.images, "fanart"),
            title: m.title,
            overview: m.overview,
            added: m.added,
        })),
        Err(e) => out.problems.push(e.to_string()),
    }
    match series {
        Ok(list) => out.titles.extend(list.into_iter().flatten().filter(|m| m.statistics.episode_file_count > 0 && m.tmdb_id > 0).map(|m| Item {
            id: m.tmdb_id,
            media_type: "tv",
            year: year(m.year),
            poster: art(&m.images, "poster"),
            backdrop: art(&m.images, "fanart"),
            title: m.title,
            overview: m.overview,
            added: m.added,
        })),
        Err(e) => out.problems.push(e.to_string()),
    }
    out.titles.sort_by(|a, b| b.added.cmp(&a.added));
    let radarr_free = async {
        match &s.radarr {
            Some(r) => r.root_folders().await.ok(),
            None => None,
        }
    };
    let sonarr_free = async {
        match &s.sonarr {
            Some(r) => r.root_folders().await.ok(),
            None => None,
        }
    };
    let (a, b) = tokio::join!(radarr_free, sonarr_free);
    out.free_bytes = a.into_iter().chain(b).flatten().map(|f| f.free_space).max();
    out
}

// ---------- A title ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// The library doesn't have it.
    Absent,
    /// Asked for; nothing found to fetch yet.
    Wanted,
    Downloading,
    Ready,
}

#[derive(Debug, Clone, Serialize)]
pub struct EpisodeState {
    pub state: State,
    pub progress: f64,
}

/// A title in the library. `ready` says something can be played, even with more on its way.
#[derive(Debug, Clone, Serialize)]
pub struct TitleState {
    pub configured: bool,
    pub state: State,
    pub ready: bool,
    pub progress: f64,
    pub speed: u64,
    pub eta_s: Option<u64>,
    /// The downloader's own words when something's wrong.
    pub note: Option<String>,
    /// A series' episodes, by "s1e3".
    pub episodes: BTreeMap<String, EpisodeState>,
}

impl TitleState {
    fn none(configured: bool) -> Self {
        Self { configured, state: State::Absent, ready: false, progress: 0.0, speed: 0, eta_s: None, note: None, episodes: BTreeMap::new() }
    }
}

fn episode_key(season: u32, episode: u32) -> String {
    format!("s{season}e{episode}")
}

/// Share done, speed, time left and any complaint of what's on its way for a title.
fn queue_totals(items: &[&QueueItem], torrents: &Torrents) -> (f64, u64, Option<u64>, Option<String>) {
    let live = |q: &QueueItem| q.download_id.as_ref().and_then(|h| torrents.get(&h.to_ascii_uppercase()));
    let size: f64 = items.iter().map(|q| q.size).sum();
    let left: f64 = items.iter().map(|q| q.sizeleft).sum();
    let progress = if size > 0.0 { ((size - left) / size).clamp(0.0, 1.0) } else { 0.0 };
    let speed = items.iter().filter_map(|q| live(q)).map(|t| t.dlspeed).sum();
    let eta = items.iter().filter_map(|q| live(q).and_then(|t| t.eta_s()).or_else(|| q.eta_s())).max();
    let note = items.iter().find_map(|q| q.error_message.clone().filter(|m| !m.is_empty()));
    (progress, speed, eta, note)
}

/// A movie's state, from what Radarr has and what's queued for it.
pub fn movie_state(movie: Option<&Movie>, queue: &[&QueueItem], torrents: &Torrents) -> TitleState {
    let mut t = TitleState::none(true);
    let Some(m) = movie else { return t };
    t.ready = m.has_file;
    t.state = if !queue.is_empty() {
        State::Downloading
    } else if m.has_file {
        State::Ready
    } else {
        State::Wanted
    };
    if m.has_file {
        t.progress = 1.0;
    }
    if !queue.is_empty() {
        (t.progress, t.speed, t.eta_s, t.note) = queue_totals(queue, torrents);
    }
    t
}

/// A series' state, from Sonarr's episodes and what's queued for it.
pub fn series_state(episodes: &[Episode], queue: &[&QueueItem], torrents: &Torrents) -> TitleState {
    let mut t = TitleState::none(true);
    for e in episodes {
        let queued = queue.iter().find(|q| q.episode_id == e.id);
        let state = match (e.has_file, queued, e.monitored) {
            (true, _, _) => State::Ready,
            (false, Some(_), _) => State::Downloading,
            (false, None, true) => State::Wanted,
            (false, None, false) => State::Absent,
        };
        if state != State::Absent {
            let progress = if e.has_file { 1.0 } else { queued.map_or(0.0, |q| q.progress()) };
            t.episodes.insert(episode_key(e.season_number, e.episode_number), EpisodeState { state, progress });
        }
    }
    t.ready = t.episodes.values().any(|e| e.state == State::Ready);
    t.state = if !queue.is_empty() {
        State::Downloading
    } else if t.episodes.values().any(|e| e.state == State::Wanted) {
        State::Wanted
    } else if t.ready {
        State::Ready
    } else {
        State::Absent
    };
    if !queue.is_empty() {
        (t.progress, t.speed, t.eta_s, t.note) = queue_totals(queue, torrents);
    }
    t
}

async fn find_series(sonarr: &Arr, tmdb_id: u64) -> Result<Option<Series>> {
    Ok(sonarr.all_series().await?.into_iter().find(|s| s.tmdb_id == tmdb_id))
}

/// A title's state in the library.
pub async fn title(ctx: &Ctx, media_type: &str, id: u64) -> Result<TitleState> {
    let s = services(ctx);
    if media_type == "movie" {
        let Some(radarr) = &s.radarr else { return Ok(TitleState::none(false)) };
        let (movie, queue, torrents) = tokio::join!(radarr.movie(id), radarr.movie_queue(), s.torrents());
        let (movie, queue) = (movie?, queue?);
        let mine: Vec<&QueueItem> = queue.iter().filter(|q| movie.as_ref().is_some_and(|m| m.id == q.movie_id)).collect();
        return Ok(movie_state(movie.as_ref(), &mine, &torrents));
    }
    let Some(sonarr) = &s.sonarr else { return Ok(TitleState::none(false)) };
    let Some(series) = find_series(sonarr, id).await? else { return Ok(TitleState::none(true)) };
    let (episodes, queue, torrents) = tokio::join!(sonarr.episodes(series.id), sonarr.series_queue(), s.torrents());
    let (episodes, queue) = (episodes?, queue?);
    let mine: Vec<&QueueItem> = queue.iter().filter(|q| q.series_id == series.id).collect();
    Ok(series_state(&episodes, &mine, &torrents))
}

// ---------- Asking for more ----------

/// The title's TVDB id, from TMDB: Sonarr goes by TVDB.
async fn tvdb_id(ctx: &Ctx, tmdb_id: u64) -> Result<u64> {
    let key = ctx.settings.get().tmdb_api_key;
    let key = key.trim();
    let url = format!("{}/tv/{tmdb_id}/external_ids", ctx.config.tmdb_url);
    // A v4 token is long and goes in a header; a v3 key is short and goes in the address.
    let req = if key.len() > 40 { tvl_kit::http::api().get(url).bearer_auth(key) } else { tvl_kit::http::api().get(url).query(&[("api_key", key)]) };
    let v: Value = req.send().await?.error_for_status()?.json().await?;
    v["tvdb_id"].as_u64().filter(|id| *id > 0).ok_or_else(|| Error::msg("TMDB has no TVDB id for that series, so Sonarr can't find it"))
}

/// Where new titles go: their first root folder, and the profile the household chose.
async fn destination(service: &Arr, quality: &str) -> Result<(u64, String)> {
    let (roots, profiles) = tokio::join!(service.root_folders(), service.profiles());
    let (roots, profiles) = (roots?, profiles?);
    let root = roots.into_iter().next().ok_or_else(|| Error::msg(format!("{} has no root folder yet", service.name())))?;
    let profile = arr::pick_profile(&profiles, quality).ok_or_else(|| Error::msg(format!("{} has no quality profile", service.name())))?;
    Ok((profile.id, root.path))
}

/// Which of a series' episodes to fetch: one, a season, or all its seasons (specials only when
/// asked for by number). What's already there isn't fetched again.
pub fn episodes_to_fetch(episodes: &[Episode], season: Option<u32>, episode: Option<u32>) -> Vec<u64> {
    episodes
        .iter()
        .filter(|e| !e.has_file)
        .filter(|e| match (season, episode) {
            (Some(s), Some(n)) => e.season_number == s && e.episode_number == n,
            (Some(s), None) => e.season_number == s,
            _ => e.season_number > 0,
        })
        .map(|e| e.id)
        .collect()
}

/// Asks for a movie, or for a series' episodes: one, a season, or everything. Asking again for
/// what isn't found searches again.
pub async fn request(ctx: &Ctx, media_type: &str, id: u64, season: Option<u32>, episode: Option<u32>) -> Result<TitleState> {
    let s = services(ctx);
    if media_type == "movie" {
        let radarr = s.radarr()?;
        match radarr.movie(id).await? {
            Some(m) => radarr.search_movie(m.id).await?,
            None => {
                let (profile, root) = destination(radarr, &s.quality).await?;
                radarr.add_movie(id, profile, &root).await?;
            }
        }
    } else {
        let sonarr = s.sonarr()?;
        let series = match find_series(sonarr, id).await? {
            Some(series) => series,
            None => {
                let tvdb = tvdb_id(ctx, id).await?;
                let (profile, root) = destination(sonarr, &s.quality).await?;
                sonarr.add_series(tvdb, profile, &root).await?
            }
        };
        let mut episodes = sonarr.episodes(series.id).await?;
        for _ in 0..EPISODES_WAIT {
            if !episodes.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            episodes = sonarr.episodes(series.id).await?;
        }
        if episodes.is_empty() {
            return Err(Error::msg("Sonarr is still reading that series: try again in a moment"));
        }
        let ids = episodes_to_fetch(&episodes, season, episode);
        if ids.is_empty() {
            return Err(Error::msg("Those episodes are already here"));
        }
        sonarr.fetch_episodes(&ids).await?;
    }
    title(ctx, media_type, id).await
}

/// Takes a title out of the library, its files and torrents too.
pub async fn remove(ctx: &Ctx, media_type: &str, id: u64) -> Result<()> {
    let s = services(ctx);
    if media_type == "movie" {
        let radarr = s.radarr()?;
        if let Some(m) = radarr.movie(id).await? {
            let torrents = radarr.movie_torrents(m.id).await;
            radarr.delete_movie(m.id).await?;
            s.drop_torrents(torrents).await;
        }
    } else {
        let sonarr = s.sonarr()?;
        if let Some(series) = find_series(sonarr, id).await? {
            let torrents = sonarr.series_torrents(series.id).await;
            sonarr.delete_series(series.id).await?;
            s.drop_torrents(torrents).await;
        }
    }
    Ok(())
}

// ---------- What's downloading ----------

#[derive(Debug, Clone, Serialize)]
pub struct Download {
    pub title: String,
    pub media_type: &'static str,
    /// TMDB id.
    pub id: u64,
    pub season: u32,
    pub episode: u32,
    pub progress: f64,
    pub speed: u64,
    pub eta_s: Option<u64>,
    pub paused: bool,
    /// Its torrent, to pause it. Several episodes can share one.
    pub hash: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct Downloads {
    pub items: Vec<Download>,
    pub speed: u64,
    pub problems: Vec<String>,
}

fn download(q: &QueueItem, media_type: &'static str, torrents: &Torrents) -> Download {
    let t = q.download_id.as_ref().and_then(|h| torrents.get(&h.to_ascii_uppercase()));
    let owner = q.movie.as_ref().or(q.series.as_ref());
    Download {
        title: owner.map_or_else(|| q.title.clone(), |o| o.title.clone()),
        media_type,
        id: owner.map_or(0, |o| o.tmdb_id),
        season: q.episode.as_ref().map_or(0, |e| e.season_number),
        episode: q.episode.as_ref().map_or(0, |e| e.episode_number),
        progress: t.map_or_else(|| q.progress(), |t| t.progress),
        speed: t.map_or(0, |t| t.dlspeed),
        eta_s: t.and_then(|t| t.eta_s()).or_else(|| q.eta_s()),
        paused: t.is_some_and(|t| t.paused()),
        hash: q.download_id.clone(),
        note: q.error_message.clone().filter(|m| !m.is_empty()),
    }
}

/// One line per torrent: a season pack is one download, not twelve.
fn one_per_torrent(items: Vec<Download>) -> Vec<Download> {
    let mut seen = HashMap::new();
    let mut out: Vec<Download> = Vec::new();
    for d in items {
        match d.hash.clone() {
            Some(h) if seen.contains_key(&h) => {
                let at: usize = seen[&h];
                out[at].episode = 0;
            }
            Some(h) => {
                seen.insert(h, out.len());
                out.push(d);
            }
            None => out.push(d),
        }
    }
    out
}

/// What's on its way now, most advanced first.
pub async fn downloads(ctx: &Ctx) -> Downloads {
    let s = services(ctx);
    let mut out = Downloads::default();
    let torrents = s.torrents().await;
    let mut items = Vec::new();
    if let Some(r) = &s.radarr {
        match r.movie_queue().await {
            Ok(q) => items.extend(q.iter().map(|x| download(x, "movie", &torrents))),
            Err(e) => out.problems.push(e.to_string()),
        }
    }
    if let Some(r) = &s.sonarr {
        match r.series_queue().await {
            Ok(q) => items.extend(q.iter().map(|x| download(x, "tv", &torrents))),
            Err(e) => out.problems.push(e.to_string()),
        }
    }
    let mut items = one_per_torrent(items);
    items.sort_by(|a, b| b.progress.total_cmp(&a.progress));
    out.speed = items.iter().map(|d| d.speed).sum();
    out.items = items;
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadAction {
    Pause,
    Resume,
}

pub async fn control(ctx: &Ctx, hash: &str, action: DownloadAction) -> Result<()> {
    let s = services(ctx);
    let qbit = s.qbit.as_ref().ok_or_else(|| Error::msg("qBittorrent isn't set up: Settings › Movies & TV"))?;
    qbit.set_paused(&hash.to_ascii_lowercase(), action == DownloadAction::Pause).await
}

// ---------- Playing ----------

#[derive(Debug, Clone, Serialize)]
pub struct Subtitle {
    /// The address on this server.
    pub path: String,
    pub lang: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NextEpisode {
    pub season: u32,
    pub episode: u32,
}

/// What a TV PC's player needs to play a file from here.
#[derive(Debug, Clone, Serialize)]
pub struct Stream {
    /// The address on this server (the TV PC already knows the host).
    pub path: String,
    pub name: String,
    pub size: u64,
    /// The episode this is; 0 for a movie.
    pub season: u32,
    pub episode: u32,
    pub subtitles: Vec<Subtitle>,
    /// The next episode the library has, right after this one.
    pub next: Option<NextEpisode>,
}

fn media_path(token: &str) -> String {
    format!("/api/media/{token}")
}

/// The episode to play: the one asked for, or the first one here.
fn pick_episode(episodes: &[Episode], season: Option<u32>, episode: Option<u32>) -> Option<&Episode> {
    let mut here: Vec<&Episode> = episodes.iter().filter(|e| e.has_file && e.season_number > 0).collect();
    here.sort_by_key(|e| (e.season_number, e.episode_number));
    match season.zip(episode) {
        Some((s, n)) => episodes.iter().find(|e| e.has_file && e.season_number == s && e.episode_number == n),
        None => here.first().copied(),
    }
}

/// The next episode after `at` that's here, in the same season or the next one to have any.
pub fn next_here(episodes: &[Episode], at: &Episode) -> Option<NextEpisode> {
    episodes
        .iter()
        .filter(|e| e.has_file && e.season_number > 0 && (e.season_number, e.episode_number) > (at.season_number, at.episode_number))
        .min_by_key(|e| (e.season_number, e.episode_number))
        .map(|e| NextEpisode { season: e.season_number, episode: e.episode_number })
}

/// A token for `file`, and for the subtitles next to it.
fn stream_of(ctx: &Ctx, roots: &[PathBuf], file: &MediaFile, at: Option<&Episode>, next: Option<NextEpisode>) -> Result<Stream> {
    let path = stream::inside_roots(roots, Path::new(&file.path))
        .filter(|p| stream::is_video(p))
        .ok_or_else(|| Error::msg("The server can't read that file: is the media folder shared with its account?"))?;
    let now = ctx.time.now_ms();
    let subtitles = stream::sidecars(&path)
        .into_iter()
        .map(|s| Subtitle { path: media_path(&ctx.library.grants.give(s.path, now)), lang: s.lang })
        .collect();
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(Stream {
        path: media_path(&ctx.library.grants.give(path, now)),
        name,
        size: file.size,
        season: at.map_or(0, |e| e.season_number),
        episode: at.map_or(0, |e| e.episode_number),
        subtitles,
        next,
    })
}

fn folders(list: Vec<arr::RootFolder>) -> Vec<PathBuf> {
    list.into_iter().map(|r| PathBuf::from(r.path)).collect()
}

/// The file of a movie or an episode, for a TV PC to play. Series without an episode named: the
/// first one here.
pub async fn stream(ctx: &Ctx, media_type: &str, id: u64, season: Option<u32>, episode: Option<u32>) -> Result<Stream> {
    let s = services(ctx);
    let gone = || Error::msg("That isn't in the library yet");
    if media_type == "movie" {
        let radarr = s.radarr()?;
        let movie = radarr.movie(id).await?.ok_or_else(gone)?;
        let file = movie.movie_file.as_ref().filter(|_| movie.has_file).ok_or_else(gone)?;
        return stream_of(ctx, &folders(radarr.root_folders().await?), file, None, None);
    }
    let sonarr = s.sonarr()?;
    let series = find_series(sonarr, id).await?.ok_or_else(gone)?;
    let (episodes, files, roots) = tokio::join!(sonarr.episodes(series.id), sonarr.episode_files(series.id), sonarr.root_folders());
    let (episodes, files, roots) = (episodes?, files?, roots?);
    let at = pick_episode(&episodes, season, episode).ok_or_else(gone)?;
    let file = files.iter().find(|f| f.id == at.episode_file_id).ok_or_else(gone)?;
    stream_of(ctx, &folders(roots), file, Some(at), next_here(&episodes, at))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(id: u64, season: u32, n: u32, has_file: bool, monitored: bool) -> Episode {
        Episode { id, season_number: season, episode_number: n, has_file, monitored, episode_file_id: if has_file { id } else { 0 } }
    }

    #[test]
    fn a_movie_goes_absent_wanted_downloading_ready() {
        let none = Torrents::default();
        assert_eq!(movie_state(None, &[], &none).state, State::Absent);
        let wanted = Movie { id: 1, ..Default::default() };
        assert_eq!(movie_state(Some(&wanted), &[], &none).state, State::Wanted);
        let q = QueueItem { movie_id: 1, size: 100.0, sizeleft: 25.0, ..Default::default() };
        let got = movie_state(Some(&wanted), &[&q], &none);
        assert_eq!((got.state, got.progress, got.ready), (State::Downloading, 0.75, false));
        let here = Movie { id: 1, has_file: true, ..Default::default() };
        let got = movie_state(Some(&here), &[], &none);
        assert_eq!((got.state, got.ready, got.progress), (State::Ready, true, 1.0));
    }

    #[test]
    fn a_series_with_more_on_its_way_can_already_play() {
        let eps = [ep(1, 1, 1, true, true), ep(2, 1, 2, false, true), ep(3, 1, 3, false, true), ep(4, 2, 1, false, false)];
        let q = QueueItem { series_id: 9, episode_id: 2, size: 10.0, sizeleft: 5.0, ..Default::default() };
        let t = series_state(&eps, &[&q], &Torrents::default());
        assert_eq!(t.state, State::Downloading);
        assert!(t.ready);
        assert_eq!(t.episodes["s1e1"].state, State::Ready);
        assert_eq!((t.episodes["s1e2"].state, t.episodes["s1e2"].progress), (State::Downloading, 0.5));
        assert_eq!(t.episodes["s1e3"].state, State::Wanted);
        assert!(!t.episodes.contains_key("s2e1"), "never asked for");
    }

    #[test]
    fn speed_comes_from_qbittorrent_whose_hashes_may_differ_in_case() {
        let mut torrents = Torrents::default();
        torrents.insert("ABC".into(), qbit::Torrent { dlspeed: 500, eta: 60, ..Default::default() });
        let q = QueueItem { download_id: Some("abc".into()), size: 10.0, sizeleft: 5.0, timeleft: Some("01:00:00".into()), ..Default::default() };
        let (_, speed, eta, _) = queue_totals(&[&q], &torrents);
        assert_eq!((speed, eta), (500, Some(60)));
    }

    #[test]
    fn fetching_skips_what_is_here_and_specials_unless_named() {
        let eps = [ep(1, 1, 1, true, true), ep(2, 1, 2, false, false), ep(3, 2, 1, false, false), ep(4, 0, 1, false, false)];
        assert_eq!(episodes_to_fetch(&eps, None, None), [2, 3]);
        assert_eq!(episodes_to_fetch(&eps, Some(2), None), [3]);
        assert_eq!(episodes_to_fetch(&eps, Some(1), Some(2)), [2]);
        assert_eq!(episodes_to_fetch(&eps, Some(0), None), [4]);
        assert!(episodes_to_fetch(&eps, Some(1), Some(1)).is_empty());
    }

    #[test]
    fn the_next_episode_skips_gaps_and_crosses_seasons() {
        let eps = [ep(1, 1, 1, true, true), ep(2, 1, 2, false, true), ep(3, 1, 3, true, true), ep(4, 2, 1, true, true), ep(5, 0, 9, true, true)];
        let next = |at: usize| next_here(&eps, &eps[at]).map(|n| (n.season, n.episode));
        assert_eq!(next(0), Some((1, 3)));
        assert_eq!(next(2), Some((2, 1)));
        assert_eq!(next(3), None);
    }

    #[test]
    fn with_no_episode_named_the_first_one_here_plays() {
        let eps = [ep(4, 2, 1, true, true), ep(1, 1, 1, false, true), ep(3, 1, 3, true, true), ep(9, 0, 1, true, true)];
        assert_eq!(pick_episode(&eps, None, None).map(|e| e.id), Some(3));
        assert_eq!(pick_episode(&eps, Some(2), Some(1)).map(|e| e.id), Some(4));
        assert!(pick_episode(&eps, Some(1), Some(1)).is_none());
    }

    #[test]
    fn a_season_pack_is_one_download() {
        let d = |hash: &str, episode| Download {
            title: "Dark".into(),
            media_type: "tv",
            id: 1,
            season: 1,
            episode,
            progress: 0.5,
            speed: 10,
            eta_s: None,
            paused: false,
            hash: Some(hash.into()),
            note: None,
        };
        let got = one_per_torrent(vec![d("a", 1), d("a", 2), d("b", 3)]);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].episode, 0, "the whole season");
        assert_eq!(got[1].episode, 3);
    }
}
