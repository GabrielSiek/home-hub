//! Things to pick and play: the library shelves on the Music screen, and search.

use std::collections::HashSet;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::AppHandle;

use super::{get, liked_songs_uri, names, state, Track, API};
use crate::error::{Error, Result};

/// Tiles are 210 px; covers this wide stay sharp on a 4K TV without being the 640 px ones.
const TILE_PX: u64 = 300;
/// Recently played: at most this many tiles.
const RECENT_MAX: usize = 16;
/// The playlist screen's cover is 400 px.
const COVER_PX: u64 = 640;
/// Song rows have a 52 px cover.
const THUMB_PX: u64 = 100;
/// A playlist screen lists at most this many songs; Play still plays the whole playlist.
const SONGS_MAX: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub uri: String,
    /// track, episode, album, playlist, artist, show or collection (Liked Songs).
    pub kind: String,
    pub title: String,
    pub subtitle: String,
    pub image: Option<String>,
    /// Where a track plays from, so what follows it keeps playing: its album, or Liked Songs.
    pub context: Option<String>,
}

/// A playlist or Liked Songs, with its songs, for the playlist screen.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Playlist {
    pub uri: String,
    pub title: String,
    /// "Your playlist", or who made it.
    pub owner: String,
    pub description: Option<String>,
    pub image: Option<String>,
    /// Songs in it; more than `songs` holds for a long one.
    pub total: u64,
    /// Empty for someone else's playlist: Spotify only lists the songs of the user's own.
    pub songs: Vec<Track>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SearchResults {
    pub tracks: Vec<Item>,
    pub artists: Vec<Item>,
    pub albums: Vec<Item>,
    pub playlists: Vec<Item>,
}

/// The smallest image at least `want` px wide, else the biggest there is. Playlist images
/// often have no width; then the first (Spotify lists the biggest first).
pub fn image(images: &Value, want: u64) -> Option<String> {
    let list: Vec<(u64, &str)> = images
        .as_array()?
        .iter()
        .filter_map(|i| Some((i["width"].as_u64().unwrap_or(0), i["url"].as_str()?)))
        .collect();
    list.iter()
        .filter(|(w, _)| *w >= want)
        .min_by_key(|(w, _)| *w)
        // Reversed: of equal widths, max_by_key keeps the last, which is then the first listed.
        .or_else(|| list.iter().rev().max_by_key(|(w, _)| *w))
        .map(|(_, url)| url.to_string())
}

pub(super) fn item(v: &Value, kind: &str, subtitle: String) -> Option<Item> {
    Some(Item {
        uri: v["uri"].as_str()?.to_string(),
        kind: kind.to_string(),
        title: v["name"].as_str().unwrap_or_default().to_string(),
        subtitle,
        image: image(&v["images"], TILE_PX),
        context: None,
    })
}

fn album_item(v: &Value) -> Option<Item> {
    let year = v["release_date"].as_str().and_then(|d| d.get(..4));
    let subtitle = [Some(names(&v["artists"])).filter(|a| !a.is_empty()), year.map(str::to_string)]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    item(v, "album", subtitle)
}

fn playlist_item(v: &Value) -> Option<Item> {
    // `items` since February 2026 (only on the user's own playlists); `tracks` before.
    let total = v["items"]["total"].as_u64().or(v["tracks"]["total"].as_u64());
    let subtitle = match (total, v["owner"]["display_name"].as_str()) {
        (Some(n), _) => format!("{n} songs"),
        (None, Some(owner)) => format!("By {owner}"),
        (None, None) => "Playlist".into(),
    };
    item(v, "playlist", subtitle)
}

fn track_item(v: &Value, context: Option<String>) -> Option<Item> {
    let episode = v["type"].as_str() == Some("episode");
    let (subtitle, images) = if episode {
        (v["show"]["name"].as_str().unwrap_or_default().to_string(), &v["images"])
    } else {
        (names(&v["artists"]), &v["album"]["images"])
    };
    Some(Item {
        uri: v["uri"].as_str()?.to_string(),
        kind: if episode { "episode" } else { "track" }.to_string(),
        title: v["name"].as_str().unwrap_or_default().to_string(),
        subtitle,
        image: image(images, TILE_PX),
        context: context.or_else(|| v["album"]["uri"].as_str().map(str::to_string)),
    })
}

fn liked_songs_item(uri: String, total: Option<u64>) -> Item {
    Item {
        uri,
        kind: "collection".into(),
        title: "Liked songs".into(),
        subtitle: total.map_or("Playlist".into(), |n| format!("{n} songs")),
        image: None,
        context: None,
    }
}

/// The items of a paged list, e.g. `/me/albums`'s `items[].album`.
pub(super) fn page<'a>(v: &'a Value, inner: Option<&str>) -> impl Iterator<Item = &'a Value> {
    let inner = inner.map(str::to_string);
    v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(move |i| match &inner {
            Some(k) => &i[k.as_str()],
            None => i,
        })
}

/// One shelf of the library: recent, playlists, albums, artists or shows.
pub async fn shelf(app: &AppHandle, kind: &str) -> Result<Vec<Item>> {
    let limit = || ("limit", "50".to_string());
    Ok(match kind {
        "recent" => recent(app).await?,
        "playlists" => {
            let v = get(app, "/me/playlists", &[limit()]).await?;
            let liked_total = get(app, "/me/tracks", &[("limit", "1".into())])
                .await
                .ok()
                .and_then(|t| t["total"].as_u64());
            std::iter::once(liked_songs_item(liked_songs_uri(app)?, liked_total))
                .chain(page(&v, None).filter_map(playlist_item))
                .collect()
        }
        "albums" => {
            let v = get(app, "/me/albums", &[limit()]).await?;
            page(&v, Some("album")).filter_map(album_item).collect()
        }
        "artists" => {
            let v = get(app, "/me/following", &[("type", "artist".into()), limit()]).await?;
            page(&v["artists"], None).filter_map(|a| item(a, "artist", "Artist".into())).collect()
        }
        "shows" => {
            let v = get(app, "/me/shows", &[limit()]).await?;
            page(&v, Some("show")).filter_map(|s| item(s, "show", "Podcast".into())).collect()
        }
        _ => return Err(Error::msg(format!("no shelf '{kind}'"))),
    })
}

/// What was played lately, by playlist, album or artist where it was played from.
async fn recent(app: &AppHandle) -> Result<Vec<Item>> {
    let v = get(app, "/me/player/recently-played", &[("limit", "50".into())]).await?;
    // The artist screen's play counts come from here too.
    let _ = super::plays::record(app, &v);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for played in page(&v, None) {
        let track = &played["track"];
        let context = played["context"]["uri"].as_str();
        let Some(key) = context.or(track["uri"].as_str()) else {
            continue;
        };
        if !seen.insert(key.to_string()) {
            continue;
        }
        let tile = match context {
            Some(uri) => context_item(app, uri, track).await,
            None => None,
        };
        if let Some(tile) = tile.or_else(|| track_item(track, None)) {
            out.push(tile);
        }
        if out.len() >= RECENT_MAX {
            break;
        }
    }
    Ok(out)
}

/// A tile for something played from; cached, since recently played lists the same ones.
async fn context_item(app: &AppHandle, uri: &str, track: &Value) -> Option<Item> {
    if let Some(t) = state(app).inner.lock().unwrap().tiles.get(uri) {
        return Some(t.clone());
    }
    let mut parts = uri.split(':').skip(1);
    let (kind, id) = (parts.next()?, parts.next()?);
    let tile = match kind {
        "collection" => Some(liked_songs_item(uri.to_string(), None)),
        "user" if uri.ends_with(":collection") => Some(liked_songs_item(uri.to_string(), None)),
        "album" => album_item(&track["album"]),
        "playlist" => {
            let fields = [("fields", "uri,name,images,owner(display_name)".to_string())];
            playlist_item(&get(app, &format!("/playlists/{id}"), &fields).await.ok()?)
        }
        "artist" => item(&get(app, &format!("/artists/{id}"), &[]).await.ok()?, "artist", "Artist".into()),
        "show" => item(&get(app, &format!("/shows/{id}"), &[]).await.ok()?, "show", "Podcast".into()),
        _ => None,
    }?;
    state(app).inner.lock().unwrap().tiles.insert(uri.to_string(), tile.clone());
    Some(tile)
}

/// A playlist (`spotify:playlist:<id>`), album (`spotify:album:<id>`) or Liked Songs, with up
/// to `SONGS_MAX` of its songs.
pub async fn playlist(app: &AppHandle, uri: &str) -> Result<Playlist> {
    if let Some(id) = uri.strip_prefix("spotify:album:") {
        return album(app, uri, id).await;
    }
    if uri.ends_with(":collection") {
        let first = get(app, "/me/tracks", &[("limit", "50".into())]).await?;
        return Ok(Playlist {
            uri: uri.to_string(),
            title: "Liked songs".into(),
            owner: "Your songs".into(),
            description: None,
            image: None,
            total: first["total"].as_u64().unwrap_or(0),
            songs: songs(app, first).await,
        });
    }
    let id = uri
        .strip_prefix("spotify:playlist:")
        .ok_or_else(|| Error::msg(format!("not a playlist: {uri}")))?;
    let v = get(app, &format!("/playlists/{id}"), &[]).await?;
    let me = state(app).token()?.user;
    let owner = match (v["owner"]["id"].as_str(), v["owner"]["display_name"].as_str()) {
        (Some(o), _) if o == me => "Your playlist".to_string(),
        (_, Some(name)) => format!("By {name}"),
        _ => "Playlist".to_string(),
    };
    // `items` since February 2026 (only on the user's own playlists); `tracks` before.
    let list = if v["items"].is_object() { &v["items"] } else { &v["tracks"] };
    Ok(Playlist {
        uri: uri.to_string(),
        title: v["name"].as_str().unwrap_or_default().to_string(),
        owner,
        description: v["description"].as_str().map(plain_text).filter(|d| !d.is_empty()),
        image: image(&v["images"], COVER_PX),
        total: list["total"].as_u64().unwrap_or(0),
        songs: songs(app, list.clone()).await,
    })
}

/// An album as a playlist: by its artists and year, its songs carrying its name and cover,
/// which Spotify leaves out of an album's own song list.
async fn album(app: &AppHandle, uri: &str, id: &str) -> Result<Playlist> {
    let v = get(app, &format!("/albums/{id}"), &[]).await?;
    let title = v["name"].as_str().unwrap_or_default().to_string();
    let year = v["release_date"].as_str().and_then(|d| d.get(..4)).map(str::to_string);
    let thumb = image(&v["images"], THUMB_PX);
    let mut list = songs(app, v["tracks"].clone()).await;
    for s in &mut list {
        s.album = title.clone();
        s.year = year.clone();
        s.image = thumb.clone();
    }
    Ok(Playlist {
        uri: uri.to_string(),
        owner: album_item(&v).map(|a| a.subtitle).unwrap_or_else(|| "Album".into()),
        title,
        description: None,
        image: image(&v["images"], COVER_PX),
        total: v["tracks"]["total"].as_u64().unwrap_or(0),
        songs: list,
    })
}

/// The songs of a paged list, following `next` up to `SONGS_MAX`. A page that fails ends
/// the list there: what's listed so far is still worth showing.
async fn songs(app: &AppHandle, mut list: Value) -> Vec<Track> {
    let mut out = Vec::new();
    loop {
        out.extend(page_songs(&list));
        let next = list["next"].as_str().and_then(|n| n.strip_prefix(API)).map(str::to_string);
        match next {
            Some(path) if out.len() < SONGS_MAX => match get(app, &path, &[]).await {
                Ok(v) => list = v,
                Err(_) => break,
            },
            _ => break,
        }
    }
    out.truncate(SONGS_MAX);
    out
}

/// One page's songs; removed ones are listed as `null` and skipped.
fn page_songs(list: &Value) -> Vec<Track> {
    // A playlist entry's song is `item` since February 2026, `track` before; an album
    // lists its songs as they are.
    page(list, None)
        .filter_map(|e| {
            song(if e["item"].is_object() {
                &e["item"]
            } else if e.get("track").is_some() {
                &e["track"]
            } else {
                e
            })
        })
        .collect()
}

fn song(v: &Value) -> Option<Track> {
    let episode = v["type"].as_str() == Some("episode");
    let (artist, album) = if episode {
        let show = v["show"]["name"].as_str().unwrap_or_default().to_string();
        (show.clone(), show)
    } else {
        (names(&v["artists"]), v["album"]["name"].as_str().unwrap_or_default().to_string())
    };
    let images = if episode { &v["images"] } else { &v["album"]["images"] };
    Some(Track {
        uri: v["uri"].as_str()?.to_string(),
        title: v["name"].as_str().unwrap_or_default().to_string(),
        artist,
        album,
        year: v["album"]["release_date"].as_str().and_then(|d| d.get(..4)).map(str::to_string),
        duration_ms: v["duration_ms"].as_u64().unwrap_or(0),
        image: image(images, THUMB_PX),
    })
}

/// A playlist description as Spotify gives it (links as `<a>` tags, HTML entities) in plain text.
fn plain_text(html: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    text.replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .trim()
        .to_string()
}

pub async fn search(app: &AppHandle, query: &str) -> Result<SearchResults> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(SearchResults::default());
    }
    let v = get(
        app,
        "/search",
        &[
            ("q", query.to_string()),
            ("type", "track,artist,album,playlist".into()),
            // The most Spotify gives in development mode.
            ("limit", "10".into()),
        ],
    )
    .await?;
    Ok(parse_search(&v))
}

fn parse_search(v: &Value) -> SearchResults {
    // Search can list `null` among the playlists; the parsers skip anything without a URI.
    SearchResults {
        tracks: page(&v["tracks"], None).filter_map(|t| track_item(t, None)).collect(),
        artists: page(&v["artists"], None).filter_map(|a| item(a, "artist", "Artist".into())).collect(),
        albums: page(&v["albums"], None).filter_map(album_item).collect(),
        playlists: page(&v["playlists"], None).filter_map(playlist_item).collect(),
    }
}

/// The body of a play request: a context from its start, or a track or episode inside
/// `context` (so the album or playlist carries on after it), or on its own.
pub fn play_body(uri: &str, context: Option<&str>) -> Value {
    let single = uri.starts_with("spotify:track:") || uri.starts_with("spotify:episode:");
    match (single, context) {
        (true, Some(ctx)) => json!({ "context_uri": ctx, "offset": { "uri": uri } }),
        (true, None) => json!({ "uris": [uri] }),
        (false, _) => json!({ "context_uri": uri }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_picks_the_closest_bigger_one() {
        let imgs = json!([
            { "url": "640", "width": 640 },
            { "url": "300", "width": 300 },
            { "url": "64", "width": 64 }
        ]);
        assert_eq!(image(&imgs, 300).as_deref(), Some("300"));
        assert_eq!(image(&imgs, 200).as_deref(), Some("300"));
        assert_eq!(image(&imgs, 1000).as_deref(), Some("640"));
        let no_widths = json!([{ "url": "first", "width": null }, { "url": "second", "width": null }]);
        assert_eq!(image(&no_widths, 300).as_deref(), Some("first"));
        assert_eq!(image(&json!(null), 300), None);
    }

    #[test]
    fn play_body_shapes() {
        assert_eq!(play_body("spotify:album:1", None), json!({ "context_uri": "spotify:album:1" }));
        assert_eq!(
            play_body("spotify:track:2", Some("spotify:album:1")),
            json!({ "context_uri": "spotify:album:1", "offset": { "uri": "spotify:track:2" } })
        );
        assert_eq!(play_body("spotify:episode:3", None), json!({ "uris": ["spotify:episode:3"] }));
    }

    #[test]
    fn playlist_song_count_old_and_new_field() {
        let new = json!({ "uri": "spotify:playlist:a", "name": "Mine", "items": { "total": 48 } });
        assert_eq!(playlist_item(&new).unwrap().subtitle, "48 songs");
        let old = json!({ "uri": "spotify:playlist:b", "name": "Old", "tracks": { "total": 3 } });
        assert_eq!(playlist_item(&old).unwrap().subtitle, "3 songs");
        let theirs = json!({ "uri": "spotify:playlist:c", "name": "Theirs", "owner": { "display_name": "Ana" } });
        assert_eq!(playlist_item(&theirs).unwrap().subtitle, "By Ana");
    }

    #[test]
    fn playlist_songs_old_and_new_field_skipping_removed_ones() {
        let new = json!({ "items": [
            { "item": {
                "uri": "spotify:track:1", "name": "Nights", "type": "track", "duration_ms": 307000,
                "artists": [{ "name": "Frank Ocean" }],
                "album": { "name": "Blonde", "release_date": "2016-08-20", "images": [] }
            } },
            { "item": null }
        ] });
        let songs = page_songs(&new);
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].album, "Blonde");
        assert_eq!(songs[0].year.as_deref(), Some("2016"));
        assert_eq!(songs[0].duration_ms, 307000);
        let old = json!({ "items": [{ "track": { "uri": "spotify:track:2", "name": "AM", "artists": [] } }] });
        assert_eq!(page_songs(&old)[0].uri, "spotify:track:2");
        let album = json!({ "items": [{ "uri": "spotify:track:3", "name": "Solo", "type": "track", "artists": [] }] });
        assert_eq!(page_songs(&album)[0].title, "Solo");
    }

    #[test]
    fn descriptions_lose_tags_and_entities() {
        assert_eq!(
            plain_text("Songs from <a href=\"spotify:artist:1\">Tame Impala</a> &amp; friends. It&#x27;s late"),
            "Songs from Tame Impala & friends. It's late"
        );
    }

    #[test]
    fn search_skips_null_entries_and_plays_tracks_in_their_album() {
        let v = json!({
            "tracks": { "items": [{
                "uri": "spotify:track:1", "name": "Let It Happen", "type": "track",
                "artists": [{ "name": "Tame Impala" }],
                "album": { "uri": "spotify:album:9", "images": [] }
            }] },
            "playlists": { "items": [null, { "uri": "spotify:playlist:p", "name": "Mix" }] },
            "albums": { "items": [{
                "uri": "spotify:album:9", "name": "Currents", "release_date": "2015-07-17",
                "artists": [{ "name": "Tame Impala" }]
            }] }
        });
        let r = parse_search(&v);
        assert_eq!(r.tracks[0].context.as_deref(), Some("spotify:album:9"));
        assert_eq!(r.tracks[0].subtitle, "Tame Impala");
        assert_eq!(r.playlists.len(), 1);
        assert_eq!(r.albums[0].subtitle, "Tame Impala · 2015");
        assert!(r.artists.is_empty());
    }
}
