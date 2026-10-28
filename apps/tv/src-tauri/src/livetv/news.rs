//! News channels on YouTube. Each streams its programmes one by one, every one scheduled ahead as
//! a stream of its own (checked 2026-10-26: CNN Brasil's "AO VIVO: CNN PRIME TIME - 26/10/2026" was
//! listed for 20:00 that morning), and some keep a stream on around the clock (Record News, Rádio
//! Gaúcha 24 horas). So a channel's streams tab is its programme guide: the most watched live
//! stream is on now, the scheduled ones come next.
//!
//! - The streams tab: one GET per channel, no quota (sports/cazetv.rs reads it).
//! - With a YouTube Data API key, `videos.list` gives the real start and the viewers (1 unit per
//!   50 videos, every channel's in one call). Without one, a live stream started when it was
//!   scheduled to, or when its watch page says.
//! - Adding a channel: YouTube's channel search, read from its page like the streams tab.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::error::{Error, Result};
use crate::history::now_ms;
use crate::sports::cazetv::Stream;
use crate::sports::tv::{self, TvChannel};
use crate::sports;
use crate::state::AppState;

const FILE: &str = "live-news.json";
const STREAMS: &str = "https://www.youtube.com/channel/";
const SEARCH: &str = "https://www.youtube.com/results";
/// YouTube's search filter for channels only.
const CHANNELS_ONLY: &str = "EgIQAg==";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
const MIN: i64 = 60_000;
/// Programmes start and end through the day.
const EVERY: i64 = 10 * MIN;
const RETRY: i64 = 3 * MIN;
/// A live thumbnail changes with the stream: a new address this often gets past the caches.
const FRAME_EVERY: i64 = 5 * MIN;

/// A news channel on YouTube, as the user lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewsSource {
    /// Its channel id, "UC…".
    pub id: String,
    pub name: String,
    pub color: String,
}

/// The channels listed until the user changes the list: Brazil's free news channels, Porto
/// Alegre's radios with them.
const DEFAULTS: &[(&str, &str, &str)] = &[
    ("UCUE2SZQrlscY_rMphL6G5aQ", "GZH", "#1C7ED6"),
    ("UCvdwhh_fDyWccR42-rReZLw", "CNN Brasil", "#CC0000"),
    ("UCoa-D_VfMkFrCYodrOC9-mA", "Band Jornalismo", "#E67700"),
    ("UCP391YRAjSOdM_bwievgaZA", "Jovem Pan News", "#495057"),
    ("UCuiLR4p6wQ3xLEm15pEn1Xw", "Record News", "#1864AB"),
    ("UC376n347Ob5Lwzq2WGzF1AA", "SBT News", "#2B8A3E"),
    ("UCVFbXI6Gu8U2f9Gjtxw4A-Q", "UOL", "#F08C00"),
    ("UCRC3upGcj_decdPi7JXT7Bg", "Rádio Gaúcha", "#0B7285"),
    ("UCWijW6tW0iI5ghsAbWDFtTg", "BandNews FM", "#D9480F"),
    ("UCF453hXTROcm9CfCERYUjtQ", "Rádio Bandeirantes", "#C92A2A"),
    ("UCcrwxZP-L12EFr150HTFk3w", "Rádio Guaíba", "#1971C2"),
];

pub fn defaults() -> Vec<NewsSource> {
    DEFAULTS
        .iter()
        .map(|(id, name, color)| NewsSource { id: id.to_string(), name: name.to_string(), color: color.to_string() })
        .collect()
}

/// Colors for channels the user adds, in turn.
const PALETTE: &[&str] = &["#1C7ED6", "#E8590C", "#7048E8", "#0CA678", "#D6336C", "#F59F00", "#1098AD", "#5C940D"];

pub fn color_for(n: usize) -> String {
    PALETTE[n % PALETTE.len()].to_string()
}

/// What every channel's streams tab listed, and when.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Listing {
    fetched_at: i64,
    /// Channel id → its live and scheduled streams.
    channels: Vec<(String, Vec<Stream>)>,
}

impl Listing {
    fn streams(&self, id: &str) -> &[Stream] {
        self.channels.iter().find(|(c, _)| c == id).map_or(&[], |(_, s)| s.as_slice())
    }
}

pub struct Cache {
    path: PathBuf,
    listing: Mutex<Option<Listing>>,
    tried: Mutex<i64>,
    error: Mutex<Option<String>>,
    /// One read at a time; the next caller gets its answer.
    reading: tokio::sync::Mutex<()>,
}

impl Cache {
    pub fn new(data_dir: &Path) -> Self {
        let path = data_dir.join(FILE);
        let listing = fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok());
        Self {
            path,
            listing: Mutex::new(listing),
            tried: Mutex::new(0),
            error: Mutex::new(None),
            reading: tokio::sync::Mutex::new(()),
        }
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().unwrap().clone()
    }
}

fn http(app: &AppHandle) -> reqwest::Client {
    app.state::<AppState>().http.clone()
}

/// Reads the channels' streams again when that's due: every ten minutes, at once for a channel
/// just added.
pub async fn refresh(app: &AppHandle, cache: &Cache, sources: &[NewsSource]) {
    let _one = cache.reading.lock().await;
    let now = now_ms();
    let fresh = cache.listing.lock().unwrap().as_ref().is_some_and(|l| {
        now - l.fetched_at < EVERY && sources.iter().all(|s| l.channels.iter().any(|(id, _)| *id == s.id))
    });
    if fresh || sources.is_empty() || now - *cache.tried.lock().unwrap() < RETRY {
        return;
    }
    *cache.tried.lock().unwrap() = now;
    let old = cache.listing.lock().unwrap().clone().unwrap_or_default();
    let urls: Vec<String> = sources.iter().map(|s| format!("{STREAMS}{}/streams", s.id)).collect();
    let pages = join_all(sources.iter().zip(&urls).map(|(s, u)| sports::streams_page(app, u, &s.name))).await;
    let mut failed = Vec::new();
    let mut read: Vec<(String, Vec<Stream>)> = Vec::new();
    for (s, page) in sources.iter().zip(pages) {
        match page {
            Ok(streams) => read.push((s.id.clone(), streams)),
            Err(e) => {
                eprintln!("live: {}: {e}", s.name);
                failed.push(s.name.clone());
                // What it listed last time stands.
                read.push((s.id.clone(), old.streams(&s.id).to_vec()));
            }
        }
    }
    if failed.len() == sources.len() {
        *cache.error.lock().unwrap() = Some("The news channels on YouTube aren't answering".into());
        return;
    }
    let key = app.state::<AppState>().config.read().unwrap().settings.youtube_key();
    let detailed = match &key {
        Some(key) => {
            let ids: Vec<String> = read.iter().flat_map(|(_, s)| s.iter().map(|x| x.id.clone())).collect();
            sports::stream_details(app, key, &ids)
                .await
                .map_err(|e| eprintln!("live: YouTube Data API: {e}"))
                .ok()
        }
        None => None,
    };
    let channels = match detailed {
        // The API leaves out what ended since the page was read.
        Some(details) => read
            .into_iter()
            .map(|(id, streams)| {
                let kept = streams.iter().filter_map(|s| details.iter().find(|d| d.id == s.id).cloned()).collect();
                (id, kept)
            })
            .collect(),
        None => {
            let starts = read.into_iter().map(|(id, streams)| {
                let old = old.streams(&id).to_vec();
                async move { (id, sports::stream_starts(app, streams, &old, 1).await) }
            });
            join_all(starts).await
        }
    };
    let listing = Listing { fetched_at: now, channels };
    if let Ok(text) = serde_json::to_string(&listing) {
        let _ = fs::write(&cache.path, text);
    }
    *cache.listing.lock().unwrap() = Some(listing);
    *cache.error.lock().unwrap() = (!failed.is_empty()).then(|| format!("Not answering: {}", failed.join(", ")));
}

/// The news channels with their programmes, as last read. One with nothing live has no address
/// to play, only what's coming.
pub fn channels(cache: &Cache, sources: &[NewsSource], now: i64) -> Vec<TvChannel> {
    let listing = cache.listing.lock().unwrap().clone().unwrap_or_default();
    sources.iter().map(|s| channel(s, listing.streams(&s.id), now)).collect()
}

fn channel(src: &NewsSource, streams: &[Stream], now: i64) -> TvChannel {
    let live = streams.iter().filter(|s| s.live).max_by_key(|s| s.viewers);
    // A show past its start that isn't live (late, or called off) isn't listed as on.
    let mut later: Vec<&Stream> = streams.iter().filter(|s| !s.live && s.scheduled.is_some_and(|t| t > now)).collect();
    later.sort_by_key(|s| s.scheduled);
    later.dedup_by_key(|s| s.scheduled);
    let mut slots = Vec::new();
    if let Some(s) = live {
        slots.push(tv::news_slot(s, later.first().and_then(|l| l.scheduled), now));
    }
    for (i, s) in later.iter().enumerate() {
        slots.push(tv::news_slot(s, later.get(i + 1).and_then(|n| n.scheduled), now));
    }
    let frame = live.and_then(|s| s.thumb.as_ref()).map(|t| {
        let join = if t.contains('?') { '&' } else { '?' };
        format!("{t}{join}t={}", now / FRAME_EVERY)
    });
    TvChannel {
        key: format!("yt-{}", src.id),
        name: src.name.clone(),
        color: src.color.clone(),
        group: "news".into(),
        provider: "youtube".into(),
        access: "Free on YouTube".into(),
        free: true,
        frame,
        app: "youtube".into(),
        url: live.map(|s| format!("{}{}", tv::YOUTUBE_WATCH, s.id)),
        slots,
        viewers: live.map(|s| s.viewers),
    }
}

// ---------- Finding a channel ----------

/// A channel YouTube's search found.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Found {
    pub id: String,
    pub name: String,
    /// "@bandjornalismo".
    pub handle: Option<String>,
    /// "7,46 mi de inscritos".
    pub subscribers: Option<String>,
    pub avatar: Option<String>,
}

fn runs(v: &Value) -> String {
    v["runs"]
        .as_array()
        .map(|r| r.iter().filter_map(|x| x["text"].as_str()).collect())
        .or_else(|| v["simpleText"].as_str().map(str::to_string))
        .unwrap_or_default()
}

fn channel_renderers<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(o) => {
            if let Some(c) = o.get("channelRenderer") {
                out.push(c);
                return;
            }
            o.values().for_each(|x| channel_renderers(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| channel_renderers(x, out)),
        _ => {}
    }
}

/// The channels on a search page. None when it isn't the page it was.
fn parse_search(html: &str) -> Option<Vec<Found>> {
    let marker = "var ytInitialData = ";
    let at = html.find(marker)? + marker.len();
    let data: Value = serde_json::Deserializer::from_str(&html[at..]).into_iter::<Value>().next()?.ok()?;
    let mut found = Vec::new();
    channel_renderers(&data, &mut found);
    Some(
        found
            .into_iter()
            .filter_map(|c| {
                // YouTube puts the handle where the subscribers were, and the subscribers where
                // the video count was.
                let texts = [runs(&c["subscriberCountText"]), runs(&c["videoCountText"])];
                let avatar = c["thumbnail"]["thumbnails"].as_array()?.last()?["url"].as_str().map(|u| {
                    if u.starts_with("//") { format!("https:{u}") } else { u.to_string() }
                });
                Some(Found {
                    id: c["channelId"].as_str()?.to_string(),
                    name: runs(&c["title"]),
                    handle: c["navigationEndpoint"]["browseEndpoint"]["canonicalBaseUrl"]
                        .as_str()
                        .and_then(|u| u.strip_prefix('/'))
                        .filter(|h| h.starts_with('@'))
                        .map(str::to_string),
                    subscribers: texts.into_iter().find(|t| t.contains("inscrit") || t.contains("subscriber")),
                    avatar,
                })
            })
            .collect(),
    )
}

/// YouTube channels named like `query`, the best match first.
pub async fn search(app: &AppHandle, query: &str) -> Result<Vec<Found>> {
    let html = http(app)
        .get(SEARCH)
        .query(&[("search_query", query), ("sp", CHANNELS_ONLY)])
        .header("User-Agent", UA)
        .header("Accept-Language", "pt-BR,pt;q=0.9")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    parse_search(&html).ok_or_else(|| Error::msg("YouTube's search page changed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(id: &str, title: &str, live: bool, viewers: u64, scheduled: Option<i64>) -> Stream {
        Stream { id: id.into(), title: title.into(), live, viewers, scheduled, since: None, thumb: Some(format!("https://i.ytimg.com/vi/{id}/hq720_live.jpg")) }
    }

    #[test]
    fn a_channels_streams_are_its_programmes() {
        let now = sports::Day { y: 2026, m: 10, d: 26 }.brt_ms(20, 30);
        let src = NewsSource { id: "UCjp".into(), name: "Jovem Pan News".into(), color: "#495057".into() };
        let streams = [
            stream("cam", "Jovem Pan News Av. Paulista | AO VIVO", true, 180, None),
            stream("show", "JORNAL JOVEM PAN - 26/10/26", true, 12_000, Some(now - 15 * MIN)),
            stream("next", "OS PINGOS NOS IS - 26/10/26", false, 3, Some(now + 60 * MIN)),
            stream("late", "3 EM 1 - 26/10/2026", false, 0, Some(now - 30 * MIN)),
        ];
        let c = channel(&src, &streams, now);
        assert_eq!((c.key.as_str(), c.group.as_str(), c.viewers), ("yt-UCjp", "news", Some(12_000)));
        assert_eq!(c.url.as_deref(), Some("https://www.youtube.com/tv#/watch?v=show"));
        assert!(c.frame.as_deref().unwrap().starts_with("https://i.ytimg.com/vi/show/hq720_live.jpg?t="));
        // The most watched is on, until the next scheduled one; one due before now isn't listed.
        let titles: Vec<&str> = c.slots.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["Jornal Jovem Pan", "Os Pingos Nos Is"]);
        assert_eq!(c.on_at(now).map(|s| s.end), Some(now + 60 * MIN));
        // Nothing live: nothing to play, only what's next.
        let off = channel(&src, &streams[2..], now);
        assert_eq!((off.on_at(now).is_none(), off.url, off.frame), (true, None, None));
    }

    #[test]
    fn channel_search() {
        let html = r#"<script>var ytInitialData = {"contents":{"items":[
          {"channelRenderer":{"channelId":"UCoa-D_VfMkFrCYodrOC9-mA","title":{"simpleText":"Band Jornalismo"},
            "thumbnail":{"thumbnails":[{"url":"//yt3.ggpht.com/a=s88"},{"url":"//yt3.ggpht.com/a=s176"}]},
            "subscriberCountText":{"simpleText":"@bandjornalismo"},"videoCountText":{"simpleText":"7,46 mi de inscritos"},
            "navigationEndpoint":{"browseEndpoint":{"canonicalBaseUrl":"/@bandjornalismo"}}}},
          {"channelRenderer":{"channelId":"UC2","title":{"simpleText":"No picture"},"thumbnail":{"thumbnails":[]}}}
        ]}};</script>"#;
        let found = parse_search(html).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0],
            Found {
                id: "UCoa-D_VfMkFrCYodrOC9-mA".into(),
                name: "Band Jornalismo".into(),
                handle: Some("@bandjornalismo".into()),
                subscribers: Some("7,46 mi de inscritos".into()),
                avatar: Some("https://yt3.ggpht.com/a=s176".into()),
            }
        );
        assert!(parse_search("<html></html>").is_none());
    }

    /// Against the real streams tabs: cargo test real_news -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_news() {
        tauri::async_runtime::block_on(async {
            let http = reqwest::Client::new();
            let now = now_ms();
            for src in defaults() {
                let html = http
                    .get(format!("{STREAMS}{}/streams", src.id))
                    .header("User-Agent", UA)
                    .header("Accept-Language", "pt-BR,pt;q=0.9")
                    .header("Cookie", crate::sports::cazetv::COOKIE)
                    .send()
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap();
                let Some(streams) = crate::sports::cazetv::parse(&html) else {
                    let at = html.find("var ytInitialData = ").map(|i| &html[i..(i + 300).min(html.len())]);
                    println!("{:20} NOT PARSED ({} bytes): {:?}", src.name, html.len(), at);
                    continue;
                };
                let c = channel(&src, &streams, now);
                let on = c.on_at(now).map(|s| format!("{:?} {} | {}", s.kind, s.title, s.sub));
                let next = c.after(now).map(|s| s.title.clone());
                println!("{:20} live={} viewers={:?} on={:?} next={:?}", c.name, c.url.is_some(), c.viewers, on, next);
                for s in streams.iter().take(4) {
                    println!("    raw: {} -> {}", s.title, tv::news_slot(s, None, now).title);
                }
            }
        });
    }
}
