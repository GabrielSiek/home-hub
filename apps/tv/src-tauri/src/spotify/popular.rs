//! Everyone's most played songs of an artist, for the artist screen's Everyone tab: from
//! Last.fm (the Web API has no artist top tracks any more), each matched to its Spotify song
//! so it can play.

use std::collections::HashSet;

use futures_util::future::join_all;
use serde_json::Value;
use tauri::{AppHandle, Manager};

use super::library::page;
use super::plays::{song_key, top_track, TopTrack, MOST_PLAYED_MAX};
use super::{get, http, state};
use crate::error::{Error, Result};
use crate::history::now_ms;
use crate::state::AppState;

const LASTFM: &str = "https://ws.audioscrobbler.com/2.0/";
/// Last.fm's counts barely move in a day.
const CACHE_MS: i64 = 24 * 3600 * 1000;

/// The artist's most played songs on Last.fm, with everyone's play counts, most played first.
/// None without a Last.fm API key. Songs not found on Spotify are left out.
pub async fn popular(app: &AppHandle, uri: &str, name: &str) -> Result<Option<Vec<TopTrack>>> {
    let Some(key) = app.state::<AppState>().config.read().unwrap().settings.lastfm_key() else {
        return Ok(None);
    };
    if let Some((at, list)) = state(app).inner.lock().unwrap().popular.get(uri) {
        if now_ms() - at < CACHE_MS {
            return Ok(Some(list.clone()));
        }
    }
    let query = [
        ("method", "artist.gettoptracks"),
        ("artist", name),
        ("autocorrect", "1"),
        ("limit", "20"),
        ("format", "json"),
        ("api_key", key.as_str()),
    ];
    let v: Value = http(app).get(LASTFM).query(&query).send().await?.json().await?;
    if let Some(why) = v["message"].as_str().filter(|_| v["error"].is_number()) {
        return Err(Error::msg(format!("Last.fm: {why}")));
    }
    let ranked = ranked(&v);
    let found = join_all(ranked.iter().map(|(title, _)| find(app, title, uri, name))).await;
    let list: Vec<TopTrack> = ranked
        .into_iter()
        .zip(found)
        .filter_map(|((_, count), t)| Some(TopTrack { count: Some(count), ..t? }))
        .collect();
    state(app).inner.lock().unwrap().popular.insert(uri.to_string(), (now_ms(), list.clone()));
    Ok(Some(list))
}

/// Last.fm's top songs with their play counts, one version of each, at most a list's worth.
fn ranked(v: &Value) -> Vec<(String, u64)> {
    let mut seen = HashSet::new();
    v["toptracks"]["track"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let count = t["playcount"].as_str().and_then(|c| c.parse().ok()).or(t["playcount"].as_u64())?;
            Some((t["name"].as_str()?.to_string(), count))
        })
        .filter(|(title, _)| seen.insert(song_key(title)))
        .take(MOST_PLAYED_MAX)
        .collect()
}

/// The artist's song called `title` on Spotify: the same song by them, else their first hit.
async fn find(app: &AppHandle, title: &str, artist_uri: &str, artist: &str) -> Option<TopTrack> {
    let clean = |s: &str| s.replace('"', "");
    let q = format!("track:\"{}\" artist:\"{}\"", clean(title), clean(artist));
    let v = get(app, "/search", &[("q", q), ("type", "track".into()), ("limit", "5".into())]).await.ok()?;
    let theirs: Vec<TopTrack> = page(&v["tracks"], None)
        .filter_map(top_track)
        .filter(|(artists, _)| artists.iter().any(|a| a == artist_uri))
        .map(|(_, t)| t)
        .collect();
    let key = song_key(title);
    theirs.iter().find(|t| song_key(&t.title) == key).or(theirs.first()).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lastfm_counts_one_version_each() {
        let v = json!({ "toptracks": { "track": [
            { "name": "The Less I Know the Better", "playcount": "61234567" },
            { "name": "Let It Happen", "playcount": "40000000" },
            { "name": "The Less I Know The Better - Remastered", "playcount": "1000" },
            { "name": "No count" }
        ] } });
        assert_eq!(
            ranked(&v),
            [("The Less I Know the Better".to_string(), 61_234_567), ("Let It Happen".to_string(), 40_000_000)]
        );
    }
}
