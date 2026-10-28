//! The streaming services' own ids for a title, from JustWatch: the source of TMDB's "where to
//! watch", and it knows the address each service has for the title in the region. Its API is
//! the one justwatch.com itself runs on (unofficial), so an answer that doesn't read as expected
//! just means no ids from here, and Play falls back to Wikidata's.

use std::collections::HashMap;
use std::path::Path;

use serde_json::{json, Value};

use super::ids::{self, ServiceIds};
use crate::config::{AppEntry, Settings};
use crate::history::now_ms;
use crate::tmdb::{self, Provider};

const API: &str = "https://apis.justwatch.com/graphql";
const USER_AGENT: &str = "TVLauncher/0.1 (personal TV launcher; links TMDB titles to streaming apps)";

/// Ids stay put; a title nobody streams yet is asked again sooner.
const KEEP_MS: i64 = 30 * 24 * 3600 * 1000;
const KEEP_EMPTY_MS: i64 = 24 * 3600 * 1000;

const QUERY: &str = "query TitleOffers($country: Country!, $language: Language!, $first: Int!, $filter: TitleFilter) {
  popularTitles(country: $country, first: $first, filter: $filter) {
    edges { node {
      objectType
      content(country: $country, language: $language) { externalIds { tmdbId } }
      offers(country: $country, platform: WEB) {
        standardWebURL
        deeplinkURL(platform: WEB)
        package { packageId clearName }
      }
    } }
  }
}";

/// The ids JustWatch has for TMDB title `tmdb_id` (named `title`) on the user's apps, in the
/// settings' region. Empty when JustWatch can't be reached or doesn't list it.
pub async fn lookup(
    http: &reqwest::Client,
    dir: &Path,
    s: &Settings,
    apps: &[AppEntry],
    media_type: &str,
    tmdb_id: u64,
    title: &str,
) -> ServiceIds {
    let country = s.region.to_uppercase();
    let key = format!("jw:{country}:{media_type}:{tmdb_id}");
    if let Some(hit) = ids::cached(dir, &key, KEEP_MS, KEEP_EMPTY_MS) {
        return hit;
    }
    let language = s.language.split(['-', '_']).next().unwrap_or("en").to_lowercase();
    let Ok(body) = search(http, &country, &language, title).await else {
        // Offline or refused: Wikidata's ids this time, ask again next time.
        return ServiceIds::default();
    };
    let ids = ServiceIds { ids: parse(&body, apps, media_type, tmdb_id), fetched_at: now_ms() };
    ids::write_cache(dir, key, &ids);
    ids
}

async fn search(http: &reqwest::Client, country: &str, language: &str, title: &str) -> Result<Value, reqwest::Error> {
    let body = json!({
        "query": QUERY,
        "variables": { "country": country, "language": language, "first": 10, "filter": { "searchQuery": title } },
    });
    http.post(API)
        .header("User-Agent", USER_AGENT)
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
}

/// The ids in the offers of the result that is this TMDB title, by the field each fills.
/// An offer belongs to an app the way TMDB's providers do (JustWatch's package ids are TMDB's),
/// and only an address on that app's own site counts: "HBO Max Amazon Channel" is on Prime Video.
fn parse(body: &Value, apps: &[AppEntry], media_type: &str, tmdb_id: u64) -> HashMap<String, Vec<String>> {
    let object = if media_type == "tv" { "SHOW" } else { "MOVIE" };
    let tmdb = tmdb_id.to_string();
    let edges = body["data"]["popularTitles"]["edges"].as_array().cloned().unwrap_or_default();
    let Some(node) = edges
        .iter()
        .map(|e| &e["node"])
        .find(|n| n["objectType"] == object && n["content"]["externalIds"]["tmdbId"].as_str() == Some(&tmdb))
    else {
        return HashMap::new();
    };
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for offer in node["offers"].as_array().into_iter().flatten() {
        let package = &offer["package"];
        let Some(provider_id) = package["packageId"].as_u64() else {
            continue;
        };
        let provider = Provider {
            provider_id: provider_id as u32,
            provider_name: package["clearName"].as_str().unwrap_or_default().to_string(),
        };
        for app in tmdb::match_apps(&[provider], apps) {
            // The title's page before its player.
            for url in [&offer["standardWebURL"], &offer["deeplinkURL"]].into_iter().filter_map(Value::as_str) {
                if let Some((field, id)) = ids::from_url(&app, media_type, url) {
                    let list = out.entry(field.to_string()).or_default();
                    if !list.contains(&id) {
                        list.push(id);
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_apps;

    fn apps() -> Vec<AppEntry> {
        parse_apps(include_str!("../../defaults/apps.toml")).unwrap()
    }

    fn offer(id: u64, name: &str, page: &str, play: &str) -> Value {
        json!({ "standardWebURL": page, "deeplinkURL": play, "package": { "packageId": id, "clearName": name } })
    }

    // Trimmed from JustWatch's answer for "Sinners" in Brazil.
    fn sinners() -> Value {
        json!({ "data": { "popularTitles": { "edges": [
            { "node": { "objectType": "MOVIE", "content": { "externalIds": { "tmdbId": "1512939" } },
                "offers": [offer(8, "Netflix", "https://www.netflix.com/title/111", "https://www.netflix.com/watch/111")] } },
            { "node": { "objectType": "MOVIE", "content": { "externalIds": { "tmdbId": "1233413" } }, "offers": [
                offer(8, "Netflix", "https://www.netflix.com/title/81957166", "https://www.netflix.com/watch/81957166"),
                offer(1796, "Netflix basic with Ads", "https://www.netflix.com/title/81957166", "https://www.netflix.com/watch/81957166"),
                offer(1899, "HBO Max",
                    "https://play.hbomax.com/show/2a072173-2bac-43ba-9933-10eba021ed96?utm_source=universal_search",
                    "https://play.hbomax.com/video/watch/d43bd66a-a83c-40eb-890f-5b14eb9ef7c9?utm_source=universal_search"),
                offer(1825, "HBO Max Amazon Channel",
                    "https://app.primevideo.com/detail?gti=amzn1.dv.gti.41e46ee6",
                    "https://app.primevideo.com/watch?gti=amzn1.dv.gti.41e46ee6"),
            ] } }
        ] } } })
    }

    #[test]
    fn picks_the_tmdb_title_and_reads_its_ids() {
        let got = parse(&sinners(), &apps(), "movie", 1233413);
        assert_eq!(got["netflix"], vec!["81957166"]);
        assert_eq!(
            got["hbomax"],
            vec!["show/2a072173-2bac-43ba-9933-10eba021ed96", "video/watch/d43bd66a-a83c-40eb-890f-5b14eb9ef7c9"]
        );
        // The HBO channel on Prime Video is neither HBO Max's address nor a Prime Video offer.
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn nothing_for_another_title_or_kind() {
        assert!(parse(&sinners(), &apps(), "movie", 1).is_empty());
        assert!(parse(&sinners(), &apps(), "tv", 1233413).is_empty());
        assert!(parse(&json!({ "errors": [] }), &apps(), "movie", 1233413).is_empty());
    }
}
