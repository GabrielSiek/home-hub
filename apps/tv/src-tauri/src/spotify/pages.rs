//! The artist and podcast screens, and following either from them.

use std::collections::{HashMap, HashSet};

use futures_util::future::join_all;
use reqwest::Method;
use serde::Serialize;
use serde_json::Value;
use tauri::AppHandle;

use super::library::{image, item, page, Item};
use super::plays::{self, song_key, top_track, MostPlayed, TopTrack, MOST_PLAYED_MAX};
use super::{call, get, state, API};
use crate::error::{Error, Result};
use crate::history::now_ms;

/// The artist photo is 360 px, the show cover 340 px.
const PHOTO_PX: u64 = 640;
/// Episode rows have a 104 px cover.
const EPISODE_PX: u64 = 200;
/// The most `/artists/{id}/albums` gives at a time.
const RELEASES_PAGE: usize = 10;
/// The most `/shows/{id}/episodes` gives at a time.
const EPISODES_PAGE: u64 = 50;
/// The podcast screen lists at most this many of the newest episodes.
const EPISODES_MAX: usize = 100;
const DAY: i64 = 86_400_000;
/// The artist screen's most played periods (this month, 6 months, all time): how far back
/// each counts, and the range of the user's Spotify top songs that tops it up.
const PERIODS: [(Option<i64>, &str); 3] = [(Some(30 * DAY), "short_term"), (Some(182 * DAY), "medium_term"), (None, "long_term")];
/// The user's top songs are fetched again after this long.
const TOP_MS: i64 = 3600 * 1000;

/// An artist with their releases, for the artist screen.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Artist {
    pub uri: String,
    pub name: String,
    pub image: Option<String>,
    /// Often empty: Spotify no longer fills this in for many artists.
    pub genres: Vec<String>,
    /// Their place among the user's most played artists of the last four weeks, from 1.
    pub top_rank: Option<u32>,
    /// None when Spotify couldn't say.
    pub following: Option<bool>,
    pub albums: Vec<Item>,
    pub singles: Vec<Item>,
    pub appears_on: Vec<Item>,
    /// The user's most played songs of theirs in each of `PERIODS`: this month, the last six
    /// months, and since the launcher started counting.
    pub most_played: [MostPlayed; 3],
    /// When the launcher started counting plays, unix ms.
    pub counted_since: Option<i64>,
    /// Their newest album, EP or single.
    pub latest: Option<Latest>,
    /// The user's plays of each album and single, by URI; ones never played are left out.
    pub release_plays: HashMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Latest {
    pub release: Item,
    /// "2025-10-17"; just the year when that's all Spotify knows.
    pub date: Option<String>,
}

/// A podcast with its newest episodes, for the podcast screen.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Show {
    pub uri: String,
    pub title: String,
    pub description: Option<String>,
    pub image: Option<String>,
    /// Episodes in it; more than `episodes` holds for a long-running one.
    pub total: u64,
    /// Language codes, e.g. "pt" or "en-US".
    pub languages: Vec<String>,
    pub explicit: bool,
    /// None when Spotify couldn't say.
    pub following: Option<bool>,
    /// Newest first.
    pub episodes: Vec<Episode>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Episode {
    pub uri: String,
    pub title: String,
    pub description: String,
    pub image: Option<String>,
    /// "2026-10-21"; just the month or year when that's all Spotify knows.
    pub date: Option<String>,
    pub duration_ms: u64,
    pub explicit: bool,
    /// Listened to the end. None when the sign-in can't read playback positions.
    pub played: Option<bool>,
    /// Where the user stopped, 0 when not started; None as for `played`.
    pub resume_ms: Option<u64>,
}

fn id<'a>(uri: &'a str, kind: &str) -> Result<&'a str> {
    uri.strip_prefix(&format!("spotify:{kind}:"))
        .ok_or_else(|| Error::msg(format!("not a Spotify {kind}: {uri}")))
}

/// Whether `uri` (an artist or show) is in the user's library; None when the check failed.
async fn following(app: &AppHandle, uri: &str) -> Option<bool> {
    get(app, "/me/library/contains", &[("uris", uri.to_string())])
        .await
        .ok()
        .and_then(|v| v[0].as_bool())
}

/// Follows or unfollows an artist, or saves or removes a podcast.
pub async fn follow(app: &AppHandle, uri: &str, on: bool) -> Result<()> {
    let method = if on { Method::PUT } else { Method::DELETE };
    call(app, method, "/me/library", &[("uris", uri.to_string())], None).await??;
    Ok(())
}

// ---------- Artist ----------

pub async fn artist(app: &AppHandle, uri: &str) -> Result<Artist> {
    let id = id(uri, "artist")?;
    let path = format!("/artists/{id}");
    let (v, albums, singles, appears_on, following, top_rank, tops, _) = tokio::join!(
        get(app, &path, &[]),
        releases(app, id, "album,compilation", 30),
        releases(app, id, "single", 30),
        releases(app, id, "appears_on", 10),
        following(app, uri),
        top_rank(app, uri),
        your_top(app),
        // A song just heard counts.
        plays::sync(app, plays::SYNC_OPEN),
    );
    let v = v?;
    let (albums, singles) = (albums?, singles?);
    let latest = albums
        .iter()
        .chain(&singles)
        .filter(|(_, date)| date.is_some())
        .max_by(|a, b| a.1.cmp(&b.1))
        .map(|(r, date)| Latest { release: r.clone(), date: date.clone() });
    let undated = |list: Vec<(Item, Option<String>)>| list.into_iter().map(|(r, _)| r).collect::<Vec<_>>();
    let (albums, singles) = (undated(albums), undated(singles));
    let now = now_ms();
    let (most_played, counted_since, release_plays) = {
        let log = state(app).plays.lock().unwrap();
        let most_played = std::array::from_fn(|i| {
            let from = PERIODS[i].0.map_or(i64::MIN, |back| now - back);
            fill(log.most_played(uri, from), &tops[i], uri)
        });
        (most_played, log.since(), log.release_plays(uri, albums.iter().chain(&singles)))
    };
    Ok(Artist {
        uri: uri.to_string(),
        name: v["name"].as_str().unwrap_or_default().to_string(),
        image: image(&v["images"], PHOTO_PX),
        genres: v["genres"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|g| g.as_str().map(str::to_string))
            .take(3)
            .collect(),
        top_rank,
        following,
        albums,
        singles,
        appears_on: undated(appears_on?),
        most_played,
        counted_since,
        latest,
        release_plays,
    })
}

/// The user's Spotify top songs for each of `PERIODS`, with their artists' URIs; the same for
/// every artist, so kept a while. Empty when Spotify won't say.
async fn your_top(app: &AppHandle) -> [Vec<(Vec<String>, TopTrack)>; 3] {
    let lists = join_all(PERIODS.iter().map(|(_, range)| top_tracks(app, range))).await;
    lists.try_into().unwrap_or_default()
}

async fn top_tracks(app: &AppHandle, range: &'static str) -> Vec<(Vec<String>, TopTrack)> {
    if let Some((at, list)) = state(app).inner.lock().unwrap().top_tracks.get(range) {
        if now_ms() - at < TOP_MS {
            return list.clone();
        }
    }
    let query = [("time_range", range.to_string()), ("limit", "50".to_string())];
    let Ok(v) = get(app, "/me/top/tracks", &query).await else {
        return Vec::new();
    };
    let list: Vec<_> = page(&v, None).filter_map(top_track).collect();
    state(app).inner.lock().unwrap().top_tracks.insert(range, (now_ms(), list.clone()));
    list
}

/// Tops up a most played list with the artist's songs among the user's Spotify top songs,
/// which have no counts, while the launcher has counted few plays.
fn fill(mut m: MostPlayed, top: &[(Vec<String>, TopTrack)], artist: &str) -> MostPlayed {
    for (_, t) in top.iter().filter(|(artists, _)| artists.iter().any(|a| a == artist)) {
        if m.tracks.len() >= MOST_PLAYED_MAX {
            break;
        }
        let key = song_key(&t.title);
        if !m.tracks.iter().any(|x| song_key(&x.title) == key) {
            m.tracks.push(t.clone());
        }
    }
    m
}

/// Up to `max` of an artist's releases in `groups`, newest first, with their release dates. A
/// page after the first that fails ends the list there.
async fn releases(app: &AppHandle, id: &str, groups: &str, max: usize) -> Result<Vec<(Item, Option<String>)>> {
    let path = format!("/artists/{id}/albums");
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut offset = 0;
    while out.len() < max {
        let query = [
            ("include_groups", groups.to_string()),
            ("limit", RELEASES_PAGE.to_string()),
            ("offset", offset.to_string()),
        ];
        let v = match get(app, &path, &query).await {
            Ok(v) => v,
            Err(e) if offset == 0 => return Err(e),
            Err(_) => break,
        };
        let n = v["items"].as_array().map_or(0, Vec::len);
        // The same release is often listed again for another market or edition.
        out.extend(
            page(&v, None)
                .filter_map(|r| Some((release(r)?, r["release_date"].as_str().map(str::to_string))))
                .filter(|(r, _)| seen.insert(r.title.to_lowercase())),
        );
        offset += n;
        if n < RELEASES_PAGE || v["next"].as_str().is_none() {
            break;
        }
    }
    out.truncate(max);
    Ok(out)
}

/// A release tile: "Album · 2020", "Single · 2019".
fn release(v: &Value) -> Option<Item> {
    let kind = match v["album_type"].as_str() {
        Some("single") if v["total_tracks"].as_u64().is_some_and(|n| n >= 4) => "EP",
        Some("single") => "Single",
        Some("compilation") => "Compilation",
        _ => "Album",
    };
    let year = v["release_date"].as_str().and_then(|d| d.get(..4));
    let subtitle = match year {
        Some(y) => format!("{kind} · {y}"),
        None => kind.to_string(),
    };
    item(v, "album", subtitle)
}

/// The artist's place among the user's top artists of the last four weeks.
async fn top_rank(app: &AppHandle, uri: &str) -> Option<u32> {
    let query = [("time_range", "short_term".to_string()), ("limit", "50".to_string())];
    let v = get(app, "/me/top/artists", &query).await.ok()?;
    top_position(&v, uri)
}

fn top_position(v: &Value, uri: &str) -> Option<u32> {
    page(v, None).position(|a| a["uri"].as_str() == Some(uri)).map(|i| i as u32 + 1)
}

// ---------- Podcast ----------

pub async fn show(app: &AppHandle, uri: &str) -> Result<Show> {
    let id = id(uri, "show")?;
    let path = format!("/shows/{id}");
    let (v, following) = tokio::join!(get(app, &path, &[]), following(app, uri));
    let v = v?;
    let cover = image(&v["images"], PHOTO_PX);
    // The show lists its first page of episodes; the rest by following `next`.
    let mut list = v["episodes"].clone();
    let mut episodes = Vec::new();
    loop {
        episodes.extend(page(&list, None).filter_map(episode));
        let next = list["next"].as_str().and_then(|n| n.strip_prefix(API)).map(str::to_string);
        match next {
            Some(path) if episodes.len() < EPISODES_MAX => match get(app, &path, &[]).await {
                Ok(v) => list = v,
                Err(_) => break,
            },
            _ => break,
        }
    }
    episodes.truncate(EPISODES_MAX);
    Ok(Show {
        uri: uri.to_string(),
        title: v["name"].as_str().unwrap_or_default().to_string(),
        description: v["description"].as_str().map(|d| d.trim().to_string()).filter(|d| !d.is_empty()),
        image: cover,
        total: v["total_episodes"].as_u64().or(v["episodes"]["total"].as_u64()).unwrap_or(0),
        languages: v["languages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| l.as_str().map(str::to_string))
            .collect(),
        explicit: v["explicit"].as_bool().unwrap_or(false),
        following,
        episodes,
    })
}

/// The oldest episodes of a show with `total` of them, oldest first: the end of the list,
/// which the podcast screen doesn't have for a long-running show.
pub async fn oldest_episodes(app: &AppHandle, uri: &str, total: u64) -> Result<Vec<Episode>> {
    let id = id(uri, "show")?;
    let query = [
        ("limit", EPISODES_PAGE.to_string()),
        ("offset", total.saturating_sub(EPISODES_PAGE).to_string()),
    ];
    let v = get(app, &format!("/shows/{id}/episodes"), &query).await?;
    let mut list: Vec<Episode> = page(&v, None).filter_map(episode).collect();
    list.reverse();
    Ok(list)
}

/// An episode; ones Spotify can't list are `null` and skipped.
fn episode(v: &Value) -> Option<Episode> {
    let resume = &v["resume_point"];
    Some(Episode {
        uri: v["uri"].as_str()?.to_string(),
        title: v["name"].as_str().unwrap_or_default().to_string(),
        description: v["description"].as_str().unwrap_or_default().trim().to_string(),
        image: image(&v["images"], EPISODE_PX),
        date: v["release_date"].as_str().map(str::to_string),
        duration_ms: v["duration_ms"].as_u64().unwrap_or(0),
        explicit: v["explicit"].as_bool().unwrap_or(false),
        played: resume["fully_played"].as_bool(),
        resume_ms: resume["resume_position_ms"].as_u64(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn releases_say_what_they_are() {
        let album = json!({ "uri": "spotify:album:1", "name": "Currents", "album_type": "album", "release_date": "2015-07-17" });
        assert_eq!(release(&album).unwrap().subtitle, "Album · 2015");
        let single = json!({ "uri": "spotify:album:2", "name": "Borderline", "album_type": "single", "total_tracks": 1, "release_date": "2019" });
        assert_eq!(release(&single).unwrap().subtitle, "Single · 2019");
        let ep = json!({ "uri": "spotify:album:3", "name": "Tame Impala", "album_type": "single", "total_tracks": 5 });
        assert_eq!(release(&ep).unwrap().subtitle, "EP");
    }

    #[test]
    fn most_played_is_topped_up_from_top_songs_once_each() {
        let song = |title: &str, count: Option<u64>| TopTrack {
            uri: format!("spotify:track:{title}"),
            title: title.into(),
            album: String::new(),
            image: None,
            context: None,
            count,
            last_played: None,
        };
        let counted = MostPlayed { tracks: vec![song("Borderline", Some(3))], plays: 3, songs: 1 };
        let mine = vec!["spotify:artist:a".to_string()];
        let top = [
            (mine.clone(), song("Borderline - Remastered", None)),
            (vec!["spotify:artist:b".to_string()], song("Someone else's", None)),
            (mine, song("Elephant", None)),
        ];
        let m = fill(counted, &top, "spotify:artist:a");
        let titles: Vec<&str> = m.tracks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, ["Borderline", "Elephant"]);
        assert_eq!(m.tracks[1].count, None);
        assert_eq!(m.plays, 3, "only counted plays add up");
    }

    #[test]
    fn top_rank_counts_from_one() {
        let v = json!({ "items": [{ "uri": "spotify:artist:a" }, { "uri": "spotify:artist:b" }] });
        assert_eq!(top_position(&v, "spotify:artist:b"), Some(2));
        assert_eq!(top_position(&v, "spotify:artist:c"), None);
    }

    #[test]
    fn episode_progress_only_with_a_resume_point() {
        let started = json!({
            "uri": "spotify:episode:1", "name": "#213", "duration_ms": 4_320_000, "release_date": "2026-10-14",
            "resume_point": { "fully_played": false, "resume_position_ms": 2_700_000 }
        });
        let e = episode(&started).unwrap();
        assert_eq!((e.played, e.resume_ms), (Some(false), Some(2_700_000)));
        let unknown = json!({ "uri": "spotify:episode:2", "name": "#212" });
        let e = episode(&unknown).unwrap();
        assert_eq!((e.played, e.resume_ms), (None, None));
        assert!(episode(&json!(null)).is_none());
    }

    #[test]
    fn ids_need_the_right_kind() {
        assert_eq!(id("spotify:show:x1", "show").unwrap(), "x1");
        assert!(id("spotify:artist:x1", "show").is_err());
    }
}
