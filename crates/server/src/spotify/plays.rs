//! The user's own play counts, which the Web API doesn't give: every song played on any
//! device, read from recently played (its last 50) while the launcher runs, and kept in
//! spotify-plays.json. Plays past the last 50 while the launcher was off are missed.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::library::{image, page, Item};
use super::{get, state};
use tvl_kit::Result;
use tvl_kit::time::{now_ms, rfc3339_ms};

use crate::ctx::Ctx;

const FILE: &str = "spotify-plays.json";
/// Song rows have a 40 px cover.
const THUMB_PX: u64 = 100;
/// The most played list has this many songs at most.
pub const MOST_PLAYED_MAX: usize = 10;
/// Recently played is read at most this often in the background…
pub const SYNC_IDLE: i64 = 5 * 60_000;
/// …and on opening an artist, so a song just heard counts.
pub const SYNC_OPEN: i64 = 60_000;

/// One song played.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Play {
    /// Unix ms.
    pub at: i64,
    pub uri: String,
    pub title: String,
    pub album: String,
    pub album_uri: Option<String>,
    pub image: Option<String>,
    /// Artist URIs.
    pub artists: Vec<String>,
}

/// A song in a most played list.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TopTrack {
    pub uri: String,
    pub title: String,
    pub album: String,
    pub image: Option<String>,
    /// Its album, so what follows it keeps playing.
    pub context: Option<String>,
    /// The user's plays, or everyone's on Last.fm. None for a song from the user's Spotify top
    /// songs, which come without counts.
    pub count: Option<u64>,
    /// When the user last played it, unix ms.
    pub last_played: Option<i64>,
}

/// An artist's most played songs over a period.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MostPlayed {
    pub tracks: Vec<TopTrack>,
    /// The artist's plays in the period, and how many different songs they were.
    pub plays: u64,
    pub songs: u64,
}

pub struct Log {
    path: PathBuf,
    /// Oldest first.
    plays: Vec<Play>,
    /// When recently played was last read, unix ms.
    synced: i64,
}

impl Log {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(FILE);
        let plays = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { path, plays, synced: 0 }
    }

    fn save(&self) -> Result<()> {
        // Not pretty-printed: a year of listening is thousands of plays.
        fs::write(&self.path, serde_json::to_string(&self.plays)?)?;
        Ok(())
    }

    /// Adds the plays newer than the newest one kept; true when there were any.
    fn add(&mut self, new: impl Iterator<Item = Play>) -> bool {
        let newest = self.plays.last().map_or(i64::MIN, |p| p.at);
        let mut fresh: Vec<Play> = new.filter(|p| p.at > newest).collect();
        if fresh.is_empty() {
            return false;
        }
        fresh.sort_by_key(|p| p.at);
        self.plays.extend(fresh);
        true
    }

    /// When counting started: the first play kept.
    pub fn since(&self) -> Option<i64> {
        self.plays.first().map(|p| p.at)
    }

    fn by(&self, artist: &str) -> impl Iterator<Item = &Play> {
        let artist = artist.to_string();
        self.plays.iter().filter(move |p| p.artists.contains(&artist))
    }

    /// The artist's songs played since `from` (unix ms), most played first; of equal counts,
    /// the one played last. Versions of a song (single, album, remaster) count as one.
    pub fn most_played(&self, artist: &str, from: i64) -> MostPlayed {
        let mut songs: HashMap<String, (u64, &Play)> = HashMap::new();
        let mut plays = 0;
        for p in self.by(artist).filter(|p| p.at >= from) {
            plays += 1;
            let e = songs.entry(song_key(&p.title)).or_insert((0, p));
            e.0 += 1;
            // Oldest first: this is the latest play so far.
            e.1 = p;
        }
        let n = songs.len() as u64;
        let mut list: Vec<(u64, &Play)> = songs.into_values().collect();
        list.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.at.cmp(&a.1.at)));
        MostPlayed {
            tracks: list
                .into_iter()
                .take(MOST_PLAYED_MAX)
                .map(|(count, p)| TopTrack {
                    uri: p.uri.clone(),
                    title: p.title.clone(),
                    album: p.album.clone(),
                    image: p.image.clone(),
                    context: p.album_uri.clone(),
                    count: Some(count),
                    last_played: Some(p.at),
                })
                .collect(),
            plays,
            songs: n,
        }
    }

    /// The user's plays of each of `releases` by the artist, by release URI; none left out.
    /// Matched by name too, as an edition the user played has a URI of its own.
    pub fn release_plays<'a>(&self, artist: &str, releases: impl Iterator<Item = &'a Item>) -> HashMap<String, u64> {
        releases
            .filter_map(|r| {
                let title = r.title.to_lowercase();
                let n = self
                    .by(artist)
                    .filter(|p| p.album_uri.as_deref() == Some(r.uri.as_str()) || p.album.to_lowercase() == title)
                    .count() as u64;
                (n > 0).then(|| (r.uri.clone(), n))
            })
            .collect()
    }
}

/// One version of a song or another: "Borderline - Remastered" and "Borderline" are the same.
pub fn song_key(title: &str) -> String {
    let t = title.to_lowercase();
    let t = t.split(" - ").next().unwrap_or(&t);
    let t = match t.find(" (") {
        Some(i) if t[i..].contains("remaster") || t[i..].contains("version") || t[i..].contains("edit") => &t[..i],
        _ => t,
    };
    t.trim().to_string()
}

/// A song from the Web API as a most played row, with its artists' URIs; no counts yet.
pub(super) fn top_track(t: &Value) -> Option<(Vec<String>, TopTrack)> {
    let artists = t["artists"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["uri"].as_str().map(str::to_string))
        .collect();
    let track = TopTrack {
        uri: t["uri"].as_str()?.to_string(),
        title: t["name"].as_str().unwrap_or_default().to_string(),
        album: t["album"]["name"].as_str().unwrap_or_default().to_string(),
        image: image(&t["album"]["images"], THUMB_PX),
        context: t["album"]["uri"].as_str().map(str::to_string),
        count: None,
        last_played: None,
    };
    Some((artists, track))
}

/// A recently played entry; podcast episodes aren't counted.
fn play(v: &Value) -> Option<Play> {
    let t = &v["track"];
    if t["type"].as_str().is_some_and(|k| k != "track") {
        return None;
    }
    let (artists, track) = top_track(t)?;
    Some(Play {
        at: rfc3339_ms(v["played_at"].as_str()?)?,
        uri: track.uri,
        title: track.title,
        album: track.album,
        album_uri: track.context,
        image: track.image,
        artists,
    })
}

/// Keeps the new plays of a recently played reply.
pub(super) fn record(app: &Ctx, v: &Value) -> Result<()> {
    let mut log = state(app).plays.lock();
    if log.add(page(v, None).filter_map(play)) {
        log.save()?;
    }
    Ok(())
}

/// Reads recently played, unless it was read in the last `every` ms.
pub async fn sync(app: &Ctx, every: i64) -> Result<()> {
    {
        let mut log = state(app).plays.lock();
        if now_ms() - log.synced < every {
            return Ok(());
        }
        log.synced = now_ms();
    }
    let v = get(app, "/me/player/recently-played", &[("limit", "50".into())]).await?;
    record(app, &v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn log(plays: &[(i64, &str, &str)]) -> Log {
        let mut l = Log { path: PathBuf::new(), plays: Vec::new(), synced: 0 };
        l.add(plays.iter().map(|(at, title, album)| Play {
            at: *at,
            uri: format!("spotify:track:{title}"),
            title: title.to_string(),
            album: album.to_string(),
            album_uri: Some(format!("spotify:album:{album}")),
            image: None,
            artists: vec!["spotify:artist:a".into()],
        }));
        l
    }

    #[test]
    fn most_played_counts_versions_as_one_and_breaks_ties_by_latest() {
        let l = log(&[
            (1, "Borderline", "Single"),
            (2, "Borderline - Remastered", "The Slow Rush"),
            (3, "Let It Happen", "Currents"),
            (4, "Eventually", "Currents"),
            (5, "Let It Happen", "Currents"),
        ]);
        let m = l.most_played("spotify:artist:a", 0);
        assert_eq!((m.plays, m.songs), (5, 3));
        // Two plays each: Let It Happen was played last.
        assert_eq!(m.tracks[0].title, "Let It Happen");
        assert_eq!(m.tracks[1].album, "The Slow Rush", "the latest version's details");
        assert_eq!(m.tracks[1].count, Some(2));
        assert_eq!(l.most_played("spotify:artist:a", 4).plays, 2);
        assert_eq!(l.most_played("spotify:artist:b", 0).plays, 0);
    }

    #[test]
    fn only_newer_plays_are_added() {
        let mut l = log(&[(10, "A", "X")]);
        let again = l.plays[0].clone();
        assert!(!l.add(std::iter::once(again.clone())));
        assert!(l.add(std::iter::once(Play { at: 11, ..again })));
        assert_eq!(l.plays.len(), 2);
    }

    #[test]
    fn release_plays_by_uri_or_name() {
        let l = log(&[(1, "Nangs", "Currents"), (2, "Eventually", "currents"), (3, "Patience", "Patience")]);
        let item = |uri: &str, title: &str| Item {
            uri: uri.into(),
            kind: "album".into(),
            title: title.into(),
            subtitle: String::new(),
            image: None,
            context: None,
        };
        let releases = [item("spotify:album:other", "Currents"), item("spotify:album:none", "Lonerism")];
        let n = l.release_plays("spotify:artist:a", releases.iter());
        assert_eq!(n.get("spotify:album:other"), Some(&2));
        assert!(!n.contains_key("spotify:album:none"));
    }

    #[test]
    fn recently_played_entries() {
        let v = json!({ "items": [
            { "played_at": "2026-10-25T18:00:00.000Z", "track": {
                "type": "track", "uri": "spotify:track:1", "name": "Dracula",
                "artists": [{ "uri": "spotify:artist:a" }],
                "album": { "uri": "spotify:album:d", "name": "Deadbeat", "images": [] }
            } },
            { "played_at": "2026-10-25T17:00:00Z", "track": { "type": "episode", "uri": "spotify:episode:2" } }
        ] });
        let plays: Vec<Play> = page(&v, None).filter_map(play).collect();
        assert_eq!(plays.len(), 1);
        assert_eq!(plays[0].album_uri.as_deref(), Some("spotify:album:d"));
        assert_eq!(plays[0].artists, ["spotify:artist:a"]);
    }

    #[test]
    fn song_keys() {
        assert_eq!(song_key("Borderline - 2020 Remaster"), "borderline");
        assert_eq!(song_key("Lost in Yesterday (Radio Edit)"), "lost in yesterday");
        assert_eq!(song_key("Feels Like We Only Go Backwards (Live)"), "feels like we only go backwards (live)");
    }
}
