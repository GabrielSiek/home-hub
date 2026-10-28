//! CazéTV's live and scheduled streams on YouTube.
//!
//! - Its channel's streams tab embeds the list as JSON (`ytInitialData`), each stream a
//!   `lockupViewModel` whose thumbnail badge says AO VIVO (live, with "176 mil assistindo") or Em
//!   breve (scheduled: "Programado para 26/10/2026, 12:30", in the viewer's time zone, which the
//!   PREF cookie sets to Brasília). Past streams are left out.
//! - With a YouTube Data API key, those videos' details come from `videos.list` (1 quota unit per
//!   50): live or scheduled, the real start time, viewers. If the page stops listing them, the
//!   channel's uploads playlist gives the ids instead, though its newest uploads miss streams
//!   scheduled long ago. A search (`search.list`, 100 units) every few minutes would use up the
//!   free 10,000 units a day.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::dates::{parse_hm, Day};
use crate::photos::rfc3339_ms;

pub const URL: &str = "https://www.youtube.com/channel/UCZiYbVptd3PVPf4f6eR6UaQ/streams";
/// Portuguese, in Brasília time.
pub const COOKIE: &str = "PREF=hl=pt-BR&gl=BR&tz=America.Sao_Paulo";
const API: &str = "https://www.googleapis.com/youtube/v3";
/// The channel's uploads (UC… → UU…).
const UPLOADS: &str = "UUZiYbVptd3PVPf4f6eR6UaQ";
/// The API key goes in this header rather than the address, so it stays out of logs and errors.
pub const KEY_HEADER: &str = "X-Goog-Api-Key";

pub fn uploads_url(page: Option<&str>) -> String {
    let token = page.map(|p| format!("&pageToken={p}")).unwrap_or_default();
    format!("{API}/playlistItems?part=contentDetails&maxResults=50&playlistId={UPLOADS}{token}")
}

/// Up to 50 videos' details.
pub fn videos_url(ids: &[String]) -> String {
    format!("{API}/videos?part=snippet,liveStreamingDetails&maxResults=50&id={}", ids.join(","))
}

/// The video ids on a page of the uploads playlist, and the next page's token.
pub fn parse_uploads(v: &Value) -> (Vec<String>, Option<String>) {
    let ids = v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["contentDetails"]["videoId"].as_str().map(str::to_string))
        .collect();
    (ids, v["nextPageToken"].as_str().map(str::to_string))
}

/// The live and scheduled ones among `videos.list`'s videos.
pub fn parse_videos(v: &Value) -> Vec<Stream> {
    v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| {
            let sn = &x["snippet"];
            let live = match sn["liveBroadcastContent"].as_str()? {
                "live" => true,
                "upcoming" => false,
                _ => return None,
            };
            let ls = &x["liveStreamingDetails"];
            let thumbs = &sn["thumbnails"];
            Some(Stream {
                id: x["id"].as_str()?.to_string(),
                title: sn["title"].as_str()?.to_string(),
                live,
                viewers: ls["concurrentViewers"].as_str().and_then(|n| n.parse().ok()).unwrap_or(0),
                scheduled: ls["scheduledStartTime"].as_str().and_then(rfc3339_ms),
                since: ls["actualStartTime"].as_str().and_then(rfc3339_ms),
                thumb: ["maxres", "standard", "high", "medium"]
                    .iter()
                    .find_map(|k| thumbs[k]["url"].as_str())
                    .map(str::to_string),
            })
        })
        .collect()
}

/// What the API says went wrong: "The request cannot be completed because you have exceeded
/// your quota."
pub fn api_error(v: &Value) -> Option<String> {
    v["error"]["message"].as_str().map(str::to_string)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stream {
    pub id: String,
    /// "AO VIVO: FLUMINENSE X INTERNACIONAL | BRASILEIRÃO 2026 | 33ª RODADA".
    pub title: String,
    pub live: bool,
    /// Watching now, or waiting for it to start.
    pub viewers: u64,
    /// When a scheduled stream starts, unix ms.
    pub scheduled: Option<i64>,
    /// When the launcher first saw it live (or its schedule said it would start), unix ms.
    #[serde(default)]
    pub since: Option<i64>,
    pub thumb: Option<String>,
}

/// Every string under a key named `name`.
fn strings<'a>(v: &'a Value, name: &str, out: &mut Vec<&'a str>) {
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                match x {
                    Value::String(s) if k == name => out.push(s),
                    _ => strings(x, name, out),
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| strings(x, name, out)),
        _ => {}
    }
}

fn lockups<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(o) => {
            if let Some(l) = o.get("lockupViewModel") {
                out.push(l);
                return;
            }
            o.values().for_each(|x| lockups(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| lockups(x, out)),
        _ => {}
    }
}

/// "176 mil assistindo" → 176000, "4,4 mil" → 4400, "1,2 mi" → 1200000.
fn count(text: &str) -> u64 {
    let mut words = text.split_whitespace();
    let Some(n) = words.next().and_then(|n| n.replace('.', "").replace(',', ".").parse::<f64>().ok()) else {
        return 0;
    };
    let k = match words.next() {
        Some("mil") => 1e3,
        Some("mi") => 1e6,
        _ => 1.0,
    };
    (n * k).round() as u64
}

/// "Programado para 04/11/2026, 14:30", in Brasília time.
fn scheduled(text: &str) -> Option<i64> {
    let rest = text.split_once("para ")?.1;
    let (date, time) = rest.split_once(", ")?;
    let mut d = date.trim().split('/').map(|x| x.parse::<i64>().ok());
    let day = Day { d: d.next()??, m: d.next()??, y: d.next()?? };
    let (h, m) = parse_hm(time)?;
    Some(day.brt_ms(h, m))
}

pub fn watch_url(id: &str) -> String {
    format!("https://www.youtube.com/watch?v={id}")
}

/// When a live stream started, from its watch page's `liveBroadcastDetails`.
pub fn started(html: &str) -> Option<i64> {
    let at = html.find("\"liveBroadcastDetails\"")?;
    let rest = &html[at..];
    let ts = rest.split_once("\"startTimestamp\":\"")?.1.split('"').next()?;
    rfc3339_ms(ts)
}

/// The live and scheduled streams on the page. None when it isn't the page it was.
pub fn parse(html: &str) -> Option<Vec<Stream>> {
    let marker = "var ytInitialData = ";
    let at = html.find(marker)? + marker.len();
    let data: Value = serde_json::Deserializer::from_str(&html[at..]).into_iter::<Value>().next()?.ok()?;
    let mut found = Vec::new();
    lockups(&data, &mut found);
    let streams = found
        .into_iter()
        .filter_map(|l| {
            let id = l["contentId"].as_str()?.to_string();
            let title = l["metadata"]["lockupMetadataViewModel"]["title"]["content"].as_str()?.to_string();
            let mut styles = Vec::new();
            strings(&l["contentImage"], "badgeStyle", &mut styles);
            let live = styles.iter().any(|s| s.ends_with("_LIVE"));
            let mut rows = Vec::new();
            strings(&l["metadata"], "content", &mut rows);
            let when = rows.iter().find_map(|r| r.starts_with("Programado").then(|| scheduled(r)).flatten());
            // Neither live nor coming: a past stream.
            if !live && when.is_none() {
                return None;
            }
            let viewers = rows
                .iter()
                .find(|r| r.ends_with("assistindo") || r.ends_with("esperando"))
                .map_or(0, |r| count(r));
            // The thumbnail's sizes, smallest first.
            let mut thumbs = Vec::new();
            strings(&l["contentImage"], "url", &mut thumbs);
            Some(Stream {
                id,
                title,
                live,
                viewers,
                scheduled: when,
                since: None,
                thumb: thumbs.into_iter().rfind(|u| u.contains("i.ytimg.com")).map(str::to_string),
            })
        })
        .collect();
    Some(streams)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lockup(id: &str, title: &str, style: &str, rows: &[&str]) -> String {
        let rows: Vec<String> = rows
            .iter()
            .map(|r| format!(r#"{{"metadataParts":[{{"text":{{"content":"{r}"}}}}]}}"#))
            .collect();
        format!(
            r#"{{"richItemRenderer":{{"content":{{"lockupViewModel":{{"contentId":"{id}","contentImage":{{"thumbnailViewModel":{{"image":{{"sources":[{{"url":"https://i.ytimg.com/vi/{id}/hq720.jpg"}}]}},"overlays":[{{"thumbnailBottomOverlayViewModel":{{"badges":[{{"thumbnailBadgeViewModel":{{"text":"x","badgeStyle":"{style}"}}}}]}}}}]}}}},"metadata":{{"lockupMetadataViewModel":{{"title":{{"content":"{title}"}},"metadata":{{"contentMetadataViewModel":{{"metadataRows":[{}]}}}}}}}}}}}}}}}}"#,
            rows.join(",")
        )
    }

    #[test]
    fn streams_tab() {
        let items = [
            lockup("live1", "AO VIVO: BARCELONA X REAL MADRID | LALIGA 26/27 | 10ª RODADA", "THUMBNAIL_OVERLAY_BADGE_STYLE_LIVE", &["176 mil assistindo"]),
            lockup("soon1", "AO VIVO: FLUMINENSE X INTERNACIONAL | BRASILEIRÃO 2026 | 33ª RODADA", "THUMBNAIL_OVERLAY_BADGE_STYLE_DEFAULT", &["17 esperando", "Programado para 28/10/2026, 15:30"]),
            lockup("past1", "JOGO COMPLETO: NAPOLI X ROMA | SÉRIE A ITALIANA 26/27", "THUMBNAIL_OVERLAY_BADGE_STYLE_DEFAULT", &["2 mi", "Transmitido há 1 dia"]),
        ];
        let html = format!(r#"<script>var ytInitialData = {{"contents":{{"items":[{}]}}}};</script>"#, items.join(","));
        let s = parse(&html).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].id.as_str(), s[0].live, s[0].viewers), ("live1", true, 176_000));
        assert_eq!(s[0].thumb.as_deref(), Some("https://i.ytimg.com/vi/live1/hq720.jpg"));
        assert!(!s[1].live);
        assert_eq!(s[1].scheduled, Some(Day { y: 2026, m: 10, d: 28 }.brt_ms(15, 30)));
        assert_eq!(count("4,4 mil assistindo"), 4400);
        assert_eq!(count("1,2 mi"), 1_200_000);
        assert_eq!(count("635 assistindo"), 635);
        assert!(parse("<html></html>").is_none());
        let watch = r#"..."microformat":{"liveBroadcastDetails":{"isLiveNow":true,"startTimestamp":"2026-10-25T17:29:12+00:00"}}..."#;
        assert_eq!(started(watch), rfc3339_ms("2026-10-25T17:29:12Z"));
    }

    #[test]
    fn data_api_answers() {
        let v: Value = serde_json::from_str(
            r#"{"items":[
              {"id":"UEV1NElLuMA","snippet":{"title":"AO VIVO: BARCELONA X REAL MADRID | LALIGA 26/27 | 10ª RODADA","liveBroadcastContent":"live",
                "thumbnails":{"high":{"url":"https://i.ytimg.com/vi/UEV1NElLuMA/hqdefault_live.jpg"},"maxres":{"url":"https://i.ytimg.com/vi/UEV1NElLuMA/maxresdefault_live.jpg"}}},
               "liveStreamingDetails":{"actualStartTime":"2026-10-25T17:29:12Z","scheduledStartTime":"2026-10-25T17:30:00Z","concurrentViewers":"243247"}},
              {"id":"t4pf4qZ0c14","snippet":{"title":"AO VIVO: FLUMINENSE X INTERNACIONAL","liveBroadcastContent":"upcoming","thumbnails":{}},
               "liveStreamingDetails":{"scheduledStartTime":"2026-10-28T18:30:00Z"}},
              {"id":"jZXkDVmpoU0","snippet":{"title":"JOGO COMPLETO","liveBroadcastContent":"none","thumbnails":{}}}
            ]}"#,
        )
        .unwrap();
        let s = parse_videos(&v);
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].live, s[0].viewers), (true, 243_247));
        assert_eq!(s[0].since, rfc3339_ms("2026-10-25T17:29:12Z"));
        assert_eq!(s[0].thumb.as_deref(), Some("https://i.ytimg.com/vi/UEV1NElLuMA/maxresdefault_live.jpg"));
        assert_eq!((s[1].live, s[1].scheduled), (false, rfc3339_ms("2026-10-28T18:30:00Z")));

        let page: Value = serde_json::from_str(r#"{"nextPageToken":"EAAa","items":[{"contentDetails":{"videoId":"a"}},{"contentDetails":{"videoId":"b"}}]}"#).unwrap();
        assert_eq!(parse_uploads(&page), (vec!["a".to_string(), "b".to_string()], Some("EAAa".to_string())));
        assert!(!uploads_url(Some("EAAa")).contains("key="));
        let quota: Value = serde_json::from_str(r#"{"error":{"code":403,"message":"The request cannot be completed because you have exceeded your quota."}}"#).unwrap();
        assert!(api_error(&quota).unwrap().contains("quota"));
    }
}
