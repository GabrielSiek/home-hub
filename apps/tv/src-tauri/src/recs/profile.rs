//! What the user likes, from what they watched, saved and rated, and which recommendation
//! rows (shelves) to show for it. No I/O here: recs/mod.rs fetches, this decides.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::ratings::{Rating, Score};
use crate::tmdb::{Candidate, Meta};
use crate::watch::{ListItem, Progress};

const DAY_MS: i64 = 86_400_000;

/// What a seed's weight mostly comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Watched,
    Rated(Score),
    Listed,
}

/// A title the user did something with: recommendations start from these.
#[derive(Debug, Clone)]
pub struct Seed {
    pub media_type: String,
    pub id: u64,
    pub title: String,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    /// Above 0: more like this. Below 0: less like this.
    pub weight: f32,
    pub source: Source,
    /// Unix ms of the last thing the user did with it.
    pub at: i64,
    /// Watched through (or most of a series), or rated well: fits "Watch it again".
    pub seen: bool,
    /// The services it was watched or saved on.
    pub services: Vec<String>,
}

impl Seed {
    pub fn key(&self) -> (String, u64) {
        (self.media_type.clone(), self.id)
    }
}

#[derive(Debug, Default)]
pub struct Profile {
    /// Strongest first.
    pub seeds: Vec<Seed>,
    /// Not recommended: anything started, finished, rated or saved (Home has a watchlist row).
    pub exclude: HashSet<(String, u64)>,
}

impl Profile {
    pub fn positive(&self) -> impl Iterator<Item = &Seed> {
        self.seeds.iter().filter(|s| s.weight > 0.0)
    }

    pub fn excluded(&self, c: &Candidate) -> bool {
        self.exclude.contains(&(c.title.media_type.clone(), c.title.id))
    }
}

/// 1 now, halving every `half_life` days, never below `floor`.
fn decay(age_ms: i64, half_life: f32, floor: f32) -> f32 {
    let days = age_ms.max(0) as f32 / DAY_MS as f32;
    0.5f32.powf(days / half_life).max(floor)
}

/// Episodes of a series the user got through.
fn episodes_watched(p: &Progress) -> u32 {
    let done = p.episodes.values().filter(|&&s| s >= 0.92).count() as u32;
    let before = if p.confirmed && p.season > 0 {
        p.seasons.iter().take(p.season as usize - 1).sum::<u32>() + p.episode.saturating_sub(1)
    } else {
        0
    };
    done.max(before)
}

/// What watching says about liking a title, before time fades it. Started and dropped weeks
/// ago counts a little against it.
fn engagement(p: &Progress, now: i64) -> f32 {
    let stale = now - p.updated_at > 21 * DAY_MS;
    if p.media_type == "tv" {
        if p.finished {
            return 2.0;
        }
        match episodes_watched(p) {
            n if n >= 3 => 1.0 + n.min(12) as f32 / 12.0,
            0 | 1 if stale => -0.4,
            0 => 0.4,
            _ => 0.7,
        }
    } else {
        if p.finished {
            return 1.5;
        }
        match p.current_share() {
            s if s >= 0.5 => 1.0,
            s if s >= 0.2 => 0.7,
            _ if stale => -0.5,
            _ => 0.4,
        }
    }
}

impl Score {
    pub fn weight(self) -> f32 {
        match self {
            Score::Dislike => -3.0,
            Score::Like => 2.0,
            Score::Love => 3.5,
        }
    }
}

/// The user's taste from the watch store and their ratings. A rating outweighs watching.
pub fn build(progress: &[Progress], list: &[ListItem], ratings: &[Rating], now: i64) -> Profile {
    let mut seeds: HashMap<(String, u64), Seed> = HashMap::new();
    let mut exclude = HashSet::new();

    for p in progress {
        let key = (p.media_type.clone(), p.id);
        exclude.insert(key.clone());
        let weight = engagement(p, now) * decay(now - p.updated_at, 90.0, 0.25);
        let seen = p.finished || (p.media_type == "tv" && episodes_watched(p) >= 3);
        let s = seeds.entry(key).or_insert_with(|| Seed {
            media_type: p.media_type.clone(),
            id: p.id,
            title: p.title.clone(),
            poster: p.poster.clone(),
            backdrop: p.backdrop.clone(),
            weight,
            source: Source::Watched,
            at: p.updated_at,
            seen,
            services: Vec::new(),
        });
        // The same title on two services: the one watched more counts.
        s.weight = s.weight.max(weight);
        s.at = s.at.max(p.updated_at);
        s.seen |= seen;
        if !s.services.contains(&p.service) {
            s.services.push(p.service.clone());
        }
    }

    for l in list {
        exclude.insert((l.media_type.clone(), l.id));
        let bonus = 0.5 * decay(now - l.added_at, 120.0, 0.3);
        let s = seeds.entry((l.media_type.clone(), l.id)).or_insert_with(|| Seed {
            media_type: l.media_type.clone(),
            id: l.id,
            title: l.title.clone(),
            poster: l.poster.clone(),
            backdrop: l.backdrop.clone(),
            weight: 0.0,
            source: Source::Listed,
            at: l.added_at,
            seen: false,
            services: Vec::new(),
        });
        s.weight += bonus;
        if !s.services.contains(&l.service) {
            s.services.push(l.service.clone());
        }
    }

    for r in ratings {
        let key = (r.media_type.clone(), r.id);
        exclude.insert(key.clone());
        let weight = r.score.weight() * decay(now - r.at, 180.0, 0.5);
        let s = seeds.entry(key).or_insert_with(|| Seed {
            media_type: r.media_type.clone(),
            id: r.id,
            title: r.title.clone(),
            poster: r.poster.clone(),
            backdrop: r.backdrop.clone(),
            weight,
            source: Source::Rated(r.score),
            at: r.at,
            seen: false,
            services: Vec::new(),
        });
        s.weight = weight;
        s.source = Source::Rated(r.score);
        s.at = s.at.max(r.at);
        s.seen = r.score != Score::Dislike;
        if s.poster.is_none() {
            s.poster = r.poster.clone();
        }
        if s.backdrop.is_none() {
            s.backdrop = r.backdrop.clone();
        }
    }

    let mut seeds: Vec<Seed> = seeds.into_values().filter(|s| s.weight.abs() > 0.01).collect();
    seeds.sort_by(|a, b| b.weight.total_cmp(&a.weight).then(b.at.cmp(&a.at)));
    Profile { seeds, exclude }
}

// ---------- Genres ----------

/// Movies and series number some genres differently; affinity is kept in movie numbers.
pub fn canonical(genre: u32) -> u32 {
    match genre {
        10759 => 28,    // Action & Adventure
        10765 => 878,   // Sci-Fi & Fantasy
        10768 => 10752, // War & Politics
        g => g,
    }
}

/// A canonical genre in `media_type`'s numbers; None when that side has no such genre.
pub fn for_media(genre: u32, media_type: &str) -> Option<u32> {
    if media_type == "movie" {
        return match genre {
            10762 => Some(10751), // Kids → Family
            10763 | 10764 | 10766 | 10767 => None,
            g => Some(g),
        };
    }
    match genre {
        28 | 12 => Some(10759),
        878 | 14 => Some(10765),
        10752 => Some(10768),
        27 | 10749 | 53 | 36 | 10402 | 10770 => None,
        g => Some(g),
    }
}

/// Genres everything has a bit of count less; some say nothing about taste.
fn genre_damp(genre: u32) -> f32 {
    match genre {
        18 => 0.6,
        35 => 0.85,
        10770 | 10763 | 10767 => 0.0,
        _ => 1.0,
    }
}

/// How much the user likes each genre, strongest first; only the ones they like.
pub fn genre_affinity(seeds: &[Seed], meta: &HashMap<(String, u64), Meta>) -> Vec<(u32, f32)> {
    let mut aff: HashMap<u32, f32> = HashMap::new();
    for s in seeds {
        let Some(m) = meta.get(&s.key()) else { continue };
        let mut genres: Vec<u32> = m.genres.iter().map(|&g| canonical(g)).collect();
        genres.sort_unstable();
        genres.dedup();
        if genres.is_empty() {
            continue;
        }
        let share = s.weight / (genres.len() as f32).sqrt();
        for g in genres {
            *aff.entry(g).or_default() += share * genre_damp(g);
        }
    }
    let mut out: Vec<(u32, f32)> = aff.into_iter().filter(|(_, v)| *v > 0.05).collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

/// How well a title's genres fit the user's, 0..1.
pub fn genre_fit(genres: &[u32], affinity: &[(u32, f32)]) -> f32 {
    let Some(top) = affinity.first().map(|a| a.1).filter(|t| *t > 0.0) else {
        return 0.0;
    };
    if genres.is_empty() {
        return 0.0;
    }
    let sum: f32 = genres
        .iter()
        .filter_map(|&g| affinity.iter().find(|a| a.0 == canonical(g)).map(|a| a.1 / top))
        .sum();
    (sum / (genres.len() as f32).sqrt()).min(1.0)
}

/// Actors who keep coming up in what the user likes: (person id, name, weight). One loved
/// title is enough; otherwise two.
pub fn favorite_people(seeds: &[Seed], meta: &HashMap<(String, u64), Meta>) -> Vec<(u64, String, f32)> {
    let mut people: HashMap<u64, (String, f32, u32, bool)> = HashMap::new();
    for s in seeds.iter().filter(|s| s.weight > 0.0) {
        let Some(m) = meta.get(&s.key()) else { continue };
        for (id, name) in m.cast.iter().take(3) {
            let e = people.entry(*id).or_insert_with(|| (name.clone(), 0.0, 0, false));
            e.1 += s.weight;
            e.2 += 1;
            e.3 |= s.source == Source::Rated(Score::Love);
        }
    }
    let mut out: Vec<(u64, String, f32)> = people
        .into_iter()
        .filter(|(_, (_, _, n, loved))| *n >= 2 || *loved)
        .map(|(id, (name, w, _, _))| (id, name, w))
        .collect();
    out.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)));
    out
}

// ---------- Ranking ----------

/// Weighted lists of candidates merged into one score per title: earlier in a list counts
/// more, and a title several lists agree on adds up. Negative weights push titles down.
pub fn blend(lists: Vec<(f32, Vec<Candidate>)>) -> Vec<(Candidate, f32)> {
    let mut scores: HashMap<(String, u64), (Candidate, f32)> = HashMap::new();
    for (weight, list) in lists {
        for (rank, c) in list.into_iter().enumerate() {
            let add = weight / (1.0 + 0.1 * rank as f32);
            let key = (c.title.media_type.clone(), c.title.id);
            scores.entry(key).or_insert_with(|| (c, 0.0)).1 += add;
        }
    }
    let mut out: Vec<(Candidate, f32)> = scores.into_values().collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.title.id.cmp(&b.0.title.id)));
    out
}

/// Takes from each list in turn (movies and series side by side), each title once.
pub fn interleave(lists: Vec<Vec<Candidate>>) -> Vec<Candidate> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let longest = lists.iter().map(Vec::len).max().unwrap_or(0);
    for i in 0..longest {
        for l in &lists {
            if let Some(c) = l.get(i) {
                if seen.insert((c.title.media_type.clone(), c.title.id)) {
                    out.push(c.clone());
                }
            }
        }
    }
    out
}

/// Reorders a list by fit with the user's genres, keeping much of its own order.
pub fn rerank(list: Vec<Candidate>, affinity: &[(u32, f32)]) -> Vec<Candidate> {
    let mut scored: Vec<(f32, Candidate)> = list
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            let base = 1.0 / (1.0 + 0.06 * i as f32);
            let quality = ((c.rating - 6.5) / 10.0).clamp(-0.1, 0.1);
            (base * (0.6 + 0.4 * genre_fit(&c.genres, affinity)) + quality, c)
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.into_iter().map(|(_, c)| c).collect()
}

// ---------- Shelves ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Recommended for you: everything the user likes, blended.
    ForYou,
    /// Popular this week.
    Trending,
    /// Trending today.
    TrendingDay,
    /// Because you watched {title}.
    Because,
    /// Because you liked {title}.
    Liked,
    /// Because you loved {title}.
    Loved,
    /// {genre} for you.
    Genre,
    /// More with {person}.
    Person,
    /// New releases for you.
    Fresh,
    /// Hidden gems for you.
    Gems,
    /// Critically acclaimed, in the user's genres.
    Acclaimed,
    /// Watch it again.
    Again,
}

/// One recommendation row. The UI names it from `kind` and `label`, then asks for its titles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shelf {
    pub key: String,
    pub kind: Kind,
    /// The seed title's media type (Because, Liked, Loved).
    pub media_type: Option<String>,
    /// The seed title's id, the genre (Genre) or the person (Person).
    pub id: Option<u64>,
    /// The seed's title or the person's name.
    pub label: Option<String>,
}

impl Shelf {
    fn plain(kind: Kind, key: &str) -> Self {
        Shelf { key: key.into(), kind, media_type: None, id: None, label: None }
    }
}

/// splitmix64: the shelves' shuffle, seeded once per run so a screen keeps its rows until the
/// launcher restarts.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }

    pub fn next_f32(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 40) as f32 / (1u64 << 24) as f32
    }
}

pub struct PlanInput<'a> {
    pub profile: &'a Profile,
    pub genres: &'a [(u32, f32)],
    pub people: &'a [(u64, String, f32)],
    /// The service whose home this is; None for the launcher's Home.
    pub service: Option<&'a str>,
}

/// Rows to show: Recommended for you and Popular this week always, then a draw from the
/// rest, so each run of the launcher shows a different mix.
pub fn plan(input: &PlanInput, rng: &mut Rng) -> Vec<Shelf> {
    let p = input.profile;
    let positives: Vec<&Seed> = p.positive().collect();
    let personal = !positives.is_empty();
    let has_genres = !input.genres.is_empty();

    // (weight, cap group, shelf, seed it's about)
    let mut options: Vec<(f32, &'static str, Shelf, Option<(String, u64)>)> = Vec::new();
    let seed_shelf = |kind: Kind, prefix: &str, s: &Seed| Shelf {
        key: format!("{prefix}:{}:{}", s.media_type, s.id),
        kind,
        media_type: Some(s.media_type.clone()),
        id: Some(s.id),
        label: Some(s.title.clone()),
    };

    // Watched lately; on a service's home, what was watched there first.
    let mut watched: Vec<&Seed> = positives.iter().copied().filter(|s| s.source == Source::Watched).collect();
    let here = |s: &Seed| input.service.is_some_and(|svc| s.services.iter().any(|x| x == svc));
    watched.sort_by(|a, b| here(b).cmp(&here(a)).then(b.at.cmp(&a.at)));
    for (i, s) in watched.iter().take(5).enumerate() {
        options.push((1.2 * 0.8f32.powi(i as i32), "because", seed_shelf(Kind::Because, "because", s), Some(s.key())));
    }

    let mut rated: Vec<&Seed> = positives.iter().copied().filter(|s| matches!(s.source, Source::Rated(_))).collect();
    rated.sort_by(|a, b| b.at.cmp(&a.at));
    for (i, s) in rated.iter().take(6).enumerate() {
        let (kind, base) = if s.source == Source::Rated(Score::Love) { (Kind::Loved, 1.3) } else { (Kind::Liked, 1.0) };
        let prefix = if kind == Kind::Loved { "loved" } else { "liked" };
        options.push((base * 0.85f32.powi(i as i32), "liked", seed_shelf(kind, prefix, s), Some(s.key())));
    }

    if let Some(&(_, top)) = input.genres.first() {
        for &(g, v) in input.genres.iter().take(4) {
            let shelf = Shelf { id: Some(g as u64), ..Shelf::plain(Kind::Genre, &format!("genre:{g}")) };
            options.push((0.9 * v / top, "genre", shelf, None));
        }
    }
    for (i, (id, name, _)) in input.people.iter().take(2).enumerate() {
        let shelf = Shelf {
            id: Some(*id),
            label: Some(name.clone()),
            ..Shelf::plain(Kind::Person, &format!("person:{id}"))
        };
        options.push((0.7 - 0.2 * i as f32, "person", shelf, None));
    }

    options.push((if has_genres { 0.8 } else { 0.5 }, "fresh", Shelf::plain(Kind::Fresh, "fresh"), None));
    options.push((if has_genres { 0.6 } else { 0.3 }, "gems", Shelf::plain(Kind::Gems, "gems"), None));
    options.push((0.5, "acclaimed", Shelf::plain(Kind::Acclaimed, "acclaimed"), None));
    options.push((0.35, "day", Shelf::plain(Kind::TrendingDay, "trending-day"), None));
    if p.seeds.iter().filter(|s| s.seen && s.weight > 0.0).count() >= 3 {
        options.push((0.5, "again", Shelf::plain(Kind::Again, "again"), None));
    }

    // Weighted draw without replacement: each option gets u^(1/w), the largest keys win.
    let mut keyed: Vec<(f32, usize)> = options
        .iter()
        .enumerate()
        .map(|(i, o)| (rng.next_f32().max(1e-6).powf(1.0 / o.0.max(0.01)), i))
        .collect();
    keyed.sort_by(|a, b| b.0.total_cmp(&a.0));

    let want = if personal { 7 } else { 4 };
    let cap = |group: &str| match group {
        "because" | "liked" | "genre" => 2,
        _ => 1,
    };
    let mut taken: HashMap<&str, usize> = HashMap::new();
    let mut seeds_used: HashSet<(String, u64)> = HashSet::new();
    let mut picks: Vec<Shelf> = Vec::new();
    for (_, i) in keyed {
        if picks.len() >= want {
            break;
        }
        let (_, group, shelf, seed) = &options[i];
        let n = taken.entry(*group).or_default();
        if *n >= cap(group) || seed.as_ref().is_some_and(|k| seeds_used.contains(k)) {
            continue;
        }
        *n += 1;
        if let Some(k) = seed {
            seeds_used.insert(k.clone());
        }
        picks.push(shelf.clone());
    }

    let trending = Shelf::plain(Kind::Trending, "trending");
    let mut out = Vec::new();
    if personal {
        out.push(Shelf::plain(Kind::ForYou, "for-you"));
        let mut rest = picks.into_iter();
        out.extend(rest.next());
        out.push(trending);
        out.extend(rest);
    } else {
        out.push(trending);
        out.extend(picks);
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::tmdb::Title;

    const NOW: i64 = 1_800_000_000_000;

    fn progress(id: u64, media: &str, finished: bool, days_ago: i64) -> Progress {
        Progress {
            service: "netflix".into(),
            media_type: media.into(),
            id,
            title: format!("T{id}"),
            poster: None,
            backdrop: None,
            seasons: vec![10, 10],
            season: if media == "tv" { 2 } else { 0 },
            episode: if media == "tv" { 3 } else { 0 },
            position: 600.0,
            duration: 3000.0,
            episodes: BTreeMap::new(),
            finished,
            updated_at: NOW - days_ago * DAY_MS,
            confirmed: true,
            share: None,
            dismissed_at: None,
        }
    }

    fn rating(id: u64, score: Score) -> Rating {
        Rating {
            media_type: "movie".into(),
            id,
            title: format!("R{id}"),
            poster: None,
            backdrop: None,
            score,
            at: NOW,
        }
    }

    fn cand(id: u64, genres: &[u32]) -> Candidate {
        Candidate {
            title: Title {
                id,
                media_type: "movie".into(),
                title: format!("C{id}"),
                year: None,
                overview: String::new(),
                poster: None,
                backdrop: None,
            },
            genres: genres.to_vec(),
            rating: 7.0,
            votes: 1000,
            popularity: 10.0,
        }
    }

    #[test]
    fn ratings_outweigh_watching_and_dislikes_go_negative() {
        let p = build(
            &[progress(1, "movie", true, 0), progress(2, "movie", true, 0)],
            &[],
            &[rating(2, Score::Dislike), rating(3, Score::Love)],
            NOW,
        );
        assert_eq!(p.seeds[0].id, 3);
        assert_eq!(p.seeds[0].weight, 3.5);
        assert_eq!(p.seeds.last().unwrap().id, 2);
        assert!(p.seeds.last().unwrap().weight < 0.0);
        assert!(p.exclude.contains(&("movie".into(), 1)));
        assert!(p.exclude.contains(&("movie".into(), 3)));
    }

    #[test]
    fn a_series_far_along_counts_more_than_one_just_started() {
        let mut started = progress(2, "tv", false, 0);
        started.season = 1;
        started.episode = 1;
        let p = build(&[progress(1, "tv", false, 0), started], &[], &[], NOW);
        let w = |id| p.seeds.iter().find(|s| s.id == id).unwrap().weight;
        // Season 2, episode 3: 12 episodes behind it.
        assert!(w(1) > 1.9);
        assert_eq!(w(2), 0.4);
    }

    #[test]
    fn dropped_long_ago_counts_against_and_old_watching_fades() {
        let mut dropped = progress(1, "movie", false, 60);
        dropped.position = 120.0;
        let p = build(&[dropped, progress(2, "movie", true, 90)], &[], &[], NOW);
        let w = |id| p.seeds.iter().find(|s| s.id == id).unwrap().weight;
        assert!(w(1) < 0.0);
        assert!((w(2) - 0.75).abs() < 0.01);
    }

    #[test]
    fn saved_titles_are_seeds_and_not_recommended() {
        let item = ListItem {
            service: "prime".into(),
            media_type: "movie".into(),
            id: 9,
            title: "L".into(),
            poster: None,
            backdrop: None,
            added_at: NOW,
        };
        let p = build(&[], &[item], &[], NOW);
        assert_eq!(p.seeds[0].source, Source::Listed);
        assert_eq!(p.seeds[0].services, vec!["prime"]);
        assert!(p.exclude.contains(&("movie".into(), 9)));
    }

    #[test]
    fn genres_map_between_movies_and_series() {
        assert_eq!(canonical(10759), 28);
        assert_eq!(for_media(28, "tv"), Some(10759));
        assert_eq!(for_media(27, "tv"), None);
        assert_eq!(for_media(10765, "movie"), Some(10765));
        assert_eq!(for_media(878, "movie"), Some(878));
    }

    #[test]
    fn affinity_follows_weights_and_fit_scores_titles() {
        let p = build(&[], &[], &[rating(1, Score::Love), rating(2, Score::Dislike)], NOW);
        let mut meta = HashMap::new();
        meta.insert(("movie".to_string(), 1), Meta { genres: vec![878, 28], cast: vec![] });
        meta.insert(("movie".to_string(), 2), Meta { genres: vec![10749], cast: vec![] });
        let aff = genre_affinity(&p.seeds, &meta);
        assert_eq!(aff.len(), 2);
        assert!(aff.iter().all(|(g, _)| *g == 28 || *g == 878));
        assert!(genre_fit(&[10765], &aff) > 0.6);
        assert_eq!(genre_fit(&[10749], &aff), 0.0);
    }

    #[test]
    fn people_need_two_titles_or_one_loved() {
        let p = build(
            &[progress(1, "movie", true, 0), progress(2, "movie", true, 0)],
            &[],
            &[rating(3, Score::Love)],
            NOW,
        );
        let mut meta = HashMap::new();
        meta.insert(("movie".to_string(), 1), Meta { genres: vec![], cast: vec![(10, "A".into()), (11, "B".into())] });
        meta.insert(("movie".to_string(), 2), Meta { genres: vec![], cast: vec![(10, "A".into())] });
        meta.insert(("movie".to_string(), 3), Meta { genres: vec![], cast: vec![(12, "C".into())] });
        let people = favorite_people(&p.seeds, &meta);
        let ids: Vec<u64> = people.iter().map(|x| x.0).collect();
        assert!(ids.contains(&10) && ids.contains(&12));
        assert!(!ids.contains(&11));
    }

    #[test]
    fn blend_adds_up_agreement_and_negatives_push_down() {
        let out = blend(vec![
            (1.0, vec![cand(1, &[]), cand(2, &[])]),
            (1.0, vec![cand(2, &[]), cand(3, &[])]),
            (-2.0, vec![cand(3, &[])]),
        ]);
        assert_eq!(out[0].0.title.id, 2);
        assert_eq!(out.last().unwrap().0.title.id, 3);
        assert!(out.last().unwrap().1 < 0.0);
    }

    #[test]
    fn interleave_alternates_and_dedupes() {
        let out = interleave(vec![vec![cand(1, &[]), cand(2, &[])], vec![cand(3, &[]), cand(1, &[])]]);
        let ids: Vec<u64> = out.iter().map(|c| c.title.id).collect();
        assert_eq!(ids, vec![1, 3, 2]);
    }

    fn planned(p: &Profile, seed: u64) -> Vec<Shelf> {
        let genres = [(28, 2.0), (878, 1.0)];
        let people = [(10, "A".to_string(), 3.0)];
        let input = PlanInput { profile: p, genres: &genres, people: &people, service: None };
        plan(&input, &mut Rng::new(seed))
    }

    #[test]
    fn plan_pins_for_you_and_trending_and_respects_caps() {
        let progress: Vec<Progress> = (1..=6).map(|i| progress(i, "movie", true, i as i64)).collect();
        let ratings: Vec<Rating> = (10..=14).map(|i| rating(i, Score::Love)).collect();
        let p = build(&progress, &[], &ratings, NOW);
        for seed in 0..50 {
            let out = planned(&p, seed);
            assert_eq!(out[0].kind, Kind::ForYou);
            assert_eq!(out[2].kind, Kind::Trending);
            assert_eq!(out.len(), 9);
            let count = |k: &[Kind]| out.iter().filter(|s| k.contains(&s.kind)).count();
            assert!(count(&[Kind::Because]) <= 2);
            assert!(count(&[Kind::Liked, Kind::Loved]) <= 2);
            assert!(count(&[Kind::Genre]) <= 2);
            let keys: HashSet<&str> = out.iter().map(|s| s.key.as_str()).collect();
            assert_eq!(keys.len(), out.len());
        }
    }

    #[test]
    fn plan_is_stable_for_a_run_and_varies_between_runs() {
        let progress: Vec<Progress> = (1..=6).map(|i| progress(i, "movie", true, i as i64)).collect();
        let p = build(&progress, &[], &[rating(10, Score::Love)], NOW);
        assert_eq!(planned(&p, 7), planned(&p, 7));
        let distinct: HashSet<Vec<String>> =
            (0..20).map(|s| planned(&p, s).into_iter().map(|x| x.key).collect()).collect();
        assert!(distinct.len() > 3);
    }

    #[test]
    fn cold_start_has_no_personal_rows() {
        let p = Profile::default();
        let input = PlanInput { profile: &p, genres: &[], people: &[], service: Some("netflix") };
        let out = plan(&input, &mut Rng::new(1));
        assert_eq!(out[0].kind, Kind::Trending);
        assert_eq!(out.len(), 5);
        assert!(out.iter().all(|s| !matches!(s.kind, Kind::ForYou | Kind::Because | Kind::Again)));
    }

    #[test]
    fn rng_stays_in_range() {
        let mut r = Rng::new(42);
        for _ in 0..1000 {
            let x = r.next_f32();
            assert!((0.0..1.0).contains(&x));
        }
    }
}
