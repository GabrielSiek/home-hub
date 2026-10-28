//! Recommendation rows for the launcher's Home and the streaming services' homes, from what
//! the user watched (watch store), saved (My list) and rated (ratings.json). Every row keeps
//! only titles that stream on the user's own services (apps.toml's `tmdb_providers`).
//!
//! The UI asks for the shelves first (`shelves`: which rows, picked anew each run), then for
//! each row's titles (`row`). The profile is rebuilt on every call: it is all local data, and
//! the TMDB answers behind it are cached here and in tmdb.rs.

pub mod profile;
pub mod ratings;

use std::collections::HashMap;
use std::sync::Mutex;

use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::config::{AppEntry, Settings};
use crate::error::{Error, Result};
use crate::history::now_ms;
use crate::state::AppState;
use crate::tmdb::{self, Candidate, Discover, Meta, Provider, Title};

use profile::{Kind, PlanInput, Profile, Rng, Seed, Shelf};
pub use ratings::{Rating, Ratings, Score};

/// Titles in a row.
const ROW: usize = 20;
/// A row with fewer titles than this is topped up from the user's genres.
const THIN: usize = 8;
/// At most this many titles are checked for where they stream, per row.
const MAX_CHECKS: usize = 80;
/// Seeds whose genres and cast are looked up.
const META_SEEDS: usize = 16;
/// Who streams a title changes slowly; genres and cast hardly ever.
const OFFERS_TTL_MS: i64 = 24 * 3_600_000;
const META_TTL_MS: i64 = 14 * 24 * 3_600_000;
const OFFERS_MAX: usize = 6000;
const META_MAX: usize = 1500;

pub struct RecsState {
    pub ratings: Mutex<Ratings>,
    /// Picks the shelves; fixed for the run, so a screen keeps its rows until the next start.
    session: u64,
    cache: Mutex<DiskCache>,
}

/// TMDB answers worth keeping across runs, in recs-cache.json.
#[derive(Default, Serialize, Deserialize)]
struct DiskCache {
    /// "BR:movie:603" → (checked at, who streams it there).
    #[serde(default)]
    offers: HashMap<String, (i64, Vec<Provider>)>,
    /// "movie:603" → (looked up at, genres and cast).
    #[serde(default)]
    meta: HashMap<String, (i64, Meta)>,
    #[serde(skip)]
    dirty: bool,
}

impl DiskCache {
    fn load(dir: &std::path::Path) -> Self {
        std::fs::read_to_string(dir.join("recs-cache.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Drops the oldest entries past `max`.
    fn trim<T>(map: &mut HashMap<String, (i64, T)>, max: usize) {
        if map.len() <= max {
            return;
        }
        let mut ages: Vec<i64> = map.values().map(|v| v.0).collect();
        ages.sort_unstable();
        let cut = ages[map.len() - max];
        map.retain(|_, v| v.0 >= cut);
    }
}

impl RecsState {
    pub fn new(dir: &std::path::Path) -> Self {
        Self {
            ratings: Mutex::new(Ratings::load(dir)),
            session: now_ms() as u64,
            cache: Mutex::new(DiskCache::load(dir)),
        }
    }
}

fn save_cache(st: &AppState) {
    let text = {
        let mut c = st.recs.cache.lock().unwrap();
        if !c.dirty {
            return;
        }
        DiskCache::trim(&mut c.offers, OFFERS_MAX);
        DiskCache::trim(&mut c.meta, META_MAX);
        c.dirty = false;
        serde_json::to_string(&*c)
    };
    if let Ok(text) = text {
        let _ = std::fs::write(st.data_dir.join("recs-cache.json"), text);
    }
}

/// A recommended title and the user's apps that stream it (empty when not checked).
#[derive(Debug, Clone, Serialize)]
pub struct RecTitle {
    #[serde(flatten)]
    pub title: Title,
    pub services: Vec<String>,
}

/// Where a row's titles may stream: one service, or all of the user's (Home).
struct Scope {
    settings: Settings,
    providers: Vec<u32>,
    apps: Vec<AppEntry>,
}

fn scope(st: &AppState, service: Option<&str>) -> Result<Scope> {
    let cfg = st.config.read().unwrap();
    let apps: Vec<AppEntry> = cfg
        .apps
        .iter()
        .filter(|a| !a.tmdb_providers.is_empty() && service.is_none_or(|s| a.id == s))
        .cloned()
        .collect();
    if let Some(s) = service {
        if apps.is_empty() {
            return Err(Error::msg(format!("'{s}' has no TMDB providers in apps.toml")));
        }
    }
    let mut providers: Vec<u32> = apps.iter().flat_map(|a| a.tmdb_providers.iter().copied()).collect();
    providers.sort_unstable();
    providers.dedup();
    Ok(Scope {
        settings: cfg.settings.clone(),
        providers,
        apps,
    })
}

/// Everything a row is chosen from.
struct Taste {
    profile: Profile,
    genres: Vec<(u32, f32)>,
    people: Vec<(u64, String, f32)>,
    meta: HashMap<(String, u64), Meta>,
}

async fn taste(st: &AppState, settings: &Settings) -> Taste {
    let profile = {
        let store = st.watch.store.lock().unwrap();
        let ratings = st.recs.ratings.lock().unwrap();
        profile::build(&store.progress, &store.watchlist, &ratings.list, now_ms())
    };
    // The strongest likes and dislikes say the most about genres and actors.
    let mut wanted: Vec<&Seed> = profile.positive().take(META_SEEDS).collect();
    wanted.extend(profile.seeds.iter().rev().filter(|s| s.weight < 0.0).take(4));
    let meta = metas(st, settings, &wanted).await;
    let genres = profile::genre_affinity(&profile.seeds, &meta);
    let people = profile::favorite_people(&profile.seeds, &meta);
    Taste { profile, genres, people, meta }
}

async fn metas(st: &AppState, settings: &Settings, seeds: &[&Seed]) -> HashMap<(String, u64), Meta> {
    let now = now_ms();
    let mut out = HashMap::new();
    let mut missing = Vec::new();
    {
        let c = st.recs.cache.lock().unwrap();
        for s in seeds {
            match c.meta.get(&format!("{}:{}", s.media_type, s.id)) {
                Some((at, m)) if now - at < META_TTL_MS => {
                    out.insert(s.key(), m.clone());
                }
                _ => missing.push(s.key()),
            }
        }
    }
    let found = join_all(missing.iter().map(|(m, id)| tmdb::meta(&st.http, settings, m, *id))).await;
    let mut c = st.recs.cache.lock().unwrap();
    for (key, got) in missing.into_iter().zip(found) {
        if let Ok(m) = got {
            c.meta.insert(format!("{}:{}", key.0, key.1), (now, m.clone()));
            c.dirty = true;
            out.insert(key, m);
        }
    }
    out
}

/// Who streams a title in the settings' region; None when TMDB couldn't say.
async fn offers(st: &AppState, settings: &Settings, media_type: &str, id: u64) -> Option<Vec<Provider>> {
    let key = format!("{}:{media_type}:{id}", settings.region.to_uppercase());
    let now = now_ms();
    if let Some((at, list)) = st.recs.cache.lock().unwrap().offers.get(&key) {
        if now - at < OFFERS_TTL_MS {
            return Some(list.clone());
        }
    }
    let list = tmdb::offers_fresh(&st.http, settings, media_type, id).await.ok()?;
    let mut c = st.recs.cache.lock().unwrap();
    c.offers.insert(key, (now, list.clone()));
    c.dirty = true;
    Some(list)
}

/// The candidates that stream on the scope's services, in order, until `want` are found.
async fn available(st: &AppState, sc: &Scope, taste: &Taste, list: Vec<Candidate>, want: usize) -> Vec<RecTitle> {
    // Each title once: an actor's credits list a title per role, trending pages overlap.
    let mut seen = std::collections::HashSet::new();
    let list: Vec<Candidate> = list
        .into_iter()
        .filter(|c| !taste.profile.excluded(c) && (c.title.poster.is_some() || c.title.backdrop.is_some()))
        .filter(|c| seen.insert((c.title.media_type.clone(), c.title.id)))
        .take(MAX_CHECKS)
        .collect();
    let mut out = Vec::new();
    for chunk in list.chunks(10) {
        let checks = chunk.iter().map(|c| offers(st, &sc.settings, &c.title.media_type, c.title.id));
        for (c, found) in chunk.iter().zip(join_all(checks).await) {
            let Some(found) = found else { continue };
            if !found.iter().any(|p| sc.providers.contains(&p.provider_id)) {
                continue;
            }
            out.push(RecTitle { title: c.title.clone(), services: tmdb::match_apps(&found, &sc.apps) });
            if out.len() >= want {
                return out;
            }
        }
    }
    out
}

/// The rows for a service's home (`service`) or the launcher's Home (None).
pub async fn shelves(app: &AppHandle, service: Option<&str>) -> Result<Vec<Shelf>> {
    let st = app.state::<AppState>();
    let sc = scope(&st, service)?;
    let taste = taste(&st, &sc.settings).await;
    save_cache(&st);
    let input = PlanInput {
        profile: &taste.profile,
        genres: &taste.genres,
        people: &taste.people,
        service,
    };
    // Each screen draws its own rows, the same ones for the whole run.
    let salt = service.unwrap_or("home").bytes().fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(b as u64));
    Ok(profile::plan(&input, &mut Rng::new(st.recs.session ^ salt)))
}

/// The titles of one row.
pub async fn row(app: &AppHandle, service: Option<&str>, shelf: &Shelf) -> Result<Vec<RecTitle>> {
    let st = app.state::<AppState>();
    let sc = scope(&st, service)?;
    let taste = taste(&st, &sc.settings).await;
    let out = fill(&st, &sc, &taste, shelf).await;
    save_cache(&st);
    out
}

async fn fill(st: &AppState, sc: &Scope, taste: &Taste, shelf: &Shelf) -> Result<Vec<RecTitle>> {
    let s = &sc.settings;
    let http = &st.http;
    let top_genres: Vec<u32> = taste.genres.iter().take(3).map(|g| g.0).collect();
    match shelf.kind {
        Kind::ForYou => {
            let mut out = available(st, sc, taste, for_you(st, sc, taste).await, ROW).await;
            top_up(st, sc, taste, &top_genres, &mut out).await;
            Ok(out)
        }
        Kind::Because | Kind::Liked | Kind::Loved => {
            let media = shelf.media_type.as_deref().ok_or_else(|| Error::msg("shelf without a title"))?;
            let id = shelf.id.ok_or_else(|| Error::msg("shelf without a title"))?;
            let (recs, similar) = futures_util::join!(
                tmdb::related(http, s, media, id, "recommendations"),
                tmdb::related(http, s, media, id, "similar"),
            );
            let lists = vec![(1.0, recs.unwrap_or_default()), (0.5, similar.unwrap_or_default())];
            let ranked = profile::rerank(profile::blend(lists).into_iter().map(|x| x.0).collect(), &taste.genres);
            let mut out = available(st, sc, taste, ranked, ROW).await;
            // Thin: more of the seed's own genres.
            let seed_genres: Vec<u32> = taste
                .meta
                .get(&(media.to_string(), id))
                .map(|m| m.genres.iter().map(|&g| profile::canonical(g)).take(2).collect())
                .unwrap_or_else(|| top_genres.clone());
            top_up(st, sc, taste, &seed_genres, &mut out).await;
            Ok(out)
        }
        Kind::Trending | Kind::TrendingDay => {
            let window = if shelf.kind == Kind::Trending { "week" } else { "day" };
            let pages = join_all((1..=3).map(|p| tmdb::trending(http, s, window, p))).await;
            let list: Vec<Candidate> = pages.into_iter().flat_map(|p| p.unwrap_or_default()).collect();
            Ok(available(st, sc, taste, list, ROW).await)
        }
        Kind::Genre => {
            let g = shelf.id.ok_or_else(|| Error::msg("shelf without a genre"))? as u32;
            let list = discover_both(st, sc, |m| {
                profile::for_media(g, m).map(|genre| Discover { genre: Some(genre), min_votes: Some(80), ..query(m) })
            })
            .await;
            Ok(available(st, sc, taste, profile::rerank(list, &taste.genres), ROW).await)
        }
        Kind::Person => {
            let person = shelf.id.ok_or_else(|| Error::msg("shelf without a person"))?;
            let mut list: Vec<Candidate> = tmdb::person_credits(http, s, person)
                .await?
                .into_iter()
                // Talk shows, news and award nights are appearances, not roles.
                .filter(|c| c.votes >= 30 && !c.genres.iter().any(|g| [10767, 10763, 10764].contains(g)))
                .collect();
            list.sort_by(|a, b| b.popularity.total_cmp(&a.popularity));
            Ok(available(st, sc, taste, list, ROW).await)
        }
        Kind::Fresh => {
            let list = discover_both(st, sc, |m| {
                Some(Discover {
                    since_years: Some(1),
                    min_votes: Some(20),
                    any_genres: genres_for(&top_genres, m),
                    ..query(m)
                })
            })
            .await;
            Ok(available(st, sc, taste, profile::rerank(list, &taste.genres), ROW).await)
        }
        Kind::Gems => {
            let list = discover_both(st, sc, |m| {
                Some(Discover {
                    sort: Some("vote_average.desc".into()),
                    min_votes: Some(if m == "tv" { 80 } else { 150 }),
                    max_votes: Some(if m == "tv" { 800 } else { 2500 }),
                    any_genres: genres_for(&top_genres, m),
                    ..query(m)
                })
            })
            .await;
            Ok(available(st, sc, taste, list, ROW).await)
        }
        Kind::Acclaimed => {
            let list = discover_both(st, sc, |m| {
                Some(Discover {
                    sort: Some("vote_average.desc".into()),
                    min_votes: Some(if m == "tv" { 500 } else { 1500 }),
                    any_genres: genres_for(&top_genres, m),
                    ..query(m)
                })
            })
            .await;
            Ok(available(st, sc, taste, list, ROW).await)
        }
        Kind::Again => {
            let seen: Vec<Candidate> = taste
                .profile
                .positive()
                .filter(|x| x.seen)
                .map(|x| Candidate {
                    title: seed_title(x),
                    genres: Vec::new(),
                    rating: 0.0,
                    votes: 0,
                    popularity: 0.0,
                })
                .collect();
            // Watched already, so `available`'s exclusion doesn't apply here.
            let mut out = Vec::new();
            for c in seen.into_iter().take(MAX_CHECKS) {
                let Some(found) = offers(st, s, &c.title.media_type, c.title.id).await else { continue };
                if found.iter().any(|p| sc.providers.contains(&p.provider_id)) {
                    out.push(RecTitle { services: tmdb::match_apps(&found, &sc.apps), title: c.title });
                }
                if out.len() >= ROW {
                    break;
                }
            }
            Ok(out)
        }
    }
}

/// Everything the user likes, blended: TMDB's recommendations and similar titles for the
/// strongest likes, minus what resembles the dislikes, favoring the user's genres. Best first,
/// not yet checked for where it streams.
async fn for_you(st: &AppState, sc: &Scope, taste: &Taste) -> Vec<Candidate> {
    let (http, s) = (&st.http, &sc.settings);
    let pos: Vec<&Seed> = taste.profile.positive().take(8).collect();
    let neg: Vec<&Seed> = taste.profile.seeds.iter().rev().filter(|x| x.weight < 0.0).take(4).collect();
    let mut asks = Vec::new();
    for seed in &pos {
        asks.push((seed.weight, seed, "recommendations"));
        asks.push((seed.weight * 0.5, seed, "similar"));
    }
    for seed in &neg {
        asks.push((seed.weight * 0.6, seed, "recommendations"));
    }
    let lists = join_all(asks.iter().map(|(_, seed, kind)| tmdb::related(http, s, &seed.media_type, seed.id, kind))).await;
    let weighted = asks.iter().zip(lists).filter_map(|((w, _, _), l)| l.ok().map(|l| (*w, l))).collect();
    let mut scored: Vec<(Candidate, f32)> = profile::blend(weighted)
        .into_iter()
        .filter(|(_, v)| *v > 0.0)
        .map(|(c, v)| {
            let fit = profile::genre_fit(&c.genres, &taste.genres);
            let quality = ((c.rating - 6.5) / 10.0).clamp(-0.1, 0.1);
            let score = v * (0.7 + 0.6 * fit) + quality;
            (c, score)
        })
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    scored.into_iter().map(|x| x.0).collect()
}

/// One row of a genre's page, on any of the user's services. `sort`: "for_you" (the user's
/// recommendations in that genre, then its popular titles that fit them best), "popular",
/// "new" or "acclaimed". Only "for_you" leaves out what the user already watched.
pub async fn genre(app: &AppHandle, genre: u32, sort: &str) -> Result<Vec<RecTitle>> {
    let st = app.state::<AppState>();
    let sc = scope(&st, None)?;
    let taste = taste(&st, &sc.settings).await;
    let g = profile::canonical(genre);
    let in_genre = |m: &str, more: Discover| profile::for_media(g, m).map(|id| Discover { genre: Some(id), ..more });
    let out = match sort {
        "for_you" => {
            let mine: Vec<Candidate> = for_you(&st, &sc, &taste)
                .await
                .into_iter()
                .filter(|c| c.genres.iter().any(|&x| profile::canonical(x) == g))
                .collect();
            let mut out = available(&st, &sc, &taste, mine, ROW).await;
            if out.len() < ROW {
                let popular = discover_both(&st, &sc, |m| in_genre(m, Discover { min_votes: Some(80), ..query(m) })).await;
                let have: Vec<(String, u64)> = out.iter().map(|r| (r.title.media_type.clone(), r.title.id)).collect();
                let more: Vec<Candidate> = profile::rerank(popular, &taste.genres)
                    .into_iter()
                    .filter(|c| !have.contains(&(c.title.media_type.clone(), c.title.id)))
                    .collect();
                let want = ROW - out.len();
                out.extend(available(&st, &sc, &taste, more, want).await);
            }
            out
        }
        "popular" | "new" | "acclaimed" => {
            let list = discover_both(&st, &sc, |m| {
                let more = match sort {
                    "popular" => Discover { min_votes: Some(80), ..query(m) },
                    "new" => Discover { since_years: Some(1), min_votes: Some(20), ..query(m) },
                    _ => Discover {
                        sort: Some("vote_average.desc".into()),
                        min_votes: Some(if m == "tv" { 300 } else { 1000 }),
                        ..query(m)
                    },
                };
                in_genre(m, more)
            })
            .await;
            // Discover only returns titles on the user's services: checking where each streams
            // just names the apps, so the watched ones may stay.
            let browse = Taste { profile: Profile::default(), ..taste };
            available(&st, &sc, &browse, list, ROW).await
        }
        other => return Err(Error::msg(format!("unknown genre row '{other}'"))),
    };
    save_cache(&st);
    Ok(out)
}

fn seed_title(s: &Seed) -> Title {
    Title {
        id: s.id,
        media_type: s.media_type.clone(),
        title: s.title.clone(),
        year: None,
        overview: String::new(),
        poster: s.poster.clone(),
        backdrop: s.backdrop.clone(),
    }
}

fn query(media_type: &str) -> Discover {
    Discover {
        media_type: media_type.to_string(),
        genre: None,
        sort: None,
        page: None,
        since_years: None,
        min_votes: None,
        max_votes: None,
        any_genres: None,
    }
}

/// The user's top genres in `media_type`'s numbers; None (any genre) when there are none.
fn genres_for(top: &[u32], media_type: &str) -> Option<Vec<u32>> {
    let mut out: Vec<u32> = top.iter().filter_map(|&g| profile::for_media(g, media_type)).collect();
    out.dedup();
    (!out.is_empty()).then_some(out)
}

/// Movies and series side by side, from discover on the scope's services; `make` returns None
/// to leave one side out.
async fn discover_both(st: &AppState, sc: &Scope, make: impl Fn(&str) -> Option<Discover>) -> Vec<Candidate> {
    let queries: Vec<Discover> = ["movie", "tv"].iter().filter_map(|m| make(m)).collect();
    let lists = join_all(queries.iter().map(|q| tmdb::discover_candidates(&st.http, &sc.settings, &sc.providers, q))).await;
    profile::interleave(lists.into_iter().map(|l| l.unwrap_or_default()).collect())
}

/// Fills a thin row with popular titles in `genres` on the scope's services.
async fn top_up(st: &AppState, sc: &Scope, taste: &Taste, genres: &[u32], out: &mut Vec<RecTitle>) {
    if out.len() >= THIN {
        return;
    }
    let list = discover_both(st, sc, |m| {
        Some(Discover { any_genres: genres_for(genres, m), min_votes: Some(100), ..query(m) })
    })
    .await;
    let have: Vec<(String, u64)> = out.iter().map(|r| (r.title.media_type.clone(), r.title.id)).collect();
    let more: Vec<Candidate> = profile::rerank(list, &taste.genres)
        .into_iter()
        .filter(|c| !have.contains(&(c.title.media_type.clone(), c.title.id)))
        .collect();
    let want = ROW - out.len();
    out.extend(available(st, sc, taste, more, want).await);
}

/// The user's rating of a title, if any.
pub fn rating(st: &AppState, media_type: &str, id: u64) -> Option<Score> {
    st.recs.ratings.lock().unwrap().get(media_type, id)
}

/// Rates a title (`None` takes the rating away) and returns what it is now.
pub fn rate(st: &AppState, mut item: Rating, score: Option<Score>) -> Result<Option<Score>> {
    item.at = now_ms();
    let mut r = st.recs.ratings.lock().unwrap();
    r.set(item, score);
    r.save(&st.data_dir)?;
    Ok(score)
}
