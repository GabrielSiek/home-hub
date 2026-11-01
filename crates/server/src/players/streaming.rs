//! The streaming services on a Google TV, and where a title is in them. Each service's own app
//! plays it, signed in on the TV: the server only says what to open.
//!
//! Titles come from JustWatch, which knows each service's address for a title in the region. Its
//! API is the one justwatch.com itself runs on (unofficial), as on the TV PCs.

use serde::Serialize;
use serde_json::{json, Value};
use tvl_kit::{Error, Result};

use crate::ctx::Ctx;

const API: &str = "https://apis.justwatch.com/graphql";
const IMAGES: &str = "https://images.justwatch.com";
const USER_AGENT: &str = "TVLauncher/0.1 (personal TV launcher; links TMDB titles to streaming apps)";
const RESULTS: u32 = 12;
/// Included in a subscription, or free. Rentals and purchases open a page asking to pay.
const WATCHABLE: [&str; 3] = ["FLATRATE", "FREE", "ADS"];

const QUERY: &str = "query Titles($country: Country!, $language: Language!, $first: Int!, $filter: TitleFilter) {
  popularTitles(country: $country, first: $first, filter: $filter) {
    edges { node {
      objectType
      content(country: $country, language: $language) { title originalReleaseYear posterUrl }
      offers(country: $country, platform: WEB) { standardWebURL monetizationType }
    } }
  }
}";

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Service {
    pub id: &'static str,
    pub name: &'static str,
    /// The app on Android TV; opening it needs nothing more.
    #[serde(skip)]
    package: &'static str,
    /// The sites whose addresses this app opens.
    #[serde(skip)]
    sites: &'static [&'static str],
}

pub const SERVICES: &[Service] = &[
    Service { id: "netflix", name: "Netflix", package: "com.netflix.ninja", sites: &["netflix.com"] },
    Service { id: "prime", name: "Prime Video", package: "com.amazon.amazonvideo.livingroom", sites: &["primevideo.com", "amazon.com", "amazon.com.br"] },
    Service { id: "disney", name: "Disney+", package: "com.disney.disneyplus", sites: &["disneyplus.com"] },
    Service { id: "hbomax", name: "HBO Max", package: "com.wbd.stream", sites: &["hbomax.com", "max.com"] },
    Service { id: "globoplay", name: "Globoplay", package: "com.globo.globotv", sites: &["globoplay.globo.com"] },
    Service { id: "youtube", name: "YouTube", package: "com.google.android.youtube.tv", sites: &["youtube.com"] },
    Service { id: "twitch", name: "Twitch", package: "tv.twitch.android.app", sites: &["twitch.tv"] },
    Service { id: "spotify", name: "Spotify", package: "com.spotify.tv.android", sites: &[] },
];

fn on_site(host: &str, site: &str) -> bool {
    host == site || host.strip_suffix(site).is_some_and(|rest| rest.ends_with('.'))
}

fn service_of(url: &str) -> Option<&'static Service> {
    let url = reqwest::Url::parse(url).ok().filter(|u| u.scheme() == "https")?;
    let host = url.host_str()?.to_ascii_lowercase();
    SERVICES.iter().find(|s| s.sites.iter().any(|site| on_site(&host, site)))
}

/// What to tell the TV for `service`: its app, or `url` in it. Only that service's own addresses
/// go through: the screens can't make the TV open anything else.
pub fn link(service: &str, url: Option<&str>) -> Result<String> {
    let known = SERVICES.iter().find(|s| s.id == service).ok_or_else(|| Error::msg("That service isn't on the TV"))?;
    match url {
        None => Ok(format!("market://launch?id={}", known.package)),
        Some(url) if service_of(url).is_some_and(|s| s.id == service) => Ok(url.to_string()),
        Some(_) => Err(Error::msg("That address isn't that service's")),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Offer {
    pub service: &'static str,
    pub name: &'static str,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Found {
    pub title: String,
    pub year: Option<u64>,
    pub series: bool,
    pub poster: Option<String>,
    /// One per service.
    pub offers: Vec<Offer>,
}

fn poster(path: Option<&str>) -> Option<String> {
    Some(format!("{IMAGES}{}", path?.replace("{profile}", "s166").replace("{format}", "jpg")))
}

/// The titles of a JustWatch answer with somewhere to watch them on the house's services.
fn parse(body: &Value) -> Vec<Found> {
    let edges = body["data"]["popularTitles"]["edges"].as_array().into_iter().flatten();
    edges
        .filter_map(|e| {
            let node = &e["node"];
            let content = &node["content"];
            let mut offers: Vec<Offer> = Vec::new();
            for offer in node["offers"].as_array().into_iter().flatten() {
                if !WATCHABLE.contains(&offer["monetizationType"].as_str().unwrap_or_default()) {
                    continue;
                }
                let Some(url) = offer["standardWebURL"].as_str() else { continue };
                let Some(s) = service_of(url) else { continue };
                if !offers.iter().any(|o| o.service == s.id) {
                    offers.push(Offer { service: s.id, name: s.name, url: url.to_string() });
                }
            }
            (!offers.is_empty()).then(|| Found {
                title: content["title"].as_str().unwrap_or_default().trim().to_string(),
                year: content["originalReleaseYear"].as_u64(),
                series: node["objectType"] == "SHOW",
                poster: poster(content["posterUrl"].as_str()),
                offers,
            })
        })
        .collect()
}

pub async fn search(ctx: &Ctx, query: &str) -> Result<Vec<Found>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let house = ctx.settings.get();
    let language = house.language.split(['-', '_']).next().unwrap_or("en").to_lowercase();
    let body = json!({
        "query": QUERY,
        "variables": { "country": house.region.to_uppercase(), "language": language, "first": RESULTS, "filter": { "searchQuery": query } },
    });
    let answer: Value = tvl_kit::http::api().post(API).header("User-Agent", USER_AGENT).json(&body).send().await?.error_for_status()?.json().await?;
    if answer.get("errors").is_some() {
        return Err(Error::msg("The title search isn't answering as it used to"));
    }
    Ok(parse(&answer))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_lists_each_service_once_and_only_what_is_included() {
        let body = json!({ "data": { "popularTitles": { "edges": [
            { "node": {
                "objectType": "SHOW",
                "content": { "title": "Stranger Things", "originalReleaseYear": 2016, "posterUrl": "/poster/1/{profile}/s.{format}" },
                "offers": [
                    { "standardWebURL": "https://www.netflix.com/title/80057281", "monetizationType": "FLATRATE" },
                    { "standardWebURL": "https://www.netflix.com/title/80057281", "monetizationType": "ADS" },
                    { "standardWebURL": "https://www.primevideo.com/detail/x", "monetizationType": "RENT" },
                    { "standardWebURL": "https://tv.apple.com/x", "monetizationType": "FLATRATE" },
                ] } },
            { "node": { "objectType": "MOVIE", "content": { "title": "Only for rent" },
                "offers": [{ "standardWebURL": "https://www.primevideo.com/detail/y", "monetizationType": "BUY" }] } },
        ] } } });
        let got = parse(&body);
        assert_eq!(got.len(), 1, "nothing to watch it on: left out");
        assert!(got[0].series);
        assert_eq!(got[0].poster.as_deref(), Some("https://images.justwatch.com/poster/1/s166/s.jpg"));
        assert_eq!(got[0].offers, [Offer { service: "netflix", name: "Netflix", url: "https://www.netflix.com/title/80057281".into() }]);
    }

    #[test]
    fn an_app_opens_by_its_package() {
        assert_eq!(link("netflix", None).unwrap(), "market://launch?id=com.netflix.ninja");
        assert!(link("nonsense", None).is_err());
    }

    #[test]
    fn only_a_services_own_addresses_reach_the_tv() {
        let netflix = "https://www.netflix.com/title/1";
        assert_eq!(link("netflix", Some(netflix)).unwrap(), netflix);
        assert!(link("disney", Some(netflix)).is_err(), "another service's");
        assert!(link("netflix", Some("https://www.netflix.com.evil.example/title/1")).is_err());
        assert!(link("netflix", Some("https://notnetflix.com/title/1")).is_err());
        assert!(link("netflix", Some("http://www.netflix.com/title/1")).is_err());
        assert!(link("netflix", Some("intent://x")).is_err());
        assert!(link("spotify", Some("https://open.spotify.com/x")).is_err(), "no site: only the app");
    }
}
