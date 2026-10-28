use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::config::{AppEntry, Settings};
use crate::error::{Error, Result};

const API: &str = "https://api.themoviedb.org/3";
const POSTER: &str = "https://image.tmdb.org/t/p/w342";
const BACKDROP: &str = "https://image.tmdb.org/t/p/w1280";

#[derive(Debug, Clone, Serialize)]
pub struct Title {
    pub id: u64,
    /// "movie" or "tv"
    pub media_type: String,
    pub title: String,
    pub year: Option<String>,
    pub overview: String,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
}

#[derive(Deserialize)]
struct Page {
    results: Vec<Item>,
    #[serde(default)]
    total_pages: u32,
}

#[derive(Deserialize, Clone)]
struct Item {
    id: u64,
    media_type: Option<String>,
    title: Option<String>,
    name: Option<String>,
    original_title: Option<String>,
    original_name: Option<String>,
    release_date: Option<String>,
    first_air_date: Option<String>,
    overview: Option<String>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    #[serde(default)]
    genre_ids: Vec<u32>,
    vote_average: Option<f32>,
    vote_count: Option<u32>,
    popularity: Option<f32>,
}

/// A title in a list, with what the recommendations rank it by.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub title: Title,
    pub genres: Vec<u32>,
    /// TMDB's 0..10 average.
    pub rating: f32,
    pub votes: u32,
    pub popularity: f32,
}

#[derive(Deserialize)]
struct ProvidersResponse {
    #[serde(default)]
    results: HashMap<String, RegionProviders>,
}

#[derive(Deserialize, Default)]
struct RegionProviders {
    #[serde(default)]
    flatrate: Vec<Provider>,
    #[serde(default)]
    free: Vec<Provider>,
    #[serde(default)]
    ads: Vec<Provider>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Provider {
    pub provider_id: u32,
    pub provider_name: String,
}

/// The key from Settings, else `TMDB_API_KEY` from the environment or a `.env` file.
fn api_key(s: &Settings) -> Option<String> {
    s.tmdb_key()
}

fn request(client: &reqwest::Client, s: &Settings, path: &str) -> Result<reqwest::RequestBuilder> {
    let key = api_key(s)
        .ok_or_else(|| Error::msg("Set TMDB_API_KEY in .env, or add a TMDB key in Settings, to search"))?;
    let key = key.as_str();
    let req = client
        .get(format!("{API}{path}"))
        .query(&[("language", s.language.as_str())]);
    // v4 read tokens are long JWTs sent as a bearer; v3 keys go in the query.
    Ok(if key.len() > 40 {
        req.bearer_auth(key)
    } else {
        req.query(&[("api_key", key)])
    })
}

/// Movies and series matching `query`, best match first, or what's trending this week when it is
/// empty. Reads one TMDB page of each: enough for a whole title, as the assistant asks.
pub async fn search(client: &reqwest::Client, s: &Settings, query: &str) -> Result<Vec<Title>> {
    Ok(search_raw(client, s, query, 1, 1).await?.0)
}

/// TMDB serves at most this many pages of any list.
const LAST_PAGE: u32 = 500;
/// TMDB lists every title holding the query's last word whole before those where it only starts
/// a word, popular or not: "sta" gives pages of "Sta per piovere" before Star Wars. So typed
/// searches read this many pages of movies and of series and rank them here.
const SEARCH_POOL: u32 = 4;
/// Titles per page of a ranked search.
const PER_PAGE: usize = 20;

/// Page `page` of `search`, whose first pages rank `depth` TMDB pages of each kind; and the page
/// after it, if there is one.
async fn search_raw(
    client: &reqwest::Client,
    s: &Settings,
    query: &str,
    page: u32,
    depth: u32,
) -> Result<(Vec<Title>, Option<u32>)> {
    let q = query.trim();
    if q.is_empty() {
        let found: Page = get_cached(client, s, "/trending/all/week", &[("page", page.to_string())]).await?;
        let next = (page < found.total_pages.min(LAST_PAGE)).then_some(page + 1);
        return Ok((found.results.into_iter().filter_map(to_title).collect(), next));
    }
    // Every page re-reads the pool; the responses are cached.
    let (pool, pages) = search_ranked(client, s, q, 1..=depth).await?;
    let pool_pages = pool.len().div_ceil(PER_PAGE) as u32;
    if page <= pool_pages {
        let more = page < pool_pages || depth < pages;
        let titles = pool.into_iter().skip((page as usize - 1) * PER_PAGE).take(PER_PAGE).collect();
        return Ok((titles, more.then_some(page + 1)));
    }
    // Past the pool: TMDB's next pages, one of each kind at a time.
    let at = depth + page - pool_pages;
    let (titles, _) = search_ranked(client, s, q, at..=at).await?;
    Ok((titles, (at < pages).then_some(page + 1)))
}

/// Movies and series from TMDB search `pages`, best match first; and how many pages the longer
/// of the two lists has.
async fn search_ranked(
    client: &reqwest::Client,
    s: &Settings,
    q: &str,
    pages: std::ops::RangeInclusive<u32>,
) -> Result<(Vec<Title>, u32)> {
    let reads: Vec<(&str, u32)> = ["movie", "tv"].into_iter().flat_map(|m| pages.clone().map(move |n| (m, n))).collect();
    let lists = futures_util::future::join_all(reads.iter().map(|&(m, n)| async move {
        let query = [("query", q.to_string()), ("include_adult", "false".into()), ("page", n.to_string())];
        get_cached::<Page>(client, s, &format!("/search/{m}"), &query).await
    }))
    .await;
    let mut scored = Vec::new();
    let mut seen = HashSet::new();
    let mut total = 0;
    for ((m, n), list) in reads.into_iter().zip(lists) {
        let list = match list {
            Ok(l) => l,
            // A later page can be past the end; the first failing is the search failing.
            Err(_) if n > *pages.start() => continue,
            Err(e) => return Err(e),
        };
        total = total.max(list.total_pages);
        for mut i in list.results {
            if seen.insert((m, i.id)) {
                i.media_type = Some(m.into());
                let score = search_score(q, &i);
                scored.extend(to_title(i).map(|t| (score, t)));
            }
        }
    }
    // Stable, so equal scores keep TMDB's order.
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    Ok((scored.into_iter().map(|(_, t)| t).collect(), total.min(LAST_PAGE)))
}

/// How a result ranks for `query`: by popularity, well ahead when its name (or its original
/// name, "Star Trek" for "Jornada nas Estrelas") starts with the query, a little more when it is
/// the query. Tuned on TMDB's answers for "sta", "bre", "dune", "the off".
fn search_score(query: &str, i: &Item) -> f32 {
    let q = fold(query);
    let names = [&i.title, &i.name, &i.original_title, &i.original_name];
    let bonus = names
        .into_iter()
        .filter_map(|n| n.as_deref().map(fold))
        .map(|n| if n == q { 2.5 } else if n.starts_with(&q) { 2.0 } else { 0.0 })
        .fold(0.0, f32::max);
    i.popularity.unwrap_or(0.0).max(0.0).ln_1p() + bonus
}

/// Lowercase words, punctuation left out: "Star Wars: Rebels" -> "star wars rebels".
fn fold(s: &str) -> String {
    let words: Vec<String> = s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect();
    words.join(" ")
}
/// Filtering by service reads up to this many TMDB pages per call, until `FILTER_WANT` are kept,
/// so a rare match doesn't come back as an empty page.
const FILTER_PAGES: u32 = 3;
const FILTER_WANT: usize = 12;

/// A search result and the user's apps that stream it.
#[derive(Debug, Clone, Serialize)]
pub struct Found {
    #[serde(flatten)]
    pub title: Title,
    pub services: Vec<String>,
}

/// Part of a paged search, and the TMDB page it continues from (None: that was all).
#[derive(Debug, Clone, Serialize)]
pub struct SearchPage {
    pub results: Vec<Found>,
    pub next: Option<u32>,
}

/// Search from TMDB page `page`, with which of `apps` stream each title in the settings' region.
/// `only`: just the titles `apps` stream; an empty query then lists what's popular on them
/// instead of what's trending everywhere.
pub async fn search_page(
    client: &reqwest::Client,
    s: &Settings,
    apps: &[AppEntry],
    query: &str,
    page: u32,
    only: bool,
) -> Result<SearchPage> {
    let page = page.clamp(1, LAST_PAGE);
    if only && query.trim().is_empty() {
        return popular_on(client, s, apps, page).await;
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut at = page;
    let mut reads = 0;
    loop {
        let (titles, next) = search_raw(client, s, query, at, SEARCH_POOL).await?;
        reads += 1;
        let checks = titles.iter().map(|t| offers(client, s, &t.media_type, t.id));
        let found = futures_util::future::join_all(checks).await;
        for (title, o) in titles.into_iter().zip(found) {
            let services = o.map(|o| match_apps(&o, apps)).unwrap_or_default();
            if (only && services.is_empty()) || !seen.insert((title.media_type.clone(), title.id)) {
                continue;
            }
            out.push(Found { title, services });
        }
        let Some(n) = next else {
            return Ok(SearchPage { results: out, next: None });
        };
        at = n;
        if !only || out.len() >= FILTER_WANT || reads >= FILTER_PAGES {
            return Ok(SearchPage { results: out, next: Some(at) });
        }
    }
}

/// What `apps` stream, most popular first, movies and series side by side.
async fn popular_on(client: &reqwest::Client, s: &Settings, apps: &[AppEntry], page: u32) -> Result<SearchPage> {
    let mut ids: Vec<u32> = apps.iter().flat_map(|a| a.tmdb_providers.iter().copied()).collect();
    ids.sort_unstable();
    ids.dedup();
    let queries = ["movie", "tv"].map(|m| Discover {
        media_type: m.into(),
        genre: None,
        sort: None,
        page: Some(page),
        since_years: None,
        min_votes: None,
        max_votes: None,
        any_genres: None,
    });
    let lists = futures_util::future::join_all(queries.iter().map(|q| discover_page(client, s, &ids, q))).await;
    let mut pages = 0;
    let lists: Vec<Vec<Candidate>> = lists
        .into_iter()
        .map(|l| {
            let (list, n) = l.unwrap_or_default();
            pages = pages.max(n);
            list
        })
        .collect();
    let mut titles = Vec::new();
    let mut seen = HashSet::new();
    for i in 0..lists.iter().map(Vec::len).max().unwrap_or(0) {
        for c in lists.iter().filter_map(|l| l.get(i)) {
            if seen.insert((c.title.media_type.clone(), c.title.id)) {
                titles.push(c.title.clone());
            }
        }
    }
    // One service: that's where they all stream. Several: ask where each one does.
    let results = if let [app] = apps {
        titles.into_iter().map(|title| Found { title, services: vec![app.id.clone()] }).collect()
    } else {
        let checks = titles.iter().map(|t| offers(client, s, &t.media_type, t.id));
        let found = futures_util::future::join_all(checks).await;
        titles
            .into_iter()
            .zip(found)
            .map(|(title, o)| Found { title, services: o.map(|o| match_apps(&o, apps)).unwrap_or_default() })
            .collect()
    };
    Ok(SearchPage { results, next: (page < pages.min(LAST_PAGE)).then_some(page + 1) })
}

/// Ids of the configured apps that stream this title in the settings' region.
pub async fn providers(
    client: &reqwest::Client,
    s: &Settings,
    apps: &[AppEntry],
    media_type: &str,
    id: u64,
) -> Result<Vec<String>> {
    Ok(match_apps(&offers(client, s, media_type, id).await?, apps))
}

/// Answers of `offers` kept: a search checks every result, more than the response cache holds.
const OFFERS_MAX: usize = 5000;

/// Who streams this title in the settings' region, by subscription, free or with ads.
async fn offers(client: &reqwest::Client, s: &Settings, media_type: &str, id: u64) -> Result<Vec<Provider>> {
    type OffersCache = Mutex<HashMap<String, (Instant, Vec<Provider>)>>;
    static OFFERS: OnceLock<OffersCache> = OnceLock::new();
    let cache = OFFERS.get_or_init(Default::default);
    let key = format!("{}:{media_type}:{id}", s.region.to_uppercase());
    if let Some((at, list)) = cache.lock().unwrap().get(&key) {
        if at.elapsed() < CACHE_TTL {
            return Ok(list.clone());
        }
    }
    let list = offers_fresh(client, s, media_type, id).await?;
    let mut c = cache.lock().unwrap();
    if c.len() >= OFFERS_MAX {
        c.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
        if c.len() >= OFFERS_MAX {
            c.clear();
        }
    }
    c.insert(key, (Instant::now(), list.clone()));
    Ok(list)
}

/// Like `offers`, but always asked again and kept out of the response cache: the recommendations
/// check hundreds of titles and keep only the answer (recs/mod.rs).
pub async fn offers_fresh(client: &reqwest::Client, s: &Settings, media_type: &str, id: u64) -> Result<Vec<Provider>> {
    check_media(media_type)?;
    let _permit = permit().await;
    let resp: ProvidersResponse = request(client, s, &format!("/{media_type}/{id}/watch/providers"))?
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let region = resp.results.into_iter().find(|(k, _)| k.eq_ignore_ascii_case(&s.region));
    let offers = region.map(|(_, r)| r).unwrap_or_default();
    Ok(offers.flatrate.into_iter().chain(offers.free).chain(offers.ads).collect())
}

fn check_media(media_type: &str) -> Result<()> {
    if media_type != "movie" && media_type != "tv" {
        return Err(Error::msg(format!("unknown media type '{media_type}'")));
    }
    Ok(())
}

// ---------- Catalog for the streaming apps' own screens ----------

const STILL: &str = "https://image.tmdb.org/t/p/w400";
const PROFILE: &str = "https://image.tmdb.org/t/p/w185";
const LOGO: &str = "https://image.tmdb.org/t/p/w500";

/// TMDB answers are kept this long, so moving between screens doesn't ask again.
const CACHE_TTL: Duration = Duration::from_secs(30 * 60);
const CACHE_MAX: usize = 400;

/// TMDB answers about 40 requests a second; the recommendations alone can ask for hundreds.
const PARALLEL: usize = 8;

type Cache = Mutex<HashMap<String, (Instant, String)>>;

async fn permit() -> tokio::sync::SemaphorePermit<'static> {
    static GATE: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    GATE.get_or_init(|| tokio::sync::Semaphore::new(PARALLEL))
        .acquire()
        .await
        .expect("the semaphore is never closed")
}

fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// GET `path` with `query`, through a short-lived cache of the response text.
async fn get_cached<T: DeserializeOwned>(
    client: &reqwest::Client,
    s: &Settings,
    path: &str,
    query: &[(&str, String)],
) -> Result<T> {
    let mut key = format!("{path}?lang={}&region={}", s.language, s.region);
    for (k, v) in query {
        key.push_str(&format!("&{k}={v}"));
    }
    let hit = cache()
        .lock()
        .unwrap()
        .get(&key)
        .filter(|(at, _)| at.elapsed() < CACHE_TTL)
        .map(|(_, text)| text.clone());
    let text = match hit {
        Some(t) => t,
        None => {
            let _permit = permit().await;
            let text = request(client, s, path)?
                .query(query)
                .send()
                .await?
                .error_for_status()?
                .text()
                .await?;
            let mut c = cache().lock().unwrap();
            if c.len() >= CACHE_MAX {
                c.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
                if c.len() >= CACHE_MAX {
                    c.clear();
                }
            }
            c.insert(key, (Instant::now(), text.clone()));
            text
        }
    };
    Ok(serde_json::from_str(&text)?)
}

/// What a row of a streaming app's home asks for.
#[derive(Debug, Clone, Deserialize)]
pub struct Discover {
    /// "movie" or "tv"
    pub media_type: String,
    /// TMDB genre id.
    pub genre: Option<u32>,
    /// TMDB sort, e.g. "popularity.desc". Default: popularity.
    pub sort: Option<String>,
    pub page: Option<u32>,
    /// Only titles first released in the last this many years.
    pub since_years: Option<u32>,
    /// Only titles with at least this many votes: keeps obscure ones out of popular rows.
    pub min_votes: Option<u32>,
    /// Only titles with at most this many votes (the recommendations' hidden gems).
    #[serde(default)]
    pub max_votes: Option<u32>,
    /// Titles in any of these genres; `genre` wins when both are set.
    #[serde(default)]
    pub any_genres: Option<Vec<u32>>,
}

/// January 1st, `years` before this year.
fn years_ago(years: u32) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let year = 1970 + (secs as f64 / (365.2425 * 86400.0)) as u32;
    format!("{}-01-01", year.saturating_sub(years))
}

/// Movies or series that `provider_ids` stream in the settings' region.
pub async fn discover(
    client: &reqwest::Client,
    s: &Settings,
    provider_ids: &[u32],
    d: &Discover,
) -> Result<Vec<Title>> {
    Ok(discover_candidates(client, s, provider_ids, d)
        .await?
        .into_iter()
        .map(|c| c.title)
        .collect())
}

/// `discover`, keeping what the recommendations rank by.
pub async fn discover_candidates(
    client: &reqwest::Client,
    s: &Settings,
    provider_ids: &[u32],
    d: &Discover,
) -> Result<Vec<Candidate>> {
    Ok(discover_page(client, s, provider_ids, d).await?.0)
}

/// `discover_candidates`, and how many pages there are.
async fn discover_page(
    client: &reqwest::Client,
    s: &Settings,
    provider_ids: &[u32],
    d: &Discover,
) -> Result<(Vec<Candidate>, u32)> {
    check_media(&d.media_type)?;
    if provider_ids.is_empty() {
        return Ok((Vec::new(), 0));
    }
    let ids: Vec<String> = provider_ids.iter().map(u32::to_string).collect();
    let mut query = vec![
        ("with_watch_providers", ids.join("|")),
        ("watch_region", s.region.clone()),
        ("with_watch_monetization_types", "flatrate|free|ads".to_string()),
        ("sort_by", d.sort.clone().unwrap_or_else(|| "popularity.desc".into())),
        ("include_adult", "false".into()),
        ("page", d.page.unwrap_or(1).to_string()),
    ];
    if let Some(g) = d.genre {
        query.push(("with_genres", g.to_string()));
    } else if let Some(any) = d.any_genres.as_ref().filter(|g| !g.is_empty()) {
        let any: Vec<String> = any.iter().map(u32::to_string).collect();
        query.push(("with_genres", any.join("|")));
    }
    // Sorting by rating alone surfaces titles with three votes.
    let rated = d.sort.as_deref().is_some_and(|x| x.starts_with("vote_average"));
    if let Some(n) = d.min_votes.or(rated.then_some(300)) {
        query.push(("vote_count.gte", n.to_string()));
    }
    if let Some(n) = d.max_votes {
        query.push(("vote_count.lte", n.to_string()));
    }
    if let Some(y) = d.since_years {
        let field = if d.media_type == "tv" { "first_air_date.gte" } else { "primary_release_date.gte" };
        query.push((field, years_ago(y)));
    }
    let page: Page = get_cached(client, s, &format!("/discover/{}", d.media_type), &query).await?;
    Ok((candidates(page.results, Some(&d.media_type)), page.total_pages))
}

/// List items as candidates; `media_type` for lists whose items don't say (similar, discover).
fn candidates(items: Vec<Item>, media_type: Option<&str>) -> Vec<Candidate> {
    items
        .into_iter()
        .filter_map(|mut i| {
            if let Some(m) = media_type {
                i.media_type = Some(m.to_string());
            }
            to_candidate(i)
        })
        .collect()
}

/// TMDB's "recommendations" (liked by the same people) or "similar" (same genres and
/// keywords) for a title.
pub async fn related(
    client: &reqwest::Client,
    s: &Settings,
    media_type: &str,
    id: u64,
    kind: &str,
) -> Result<Vec<Candidate>> {
    check_media(media_type)?;
    if kind != "recommendations" && kind != "similar" {
        return Err(Error::msg(format!("unknown list '{kind}'")));
    }
    let page: Page = get_cached(client, s, &format!("/{media_type}/{id}/{kind}"), &[]).await?;
    Ok(candidates(page.results, Some(media_type)))
}

/// What is trending everywhere, `window` "day" or "week".
pub async fn trending(client: &reqwest::Client, s: &Settings, window: &str, page: u32) -> Result<Vec<Candidate>> {
    let page: Page = get_cached(client, s, &format!("/trending/all/{window}"), &[("page", page.to_string())]).await?;
    Ok(candidates(page.results, None))
}

#[derive(Deserialize)]
struct PersonCredits {
    #[serde(default)]
    cast: Vec<Item>,
}

/// Movies and series a person acted in.
pub async fn person_credits(client: &reqwest::Client, s: &Settings, person: u64) -> Result<Vec<Candidate>> {
    let credits: PersonCredits = get_cached(client, s, &format!("/person/{person}/combined_credits"), &[]).await?;
    Ok(candidates(credits.cast, None))
}

#[derive(Deserialize)]
struct NamedId {
    id: u64,
    name: String,
}

/// What the recommendations need to know about a title the user watched or rated.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Meta {
    pub genres: Vec<u32>,
    /// Top-billed cast: person id and name.
    pub cast: Vec<(u64, String)>,
}

#[derive(Deserialize)]
struct RawMeta {
    #[serde(default)]
    genres: Vec<NamedId>,
    credits: Option<RawMetaCredits>,
}

#[derive(Deserialize)]
struct RawMetaCredits {
    #[serde(default)]
    cast: Vec<NamedId>,
}

/// Genres and lead actors, read from the same answer as `details` (so it is usually cached).
pub async fn meta(client: &reqwest::Client, s: &Settings, media_type: &str, id: u64) -> Result<Meta> {
    check_media(media_type)?;
    let raw: RawMeta = get_cached(client, s, &format!("/{media_type}/{id}"), &details_query(s, media_type)).await?;
    Ok(Meta {
        genres: raw.genres.into_iter().map(|g| g.id as u32).collect(),
        cast: raw
            .credits
            .map(|c| c.cast)
            .unwrap_or_default()
            .into_iter()
            .take(4)
            .map(|c| (c.id, c.name))
            .collect(),
    })
}

/// Search, keeping only what `provider_ids` stream in the settings' region.
pub async fn search_on(
    client: &reqwest::Client,
    s: &Settings,
    provider_ids: &[u32],
    query: &str,
) -> Result<Vec<Title>> {
    let found = search(client, s, query).await?;
    let checks = found.iter().take(20).map(|t| offers(client, s, &t.media_type, t.id));
    let offers = futures_util::future::join_all(checks).await;
    Ok(found
        .into_iter()
        .zip(offers)
        .filter(|(_, o)| {
            o.as_ref()
                .is_ok_and(|list| list.iter().any(|p| provider_ids.contains(&p.provider_id)))
        })
        .map(|(t, _)| t)
        .collect())
}

#[derive(Debug, Clone, Serialize)]
pub struct Season {
    pub number: u32,
    pub name: String,
    pub episode_count: u32,
    pub year: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CastMember {
    /// TMDB person id.
    pub id: u64,
    pub name: String,
    pub character: String,
    pub photo: Option<String>,
    /// "director" (a movie's) or "creator" (a series'); None for the cast.
    pub job: Option<&'static str>,
}

/// Everything the details screen shows about a movie or series.
#[derive(Debug, Clone, Serialize)]
pub struct Details {
    #[serde(flatten)]
    pub title: Title,
    pub tagline: Option<String>,
    pub genres: Vec<String>,
    /// Minutes: the movie's length, or a typical episode's.
    pub runtime: Option<u32>,
    /// Age rating in the settings' region ("16", "L", ...).
    pub certification: Option<String>,
    /// The title's logo artwork, when TMDB has one.
    pub logo: Option<String>,
    /// Regular seasons, specials (season 0) left out.
    pub seasons: Vec<Season>,
    pub cast: Vec<CastMember>,
    /// YouTube video id of a trailer.
    pub trailer: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Episode {
    pub season: u32,
    pub number: u32,
    pub name: String,
    pub overview: String,
    pub still: Option<String>,
    /// Minutes.
    pub runtime: Option<u32>,
    pub air_date: Option<String>,
}

#[derive(Deserialize)]
struct RawDetails {
    id: u64,
    title: Option<String>,
    name: Option<String>,
    release_date: Option<String>,
    first_air_date: Option<String>,
    #[serde(default)]
    overview: Option<String>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    tagline: Option<String>,
    #[serde(default)]
    genres: Vec<Named>,
    runtime: Option<u32>,
    #[serde(default)]
    episode_run_time: Vec<u32>,
    last_episode_to_air: Option<RawRuntime>,
    #[serde(default)]
    seasons: Vec<RawSeason>,
    credits: Option<RawCredits>,
    videos: Option<Results<RawVideo>>,
    images: Option<RawImages>,
    release_dates: Option<Results<RawReleases>>,
    content_ratings: Option<Results<RawRating>>,
    #[serde(default)]
    created_by: Vec<RawCast>,
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

/// `details`' query; `meta` asks the same so the two share one cached answer.
fn details_query(s: &Settings, media_type: &str) -> [(&'static str, String); 3] {
    let lang = s.language.split('-').next().unwrap_or("en");
    let extra = if media_type == "movie" { "release_dates" } else { "content_ratings" };
    [
        ("append_to_response", format!("credits,videos,images,{extra}")),
        // Trailers and logos are often only in English; TMDB filters both by language otherwise.
        ("include_video_language", format!("{lang},en,null")),
        ("include_image_language", format!("{lang},en,null")),
    ]
}

#[derive(Deserialize)]
struct RawRuntime {
    runtime: Option<u32>,
}

#[derive(Deserialize)]
struct RawSeason {
    season_number: u32,
    name: Option<String>,
    #[serde(default)]
    episode_count: u32,
    air_date: Option<String>,
}

#[derive(Deserialize)]
struct RawCredits {
    #[serde(default)]
    cast: Vec<RawCast>,
    #[serde(default)]
    crew: Vec<RawCast>,
}

#[derive(Deserialize)]
struct RawCast {
    id: u64,
    name: String,
    job: Option<String>,
    character: Option<String>,
    profile_path: Option<String>,
}

#[derive(Deserialize)]
struct Results<T> {
    #[serde(default = "Vec::new")]
    results: Vec<T>,
}

#[derive(Deserialize)]
struct RawVideo {
    key: String,
    site: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    official: bool,
}

#[derive(Deserialize)]
struct RawImages {
    #[serde(default)]
    logos: Vec<RawImage>,
}

#[derive(Deserialize)]
struct RawImage {
    file_path: String,
    iso_639_1: Option<String>,
}

#[derive(Deserialize)]
struct RawReleases {
    iso_3166_1: String,
    #[serde(default)]
    release_dates: Vec<RawCert>,
}

#[derive(Deserialize)]
struct RawCert {
    #[serde(default)]
    certification: String,
}

#[derive(Deserialize)]
struct RawRating {
    iso_3166_1: String,
    #[serde(default)]
    rating: String,
}

pub async fn details(client: &reqwest::Client, s: &Settings, media_type: &str, id: u64) -> Result<Details> {
    check_media(media_type)?;
    let lang = s.language.split('-').next().unwrap_or("en").to_string();
    let raw: RawDetails = get_cached(client, s, &format!("/{media_type}/{id}"), &details_query(s, media_type)).await?;
    Ok(to_details(raw, media_type, &s.region, &lang))
}

fn to_details(raw: RawDetails, media_type: &str, region: &str, lang: &str) -> Details {
    let certification = raw
        .release_dates
        .and_then(|r| {
            r.results
                .into_iter()
                .find(|x| x.iso_3166_1.eq_ignore_ascii_case(region))
                .and_then(|x| x.release_dates.into_iter().map(|c| c.certification).find(|c| !c.is_empty()))
        })
        .or_else(|| {
            raw.content_ratings.and_then(|r| {
                r.results
                    .into_iter()
                    .find(|x| x.iso_3166_1.eq_ignore_ascii_case(region))
                    .map(|x| x.rating)
                    .filter(|c| !c.is_empty())
            })
        });
    let videos = raw.videos.map(|v| v.results).unwrap_or_default();
    let trailer = videos
        .iter()
        .filter(|v| v.site == "YouTube")
        .min_by_key(|v| (v.kind != "Trailer", !v.official))
        .map(|v| v.key.clone());
    let logos = raw.images.map(|i| i.logos).unwrap_or_default();
    let logo = logos
        .iter()
        .find(|l| l.iso_639_1.as_deref() == Some(lang))
        .or_else(|| logos.iter().find(|l| l.iso_639_1.as_deref() == Some("en")))
        .or(logos.first())
        .map(|l| format!("{LOGO}{}", l.file_path));
    let runtime = raw
        .runtime
        .filter(|r| *r > 0)
        .or_else(|| raw.episode_run_time.first().copied())
        .or_else(|| raw.last_episode_to_air.and_then(|e| e.runtime));
    let seasons = raw
        .seasons
        .into_iter()
        .filter(|x| x.season_number > 0 && x.episode_count > 0)
        .map(|x| Season {
            number: x.season_number,
            name: x.name.unwrap_or_default(),
            episode_count: x.episode_count,
            year: x.air_date.filter(|d| d.len() >= 4).map(|d| d[..4].to_string()),
        })
        .collect();
    let (cast, crew) = raw.credits.map(|c| (c.cast, c.crew)).unwrap_or_default();
    // The movie's directors, or the series' creators, ahead of the cast.
    let (heads, job) = if media_type == "tv" {
        (raw.created_by, "creator")
    } else {
        (crew.into_iter().filter(|c| c.job.as_deref() == Some("Director")).collect(), "director")
    };
    let mut seen = HashSet::new();
    let heads = heads.into_iter().filter(|c| seen.insert(c.id)).take(2).map(|c| (c, Some(job)));
    let cast = heads
        .chain(cast.into_iter().take(12).map(|c| (c, None)))
        .map(|(c, job)| CastMember {
            id: c.id,
            name: c.name,
            character: c.character.unwrap_or_default(),
            photo: c.profile_path.map(|p| format!("{PROFILE}{p}")),
            job,
        })
        .collect();
    let item = Item {
        id: raw.id,
        media_type: Some(media_type.to_string()),
        title: raw.title,
        name: raw.name,
        original_title: None,
        original_name: None,
        release_date: raw.release_date,
        first_air_date: raw.first_air_date,
        overview: raw.overview,
        poster_path: raw.poster_path,
        backdrop_path: raw.backdrop_path,
        genre_ids: Vec::new(),
        vote_average: None,
        vote_count: None,
        popularity: None,
    };
    let title = to_title(item).unwrap_or_else(|| Title {
        id: raw.id,
        media_type: media_type.to_string(),
        title: String::new(),
        year: None,
        overview: String::new(),
        poster: None,
        backdrop: None,
    });
    Details {
        title,
        tagline: raw.tagline.filter(|t| !t.is_empty()),
        genres: raw.genres.into_iter().map(|g| g.name).collect(),
        runtime,
        certification,
        logo,
        seasons,
        cast,
        trailer,
    }
}

#[derive(Deserialize)]
struct RawSeasonPage {
    #[serde(default)]
    episodes: Vec<RawEpisode>,
}

#[derive(Deserialize)]
struct RawEpisode {
    season_number: u32,
    episode_number: u32,
    name: Option<String>,
    overview: Option<String>,
    still_path: Option<String>,
    runtime: Option<u32>,
    air_date: Option<String>,
}

/// One season's episodes, in order.
pub async fn season(client: &reqwest::Client, s: &Settings, tv_id: u64, number: u32) -> Result<Vec<Episode>> {
    let page: RawSeasonPage = get_cached(client, s, &format!("/tv/{tv_id}/season/{number}"), &[]).await?;
    Ok(to_episodes(page))
}

fn to_episodes(page: RawSeasonPage) -> Vec<Episode> {
    page.episodes
        .into_iter()
        .map(|e| Episode {
            season: e.season_number,
            number: e.episode_number,
            name: e.name.unwrap_or_default(),
            overview: e.overview.unwrap_or_default(),
            still: e.still_path.map(|p| format!("{STILL}{p}")),
            runtime: e.runtime,
            air_date: e.air_date.filter(|d| !d.is_empty()),
        })
        .collect()
}

// ---------- People: an actor's or director's page ----------

const PHOTO: &str = "https://image.tmdb.org/t/p/w500";

/// TMDB's talk-show and news genres: people there appear as themselves, not in a role.
const TALK_GENRES: [u32; 2] = [10767, 10763];

/// How many of a person's best-known titles their page shows.
const KNOWN_MAX: usize = 10;

/// At most this many of a person's titles are checked for the user's apps, best known first.
const SERVICES_MAX: usize = 150;

#[derive(Deserialize)]
struct RawPerson {
    id: u64,
    name: String,
    known_for_department: Option<String>,
    birthday: Option<String>,
    deathday: Option<String>,
    place_of_birth: Option<String>,
    biography: Option<String>,
    profile_path: Option<String>,
    combined_credits: Option<RawPersonCredits>,
}

#[derive(Deserialize)]
struct RawBio {
    biography: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawPersonCredits {
    #[serde(default)]
    cast: Vec<RawPersonCredit>,
    #[serde(default)]
    crew: Vec<RawPersonCredit>,
}

#[derive(Deserialize)]
struct RawPersonCredit {
    #[serde(flatten)]
    item: Item,
    character: Option<String>,
    job: Option<String>,
    department: Option<String>,
    episode_count: Option<u32>,
    /// Billing in the cast, 0 first.
    order: Option<u32>,
}

/// A title on a person's page and what they did in it.
#[derive(Debug, Clone, Serialize)]
pub struct Credit {
    #[serde(flatten)]
    pub title: Title,
    /// The character, or the jobs ("Director, Writer").
    pub role: String,
    /// Behind the camera.
    pub crew: bool,
}

/// Everything an actor's or director's page shows.
#[derive(Debug, Clone, Serialize)]
pub struct Person {
    pub id: u64,
    pub name: String,
    /// TMDB's department, always in English: "Acting", "Directing", "Writing"...
    pub department: Option<String>,
    /// "1995-12-27"
    pub birthday: Option<String>,
    pub deathday: Option<String>,
    pub place: Option<String>,
    pub bio: String,
    pub photo: Option<String>,
    /// Their best-known titles, most known first.
    pub known_for: Vec<Credit>,
    /// Every role and job, newest first; titles without a date yet (announced) before those.
    pub credits: Vec<Credit>,
}

async fn raw_person(client: &reqwest::Client, s: &Settings, id: u64) -> Result<RawPerson> {
    let query = [("append_to_response", "combined_credits".to_string())];
    get_cached(client, s, &format!("/person/{id}"), &query).await
}

pub async fn person(client: &reqwest::Client, s: &Settings, id: u64) -> Result<Person> {
    let mut p = to_person(raw_person(client, s, id).await?);
    // Most biographies are only written in English.
    if p.bio.is_empty() && !s.language.starts_with("en") {
        let mut en = s.clone();
        en.language = "en-US".into();
        if let Ok(raw) = get_cached::<RawBio>(client, &en, &format!("/person/{id}"), &[]).await {
            p.bio = raw.biography.unwrap_or_default().trim().to_string();
        }
    }
    Ok(p)
}

/// Which of `apps` stream a person's titles, keyed "movie:603"; titles none of them stream are
/// left out. Asked apart from `person`: checking every title takes a few seconds.
pub async fn person_services(
    client: &reqwest::Client,
    s: &Settings,
    apps: &[AppEntry],
    id: u64,
) -> Result<HashMap<String, Vec<String>>> {
    let credits = raw_person(client, s, id).await?.combined_credits.unwrap_or_default();
    let mut all: Vec<RawPersonCredit> = credits.cast.into_iter().chain(credits.crew).filter(|c| !is_talk(c)).collect();
    all.sort_by(|a, b| known_weight(b).total_cmp(&known_weight(a)));
    let mut seen = HashSet::new();
    let titles: Vec<(String, u64)> = all
        .into_iter()
        .filter_map(|c| {
            let m = c.item.media_type.filter(|m| m == "movie" || m == "tv")?;
            seen.insert((m.clone(), c.item.id)).then_some((m, c.item.id))
        })
        .take(SERVICES_MAX)
        .collect();
    let found = futures_util::future::join_all(titles.iter().map(|(m, id)| offers(client, s, m, *id))).await;
    Ok(titles
        .into_iter()
        .zip(found)
        .filter_map(|((m, id), o)| {
            let on = match_apps(&o.ok()?, apps);
            (!on.is_empty()).then(|| (format!("{m}:{id}"), on))
        })
        .collect())
}

fn is_talk(c: &RawPersonCredit) -> bool {
    c.item.genre_ids.iter().any(|g| TALK_GENRES.contains(g))
}

/// Playing themselves: "Self", "Himself (archive footage)", "Self - Host".
fn is_self(c: &RawPersonCredit) -> bool {
    let ch = c.character.as_deref().unwrap_or("").trim().to_lowercase();
    ["self", "himself", "herself", "themselves"]
        .iter()
        .any(|w| ch == *w || ch.strip_prefix(w).is_some_and(|rest| rest.starts_with([' ', '-', '('])))
}

/// How well known a credit makes someone: how many people rated the title, less the further down
/// the cast they're billed (a teenager in Interstellar), little for a series they were in for an
/// episode or two.
fn known_weight(c: &RawPersonCredit) -> f32 {
    let votes = c.item.vote_count.unwrap_or(0) as f32;
    let billing = 1.0 / (1.0 + c.order.unwrap_or(0) as f32 * 0.25);
    let brief = c.item.media_type.as_deref() == Some("tv")
        && c.job.as_deref() != Some("Creator")
        && c.episode_count.is_some_and(|n| n < 3);
    votes * billing * if brief { 0.1 } else { 1.0 }
}

/// Credits for the same title merged into one, their roles joined, in the order first seen;
/// each with its date ("9999" when it has none yet) to sort by.
fn merge_credits<'a>(list: impl IntoIterator<Item = &'a RawPersonCredit>, crew: bool) -> Vec<(String, Credit)> {
    let mut out: Vec<(String, Credit)> = Vec::new();
    let mut at: HashMap<(String, u64), usize> = HashMap::new();
    for c in list {
        let role = if crew { &c.job } else { &c.character };
        let role = role.as_deref().unwrap_or("").trim().to_string();
        let date = c.item.release_date.clone().or(c.item.first_air_date.clone()).filter(|d| !d.is_empty());
        let Some(title) = to_title(c.item.clone()) else { continue };
        let key = (title.media_type.clone(), title.id);
        if let Some(&i) = at.get(&key) {
            let roles = &mut out[i].1.role;
            if !role.is_empty() && !roles.split(", ").any(|r| r == role) {
                if !roles.is_empty() {
                    roles.push_str(", ");
                }
                roles.push_str(&role);
            }
            continue;
        }
        at.insert(key, out.len());
        out.push((date.unwrap_or_else(|| "9999".into()), Credit { title, role, crew }));
    }
    out
}

fn to_person(raw: RawPerson) -> Person {
    let credits = raw.combined_credits.unwrap_or_default();
    let department = raw.known_for_department.filter(|d| !d.is_empty());
    let cast: Vec<RawPersonCredit> = credits.cast.into_iter().filter(|c| !is_talk(c)).collect();
    let crew: Vec<RawPersonCredit> = credits.crew.into_iter().filter(|c| !is_talk(c)).collect();

    // Known for: an actor's roles; anyone else's work in their own department (and the series
    // they created). Either one when the other has nothing.
    let acting = department.as_deref().is_none_or(|d| d == "Acting");
    let roles: Vec<&RawPersonCredit> = cast.iter().filter(|c| !is_self(c)).collect();
    let jobs: Vec<&RawPersonCredit> = crew
        .iter()
        .filter(|c| c.department == department || c.job.as_deref() == Some("Creator"))
        .collect();
    let (mut pool, from_crew) = if (acting && !roles.is_empty()) || jobs.is_empty() { (roles, false) } else { (jobs, true) };
    pool.retain(|c| c.item.poster_path.is_some());
    pool.sort_by(|a, b| known_weight(b).total_cmp(&known_weight(a)));
    let known_for = merge_credits(pool, from_crew).into_iter().take(KNOWN_MAX).map(|(_, c)| c).collect();

    // The filmography leaves out playing themselves (documentaries, award shows) and "Thanks"
    // credits, unless that's all there is.
    let thanks = |c: &&RawPersonCredit| c.job.as_deref() != Some("Thanks");
    let mut all = merge_credits(cast.iter().filter(|c| !is_self(c)), false);
    all.extend(merge_credits(crew.iter().filter(thanks), true));
    if all.is_empty() {
        all = merge_credits(&cast, false);
        all.extend(merge_credits(&crew, true));
    }
    // Newest first; stable, so on the same date roles stay ahead of jobs.
    all.sort_by(|a, b| b.0.cmp(&a.0));
    Person {
        id: raw.id,
        name: raw.name,
        department,
        birthday: raw.birthday.filter(|d| !d.is_empty()),
        deathday: raw.deathday.filter(|d| !d.is_empty()),
        place: raw.place_of_birth.filter(|p| !p.is_empty()),
        bio: raw.biography.unwrap_or_default().trim().to_string(),
        photo: raw.profile_path.map(|p| format!("{PHOTO}{p}")),
        known_for,
        credits: all.into_iter().map(|(_, c)| c).collect(),
    }
}

fn to_candidate(mut i: Item) -> Option<Candidate> {
    let genres = std::mem::take(&mut i.genre_ids);
    let rating = i.vote_average.unwrap_or(0.0);
    let votes = i.vote_count.unwrap_or(0);
    let popularity = i.popularity.unwrap_or(0.0);
    Some(Candidate {
        title: to_title(i)?,
        genres,
        rating,
        votes,
        popularity,
    })
}

fn to_title(i: Item) -> Option<Title> {
    let media_type = i.media_type.unwrap_or_default();
    if media_type != "movie" && media_type != "tv" {
        return None;
    }
    let title = i.title.or(i.name)?;
    let year = i
        .release_date
        .or(i.first_air_date)
        .filter(|d| d.len() >= 4)
        .map(|d| d[..4].to_string());
    Some(Title {
        id: i.id,
        media_type,
        title,
        year,
        overview: i.overview.unwrap_or_default(),
        poster: i.poster_path.map(|p| format!("{POSTER}{p}")),
        backdrop: i.backdrop_path.map(|p| format!("{BACKDROP}{p}")),
    })
}

/// Maps TMDB providers (in TMDB's priority order) to app ids, by provider id or by name. A name
/// also matches its plans and resellers: "Netflix Standard with Ads", "Amazon Prime Video with Ads".
pub fn match_apps(providers: &[Provider], apps: &[AppEntry]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in providers {
        let provider = p.provider_name.to_lowercase();
        let provider = provider.strip_prefix("amazon ").unwrap_or(&provider);
        for a in apps {
            let name = a.name.to_lowercase();
            let hit = a.tmdb_providers.contains(&p.provider_id)
                || provider == name
                || provider.strip_prefix(name.as_str()).is_some_and(|rest| rest.starts_with(' '));
            if hit && !out.contains(&a.id) {
                out.push(a.id.clone());
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
        parse_apps(include_str!("../defaults/apps.toml")).unwrap()
    }

    fn p(id: u32, name: &str) -> Provider {
        Provider {
            provider_id: id,
            provider_name: name.into(),
        }
    }

    #[test]
    fn settings_key_wins_over_env() {
        // The only test that touches TMDB_API_KEY, so parallel tests can't race on it.
        std::env::set_var("TMDB_API_KEY", " from-env ");
        let mut s = Settings::default();
        assert_eq!(api_key(&s).as_deref(), Some("from-env"));
        s.tmdb_api_key = "from-settings".into();
        assert_eq!(api_key(&s).as_deref(), Some("from-settings"));
        std::env::set_var("TMDB_API_KEY", "");
        s.tmdb_api_key = "  ".into();
        assert_eq!(api_key(&s), None);
    }

    #[test]
    fn matches_by_id_in_provider_order() {
        let got = match_apps(&[p(1899, "Max"), p(8, "Netflix"), p(9999, "Other")], &apps());
        assert_eq!(got, vec!["hbomax", "netflix"]);
    }

    #[test]
    fn matches_by_name_and_dedupes() {
        let got = match_apps(&[p(5, "globoplay"), p(307, "Globoplay")], &apps());
        assert_eq!(got, vec!["globoplay"]);
    }

    #[test]
    fn matches_plans_by_name() {
        let got = match_apps(
            &[p(2100, "Amazon Prime Video with Ads"), p(1796, "Netflix Standard with Ads"), p(1, "Netflixy")],
            &apps(),
        );
        assert_eq!(got, vec!["prime", "netflix"]);
    }

    #[test]
    fn paramount_opens_in_prime() {
        let got = match_apps(&[p(531, "Paramount Plus"), p(582, "Paramount+ Amazon Channel")], &apps());
        assert_eq!(got, vec!["prime"]);
    }

    #[test]
    fn parses_search_page() {
        let page: Page = serde_json::from_str(
            r#"{"results":[
                {"id":1,"media_type":"movie","title":"Dune","release_date":"2021-09-15","poster_path":"/a.jpg"},
                {"id":2,"media_type":"tv","name":"Dark","first_air_date":"2017-12-01"},
                {"id":3,"media_type":"person","name":"Someone"},
                {"id":4,"media_type":"movie","title":"Undated","release_date":""}
            ]}"#,
        )
        .unwrap();
        let titles: Vec<Title> = page.results.into_iter().filter_map(to_title).collect();
        assert_eq!(titles.len(), 3);
        assert_eq!(titles[0].year.as_deref(), Some("2021"));
        assert_eq!(titles[0].poster.as_deref(), Some("https://image.tmdb.org/t/p/w342/a.jpg"));
        assert_eq!(titles[1].title, "Dark");
        assert!(titles[2].year.is_none());
    }

    #[test]
    fn search_ranks_popular_prefix_matches_over_whole_word_ones() {
        let item = |title: &str, original: &str, popularity: f32| -> Item {
            serde_json::from_value(serde_json::json!({
                "id": 1, "title": title, "original_title": original, "popularity": popularity
            }))
            .unwrap()
        };
        let score = |i: &Item| search_score("sta", i);
        let obscure = item("Sta per piovere", "Sta per piovere", 1.0);
        let star_wars = item("Star Wars: The Clone Wars", "Star Wars: The Clone Wars", 97.0);
        let star_trek = item("Jornada nas Estrelas", "Star Trek", 94.0);
        assert!(score(&star_trek) > score(&obscure), "matches on the original name");
        let starring = item("The Tonight Show Starring Johnny Carson", "The Tonight Show Starring Johnny Carson", 397.0);
        assert!(score(&star_wars) > score(&obscure));
        assert!(score(&star_wars) > score(&starring), "the name starting with it outweighs popularity");
        let dune = item("Duna", "Dune", 48.0);
        let dune_three = item("Duna: Parte Três", "Dune: Part Three", 40.0);
        assert!(search_score("dune", &dune) > search_score("dune", &dune_three));
        assert_eq!(fold("  Star Wars: Rebels!"), "star wars rebels");
    }

    #[test]
    fn parses_series_details() {
        let raw: RawDetails = serde_json::from_str(
            r#"{"id":94605,"name":"Arcane","first_air_date":"2021-11-06","overview":"Two sisters.",
                "poster_path":"/p.jpg","backdrop_path":"/b.jpg","tagline":"",
                "genres":[{"id":16,"name":"Animação"}],"episode_run_time":[],
                "last_episode_to_air":{"runtime":41},
                "seasons":[{"season_number":0,"name":"Specials","episode_count":2},
                           {"season_number":1,"name":"Temporada 1","episode_count":9,"air_date":"2021-11-06"},
                           {"season_number":2,"name":"Temporada 2","episode_count":0}],
                "credits":{"cast":[{"id":1,"name":"Hailee Steinfeld","character":"Vi","profile_path":"/h.jpg"}]},
                "videos":{"results":[{"key":"teaser","site":"YouTube","type":"Teaser","official":true},
                                     {"key":"trailer","site":"YouTube","type":"Trailer","official":false}]},
                "images":{"logos":[{"file_path":"/en.png","iso_639_1":"en"},{"file_path":"/pt.png","iso_639_1":"pt"}]},
                "content_ratings":{"results":[{"iso_3166_1":"US","rating":"TV-14"},{"iso_3166_1":"BR","rating":"16"}]}}"#,
        )
        .unwrap();
        let d = to_details(raw, "tv", "BR", "pt");
        assert_eq!(d.title.title, "Arcane");
        assert_eq!(d.title.media_type, "tv");
        assert_eq!(d.tagline, None);
        assert_eq!(d.runtime, Some(41));
        assert_eq!(d.certification.as_deref(), Some("16"));
        assert_eq!(d.trailer.as_deref(), Some("trailer"));
        assert_eq!(d.logo.as_deref(), Some("https://image.tmdb.org/t/p/w500/pt.png"));
        assert_eq!(d.seasons.len(), 1);
        assert_eq!(d.seasons[0].year.as_deref(), Some("2021"));
        assert_eq!(d.cast[0].character, "Vi");
    }

    #[test]
    fn puts_directors_and_creators_first() {
        let raw: RawDetails = serde_json::from_str(
            r#"{"id":1,"title":"Dune","credits":{
                "cast":[{"id":10,"name":"Timothée Chalamet","character":"Paul"}],
                "crew":[{"id":20,"name":"Denis Villeneuve","job":"Director"},{"id":20,"name":"Denis Villeneuve","job":"Director"},
                        {"id":30,"name":"Hans Zimmer","job":"Original Music Composer"}]}}"#,
        )
        .unwrap();
        let d = to_details(raw, "movie", "BR", "pt");
        let names: Vec<(&str, Option<&str>)> = d.cast.iter().map(|c| (c.name.as_str(), c.job)).collect();
        assert_eq!(names, vec![("Denis Villeneuve", Some("director")), ("Timothée Chalamet", None)]);
        assert_eq!(d.cast[0].id, 20);

        let raw: RawDetails = serde_json::from_str(
            r#"{"id":2,"name":"Arcane","created_by":[{"id":40,"name":"Christian Linke","profile_path":"/c.jpg"}],
                "credits":{"cast":[],"crew":[{"id":41,"name":"Pascal Charrue","job":"Director"}]}}"#,
        )
        .unwrap();
        let d = to_details(raw, "tv", "BR", "pt");
        assert_eq!(d.cast.len(), 1);
        assert_eq!((d.cast[0].name.as_str(), d.cast[0].job), ("Christian Linke", Some("creator")));
    }

    #[test]
    fn parses_person() {
        let raw: RawPerson = serde_json::from_str(
            r#"{"id":1190668,"name":"Timothée Chalamet","known_for_department":"Acting","birthday":"1995-12-27",
                "deathday":null,"place_of_birth":"New York City, New York, USA","biography":" Actor. ",
                "profile_path":"/p.jpg","imdb_id":"nm3154303","external_ids":{"instagram_id":"tchalamet"},
                "combined_credits":{
                  "cast":[
                    {"id":438631,"media_type":"movie","title":"Dune","character":"Paul Atreides","release_date":"2021-09-15","poster_path":"/d.jpg","vote_count":13000},
                    {"id":1396,"media_type":"tv","name":"Homeland","character":"Finn Walden","first_air_date":"2011-10-02","poster_path":"/h.jpg","vote_count":5000,"episode_count":2},
                    {"id":2,"media_type":"tv","name":"The Tonight Show","character":"Self - Guest","first_air_date":"2014-02-17","poster_path":"/t.jpg","vote_count":900,"genre_ids":[10767]},
                    {"id":3,"media_type":"movie","title":"Oscars","character":"Self","release_date":"2018-03-04","poster_path":"/o.jpg","vote_count":100},
                    {"id":4,"media_type":"movie","title":"Untitled","character":"Lead","release_date":""},
                    {"id":5,"media_type":"movie","title":"Wonka","character":"Willy Wonka","release_date":"2023-12-06","poster_path":"/w.jpg","vote_count":4000}
                  ],
                  "crew":[
                    {"id":6,"media_type":"movie","title":"A Complete Unknown","job":"Producer","department":"Production","release_date":"2024-12-18","vote_count":1000},
                    {"id":6,"media_type":"movie","title":"A Complete Unknown","job":"Executive Producer","department":"Production","release_date":"2024-12-18","vote_count":1000}
                  ]}}"#,
        )
        .unwrap();
        let p = to_person(raw);
        assert_eq!(p.bio, "Actor.");
        assert_eq!(p.photo.as_deref(), Some("https://image.tmdb.org/t/p/w500/p.jpg"));
        let known: Vec<&str> = p.known_for.iter().map(|c| c.title.title.as_str()).collect();
        // Talk shows and playing himself left out; two episodes count for little.
        assert_eq!(known, vec!["Dune", "Wonka", "Homeland"]);
        let credits: Vec<(&str, &str, bool)> = p.credits.iter().map(|c| (c.title.title.as_str(), c.role.as_str(), c.crew)).collect();
        assert_eq!(
            credits,
            vec![
                ("Untitled", "Lead", false),
                ("A Complete Unknown", "Producer, Executive Producer", true),
                ("Wonka", "Willy Wonka", false),
                ("Dune", "Paul Atreides", false),
                ("Homeland", "Finn Walden", false),
            ]
        );
    }

    #[test]
    fn a_director_is_known_for_directing() {
        let raw: RawPerson = serde_json::from_str(
            r#"{"id":137427,"name":"Denis Villeneuve","known_for_department":"Directing",
                "combined_credits":{
                  "cast":[{"id":9,"media_type":"movie","title":"A Cameo","character":"Man","poster_path":"/c.jpg","vote_count":99999}],
                  "crew":[
                    {"id":438631,"media_type":"movie","title":"Dune","job":"Director","department":"Directing","poster_path":"/d.jpg","vote_count":13000},
                    {"id":438631,"media_type":"movie","title":"Dune","job":"Screenplay","department":"Writing","poster_path":"/d.jpg","vote_count":13000},
                    {"id":335984,"media_type":"movie","title":"Blade Runner 2049","job":"Director","department":"Directing","poster_path":"/b.jpg","vote_count":14000}
                  ]}}"#,
        )
        .unwrap();
        let p = to_person(raw);
        let known: Vec<(&str, &str)> = p.known_for.iter().map(|c| (c.title.title.as_str(), c.role.as_str())).collect();
        assert_eq!(known, vec![("Blade Runner 2049", "Director"), ("Dune", "Director")]);
        assert_eq!(p.credits.iter().find(|c| c.title.title == "Dune").unwrap().role, "Director, Screenplay");
    }

    #[test]
    fn parses_movie_certification() {
        let raw: RawDetails = serde_json::from_str(
            r#"{"id":1,"title":"Dune","runtime":166,
                "release_dates":{"results":[{"iso_3166_1":"BR","release_dates":[{"certification":""},{"certification":"14"}]}]}}"#,
        )
        .unwrap();
        let d = to_details(raw, "movie", "br", "pt");
        assert_eq!(d.certification.as_deref(), Some("14"));
        assert_eq!(d.runtime, Some(166));
        assert!(d.seasons.is_empty());
    }

    #[test]
    fn parses_season() {
        let page: RawSeasonPage = serde_json::from_str(
            r#"{"episodes":[{"season_number":1,"episode_number":1,"name":"Welcome","still_path":"/s.jpg","runtime":43,"air_date":""},
                            {"season_number":1,"episode_number":2,"name":null}]}"#,
        )
        .unwrap();
        let eps = to_episodes(page);
        assert_eq!(eps.len(), 2);
        assert_eq!(eps[0].still.as_deref(), Some("https://image.tmdb.org/t/p/w400/s.jpg"));
        assert_eq!(eps[0].air_date, None);
        assert_eq!(eps[1].name, "");
    }
}
