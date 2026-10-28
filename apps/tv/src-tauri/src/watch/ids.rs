//! The streaming services' own ids for a TMDB title, from Wikidata, which links both.
//! They make the Play button open the title itself instead of the service's search.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::history::now_ms;

const SPARQL: &str = "https://query.wikidata.org/sparql";
/// Wikidata asks every client to name itself.
const USER_AGENT: &str = "TVLauncher/0.1 (personal TV launcher; links TMDB titles to streaming apps)";

/// Looked-up ids are kept this long; titles with none are asked again sooner, as Wikidata grows.
const KEEP_MS: i64 = 30 * 24 * 3600 * 1000;
const KEEP_EMPTY_MS: i64 = 7 * 24 * 3600 * 1000;

/// Wikidata property → the field it fills.
const PROPS: [(&str, &str); 9] = [
    ("P1874", "netflix"),
    ("P8298", "hbomax"),
    ("P13902", "disney_browse"),
    ("P7595", "disney_movie"),
    ("P7596", "disney_series"),
    ("P14440", "prime"),
    ("P14462", "prime_gti"),
    ("P8055", "amazon"),
    ("P12356", "globoplay"),
];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ServiceIds {
    /// Field name → the ids Wikidata has (a title can have several items: the film, the franchise).
    #[serde(default)]
    pub ids: HashMap<String, Vec<String>>,
    pub fetched_at: i64,
}

impl ServiceIds {
    fn first(&self, field: &str) -> Option<&str> {
        self.ids.get(field).and_then(|v| v.first()).map(String::as_str)
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Cache {
    #[serde(default)]
    titles: HashMap<String, ServiceIds>,
}

fn cache_path(dir: &Path) -> std::path::PathBuf {
    dir.join("service_ids.json")
}

fn read_cache(dir: &Path) -> Cache {
    fs::read_to_string(cache_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub(super) fn write_cache(dir: &Path, key: String, ids: &ServiceIds) {
    let mut cache = read_cache(dir);
    cache.titles.insert(key, ids.clone());
    if let Ok(text) = serde_json::to_string(&cache) {
        let _ = fs::write(cache_path(dir), text);
    }
}

/// The cached entry `key`, while it is fresh: `keep` ms when it has ids, `keep_empty` when not.
pub(super) fn cached(dir: &Path, key: &str, keep: i64, keep_empty: i64) -> Option<ServiceIds> {
    let hit = read_cache(dir).titles.get(key).cloned()?;
    let keep = if hit.ids.is_empty() { keep_empty } else { keep };
    (now_ms() - hit.fetched_at < keep).then_some(hit)
}

/// The services' ids for a TMDB movie or series; empty when Wikidata can't be reached.
pub async fn lookup(http: &reqwest::Client, dir: &Path, media_type: &str, tmdb_id: u64) -> ServiceIds {
    let key = format!("{media_type}:{tmdb_id}");
    let now = now_ms();
    if let Some(hit) = cached(dir, &key, KEEP_MS, KEEP_EMPTY_MS) {
        return hit;
    }
    let Ok(found) = query(http, media_type, tmdb_id).await else {
        // Offline or Wikidata busy: play through the search this time, ask again next time.
        return ServiceIds::default();
    };
    let ids = ServiceIds { ids: found, fetched_at: now };
    write_cache(dir, key, &ids);
    ids
}

/// The TMDB movie or series ("movie"/"tv", id) that a service's own id (Wikidata property
/// `prop`, e.g. P1874 for Netflix) belongs to. None when Wikidata doesn't link them.
pub async fn reverse(http: &reqwest::Client, dir: &Path, prop: &str, value: &str) -> Option<(String, u64)> {
    // Only ids as the services write them go into the query.
    if value.is_empty() || !value.chars().all(|c| c.is_ascii_alphanumeric() || "._:/-".contains(c)) {
        return None;
    }
    let key = format!("rev:{prop}:{value}");
    let now = now_ms();
    let hit = read_cache(dir).titles.get(&key).cloned();
    let ids = match hit {
        Some(h) if now - h.fetched_at < if h.ids.is_empty() { KEEP_EMPTY_MS } else { KEEP_MS } => h,
        _ => {
            let query = format!(
                "SELECT ?p ?v WHERE {{ ?item wdt:{prop} \"{value}\" . VALUES ?p {{ wdt:P4983 wdt:P4947 }} ?item ?p ?v . }}"
            );
            let found = run(http, query).await.ok()?;
            let ids = ServiceIds { ids: parse_tmdb(&found), fetched_at: now };
            write_cache(dir, key, &ids);
            ids
        }
    };
    let pick = |field: &str, media: &str| {
        ids.first(field)
            .and_then(|v| v.parse::<u64>().ok())
            .map(|id| (media.to_string(), id))
    };
    pick("tmdb_tv", "tv").or_else(|| pick("tmdb_movie", "movie"))
}

fn parse_tmdb(body: &Value) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for row in body["results"]["bindings"].as_array().cloned().unwrap_or_default() {
        let (Some(p), Some(v)) = (row["p"]["value"].as_str(), row["v"]["value"].as_str()) else {
            continue;
        };
        let field = match p.rsplit('/').next() {
            Some("P4983") => "tmdb_tv",
            Some("P4947") => "tmdb_movie",
            _ => continue,
        };
        out.entry(field.to_string()).or_default().push(v.to_string());
    }
    out
}

async fn query(http: &reqwest::Client, media_type: &str, tmdb_id: u64) -> Result<HashMap<String, Vec<String>>, reqwest::Error> {
    Ok(parse(&run(http, sparql(media_type, tmdb_id)).await?))
}

async fn run(http: &reqwest::Client, query: String) -> Result<Value, reqwest::Error> {
    http.get(SPARQL)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/sparql-results+json")
        .query(&[("query", query), ("format", "json".into())])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
}

fn sparql(media_type: &str, tmdb_id: u64) -> String {
    let tmdb_prop = if media_type == "tv" { "P4983" } else { "P4947" };
    let props: Vec<String> = PROPS.iter().map(|(p, _)| format!("wdt:{p}")).collect();
    format!(
        "SELECT ?p ?v WHERE {{ ?item wdt:{tmdb_prop} \"{tmdb_id}\" . VALUES ?p {{ {} }} ?item ?p ?v . }}",
        props.join(" ")
    )
}

fn parse(body: &Value) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    let rows = body["results"]["bindings"].as_array().cloned().unwrap_or_default();
    for row in rows {
        let (Some(p), Some(v)) = (row["p"]["value"].as_str(), row["v"]["value"].as_str()) else {
            continue;
        };
        let prop = p.rsplit('/').next().unwrap_or_default();
        if let Some((_, field)) = PROPS.iter().find(|(id, _)| *id == prop) {
            let list = out.entry(field.to_string()).or_default();
            if !list.iter().any(|x| x == v) {
                list.push(v.to_string());
            }
        }
    }
    out
}

/// The id a service's own address for a title holds, as the field it fills ("netflix", ...).
/// None for anything else: a search, an episode's player, another service's page.
pub fn from_url(service: &str, media_type: &str, url: &str) -> Option<(&'static str, String)> {
    let url = reqwest::Url::parse(url).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    let on = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
    let parts: Vec<&str> = url.path_segments()?.filter(|p| !p.is_empty()).collect();
    let id_like = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    match (service, parts.as_slice()) {
        ("netflix", ["title", id, ..]) if on("netflix.com") && id.chars().all(|c| c.is_ascii_digit()) => {
            Some(("netflix", id.to_string()))
        }
        // A film's player has the film's own id; a series' has an episode's.
        ("netflix", ["watch", id, ..]) if media_type == "movie" && on("netflix.com") && id.chars().all(|c| c.is_ascii_digit()) => {
            Some(("netflix", id.to_string()))
        }
        ("hbomax", [kind @ ("show" | "movie" | "series" | "mini-series" | "feature"), id, ..])
            if (on("hbomax.com") || on("max.com")) && id_like(id) =>
        {
            Some(("hbomax", format!("{kind}/{id}")))
        }
        // A film's player is the film; a series' is one of its episodes.
        ("hbomax", ["video", "watch", id, ..]) if media_type == "movie" && (on("hbomax.com") || on("max.com")) && id_like(id) => {
            Some(("hbomax", format!("video/watch/{id}")))
        }
        ("disney", ["browse", id, ..]) if on("disneyplus.com") && id.starts_with("entity-") && id_like(id) => {
            Some(("disney_browse", id.to_string()))
        }
        ("prime", _) if on("primevideo.com") || on("amazon.com") || on("amazon.com.br") => url
            .query_pairs()
            .find(|(k, _)| k == "gti")
            .map(|(_, v)| v.into_owned())
            .or_else(|| match parts.as_slice() {
                [.., "detail", id] | [.., "detail", id, _] => Some(id.to_string()),
                _ => None,
            })
            .filter(|id| id_like(id))
            .map(|id| (if id.starts_with("amzn1.dv.gti.") { "prime_gti" } else { "amazon" }, id)),
        ("globoplay", [.., "t", id]) if on("globo.com") && id_like(id) => Some(("globoplay", id.to_string())),
        _ => None,
    }
}

/// Ids the search fallback found on the service itself (watch.js reports the title's page).
fn learned_key(media_type: &str, tmdb_id: u64) -> String {
    format!("learned:{media_type}:{tmdb_id}")
}

/// Kept for good: a service's id for a title doesn't change, and this one played.
pub fn learned(dir: &Path, media_type: &str, tmdb_id: u64) -> ServiceIds {
    cached(dir, &learned_key(media_type, tmdb_id), i64::MAX, 0).unwrap_or_default()
}

/// Keeps the id in `url`, the page a search on `service` led to, for the next Play.
pub fn learn(dir: &Path, service: &str, media_type: &str, tmdb_id: u64, url: &str) -> bool {
    let Some((field, id)) = from_url(service, media_type, url) else {
        return false;
    };
    let mut ids = learned(dir, media_type, tmdb_id);
    ids.ids.insert(field.to_string(), vec![id]);
    ids.fetched_at = now_ms();
    write_cache(dir, learned_key(media_type, tmdb_id), &ids);
    true
}

/// The address in `trail` (the tab's way from the search to the video, oldest first) that
/// names the title, nearest the video first: a film's own player, else its page.
fn trail_title(service: &str, media_type: &str, trail: &[String]) -> Option<String> {
    trail
        .iter()
        .rev()
        .map(|url| netflix_preview(url).unwrap_or_else(|| url.clone()))
        .find(|url| from_url(service, media_type, url).is_some())
}

/// Netflix's search opens a result as a dialog over the results (jbv=): that title's page.
fn netflix_preview(url: &str) -> Option<String> {
    let url = reqwest::Url::parse(url).ok()?;
    let host = url.host_str()?;
    if host != "netflix.com" && !host.ends_with(".netflix.com") {
        return None;
    }
    let id = url.query_pairs().find(|(k, _)| k == "jbv")?.1;
    id.chars().all(|c| c.is_ascii_digit()).then(|| format!("https://www.netflix.com/title/{id}"))
}

/// Keeps the title's address from `trail`, once its video played, for the next Play. The address
/// kept, if any.
pub fn learn_trail(dir: &Path, service: &str, media_type: &str, tmdb_id: u64, trail: &[String]) -> Option<String> {
    let url = trail_title(service, media_type, trail)?;
    learn(dir, service, media_type, tmdb_id, &url).then_some(url)
}

/// What Play opens: the title's own page or player, when the service's id is known.
/// `episode`: a particular episode was picked, so the page has to show the episode list
/// (Netflix's player would start wherever the account left off instead).
pub fn deep_link(service: &str, media_type: &str, ids: &ServiceIds, episode: bool) -> Option<String> {
    let series = media_type == "tv";
    match service {
        "netflix" => ids.first("netflix").map(|id| {
            if series && episode {
                format!("https://www.netflix.com/title/{id}")
            } else {
                format!("https://www.netflix.com/watch/{id}")
            }
        }),
        "hbomax" => ids
            .first("hbomax")
            .map(|path| format!("https://play.hbomax.com/{}", path.trim_start_matches('/'))),
        "disney" => ids
            .first("disney_browse")
            .map(|id| format!("https://www.disneyplus.com/browse/{id}"))
            .or_else(|| {
                if series {
                    ids.first("disney_series").map(|id| format!("https://www.disneyplus.com/series/wp/{id}"))
                } else {
                    ids.first("disney_movie").map(|id| format!("https://www.disneyplus.com/movies/wd/{id}"))
                }
            }),
        "prime" => ids
            .first("prime")
            .or_else(|| ids.first("prime_gti"))
            .or_else(|| ids.first("amazon"))
            .map(|id| format!("https://www.primevideo.com/detail/{id}")),
        "globoplay" => ids
            .first("globoplay")
            .map(|id| format!("https://globoplay.globo.com/-/t/{id}/")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ids(pairs: &[(&str, &str)]) -> ServiceIds {
        let mut s = ServiceIds::default();
        for (k, v) in pairs {
            s.ids.entry(k.to_string()).or_default().push(v.to_string());
        }
        s
    }

    #[test]
    fn query_names_the_right_tmdb_property() {
        let q = sparql("tv", 100088);
        assert!(q.contains("wdt:P4983 \"100088\""));
        assert!(q.contains("wdt:P1874") && q.contains("wdt:P12356"));
        assert!(sparql("movie", 1).contains("wdt:P4947 \"1\""));
    }

    #[test]
    fn parses_bindings_and_dedupes() {
        let body = json!({ "results": { "bindings": [
            { "p": { "value": "http://www.wikidata.org/prop/direct/P8298" }, "v": { "value": "show/93ba22b1" } },
            { "p": { "value": "http://www.wikidata.org/prop/direct/P1874" }, "v": { "value": "81435684" } },
            { "p": { "value": "http://www.wikidata.org/prop/direct/P1874" }, "v": { "value": "81435684" } },
            { "p": { "value": "http://www.wikidata.org/prop/direct/P9999" }, "v": { "value": "x" } }
        ] } });
        let got = parse(&body);
        assert_eq!(got["hbomax"], vec!["show/93ba22b1"]);
        assert_eq!(got["netflix"], vec!["81435684"]);
        assert_eq!(got.len(), 2);
        assert!(parse(&json!({})).is_empty());
    }

    #[test]
    fn parses_tmdb_ids() {
        let body = json!({ "results": { "bindings": [
            { "p": { "value": "http://www.wikidata.org/prop/direct/P4983" }, "v": { "value": "66732" } },
            { "p": { "value": "http://www.wikidata.org/prop/direct/P1874" }, "v": { "value": "x" } }
        ] } });
        let got = parse_tmdb(&body);
        assert_eq!(got["tmdb_tv"], vec!["66732"]);
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn links_per_service() {
        let nf = ids(&[("netflix", "81435684")]);
        assert_eq!(deep_link("netflix", "tv", &nf, false).unwrap(), "https://www.netflix.com/watch/81435684");
        assert_eq!(deep_link("netflix", "tv", &nf, true).unwrap(), "https://www.netflix.com/title/81435684");
        assert_eq!(deep_link("netflix", "movie", &nf, true).unwrap(), "https://www.netflix.com/watch/81435684");
        assert_eq!(
            deep_link("hbomax", "tv", &ids(&[("hbomax", "show/93ba")]), false).unwrap(),
            "https://play.hbomax.com/show/93ba"
        );
        assert_eq!(
            deep_link("disney", "tv", &ids(&[("disney_series", "abc123def456")]), false).unwrap(),
            "https://www.disneyplus.com/series/wp/abc123def456"
        );
        assert_eq!(
            deep_link("disney", "movie", &ids(&[("disney_movie", "m"), ("disney_browse", "entity-1")]), false).unwrap(),
            "https://www.disneyplus.com/browse/entity-1"
        );
        assert_eq!(
            deep_link("prime", "tv", &ids(&[("amazon", "B07QQQ52B3")]), false).unwrap(),
            "https://www.primevideo.com/detail/B07QQQ52B3"
        );
        assert_eq!(
            deep_link("globoplay", "tv", &ids(&[("globoplay", "12345")]), false).unwrap(),
            "https://globoplay.globo.com/-/t/12345/"
        );
        assert_eq!(deep_link("netflix", "tv", &ServiceIds::default(), false), None);
        assert_eq!(deep_link("youtube", "tv", &nf, false), None);
    }

    #[test]
    fn reads_ids_from_service_addresses() {
        let got = from_url;
        assert_eq!(got("netflix", "movie", "https://www.netflix.com/title/81957166"), Some(("netflix", "81957166".into())));
        // A series' player link is an episode's id, and a search is no title.
        assert_eq!(got("netflix", "tv", "https://www.netflix.com/watch/80077368"), None);
        assert_eq!(got("netflix", "movie", "https://www.netflix.com/search?q=Sinners&jbv=81957166"), None);
        assert_eq!(
            got("hbomax", "movie", "https://play.hbomax.com/show/2a072173-2bac?utm_source=universal_search"),
            Some(("hbomax", "show/2a072173-2bac".into()))
        );
        assert_eq!(
            got("hbomax", "movie", "https://play.hbomax.com/video/watch/d43bd66a"),
            Some(("hbomax", "video/watch/d43bd66a".into()))
        );
        assert_eq!(got("hbomax", "tv", "https://play.hbomax.com/video/watch/c07ae042"), None);
        assert_eq!(got("hbomax", "movie", "https://app.primevideo.com/detail?gti=amzn1.dv.gti.41e4"), None);
        assert_eq!(
            got("disney", "tv", "https://www.disneyplus.com/browse/entity-05eb6a8e"),
            Some(("disney_browse", "entity-05eb6a8e".into()))
        );
        assert_eq!(
            got("prime", "tv", "https://app.primevideo.com/detail?gti=amzn1.dv.gti.9ab600aa"),
            Some(("prime_gti", "amzn1.dv.gti.9ab600aa".into()))
        );
        assert_eq!(
            got("globoplay", "tv", "https://globoplay.globo.com/sob-pressao/t/PhQ7Wtzn6C/"),
            Some(("globoplay", "PhQ7Wtzn6C".into()))
        );
        assert_eq!(got("globoplay", "tv", "https://globoplay.globo.com/v/6002976"), None);
        assert_eq!(got("netflix", "movie", "https://evil.example/title/1"), None);
        assert_eq!(got("netflix", "movie", "https://www.netflix.com/watch/81957166?trackId=1"), Some(("netflix", "81957166".into())));
    }

    #[test]
    fn reads_the_title_from_the_way_to_the_video() {
        let trail = |urls: &[&str]| urls.iter().map(|u| u.to_string()).collect::<Vec<_>>();
        // HBO Max: search, the film's page, its player. The player is nearest the video.
        let max = trail(&[
            "https://play.hbomax.com/search?q=Sinners",
            "https://play.hbomax.com/movie/2a072173-2bac",
            "https://play.hbomax.com/video/watch/d43bd66a/e1",
        ]);
        assert_eq!(trail_title("hbomax", "movie", &max).unwrap(), "https://play.hbomax.com/video/watch/d43bd66a/e1");
        // A series' player is an episode: the show's page before it.
        let show = trail(&[
            "https://play.hbomax.com/search?q=The%20Pitt",
            "https://play.hbomax.com/show/93ba22b1",
            "https://play.hbomax.com/video/watch/c07ae042",
        ]);
        assert_eq!(trail_title("hbomax", "tv", &show).unwrap(), "https://play.hbomax.com/show/93ba22b1");
        // Netflix's result dialog, then an episode's player.
        let nf = trail(&["https://www.netflix.com/search?q=Dark&jbv=80100172", "https://www.netflix.com/watch/80083977"]);
        assert_eq!(trail_title("netflix", "tv", &nf).unwrap(), "https://www.netflix.com/title/80100172");
        assert_eq!(trail_title("hbomax", "movie", &trail(&["https://play.hbomax.com/search?q=x"])), None);
        assert_eq!(netflix_preview("https://evil.example/search?jbv=1"), None);
    }
}
