use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;

const MAX_PROGRESS: usize = 200;
const MAX_LIST: usize = 300;
/// The service's Continue watching row carries no dates: a title played from the launcher this
/// recently is left alone, as the service may not show the new position yet.
const RECENT_MS: i64 = 10 * 60 * 1000;

fn yes() -> bool {
    true
}

/// How far into a movie or series the user is, as the launcher saw it play.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Progress {
    pub service: String,
    /// "movie" or "tv"
    pub media_type: String,
    pub id: u64,
    pub title: String,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    /// Episode counts of the regular seasons, in order. Empty for a movie.
    #[serde(default)]
    pub seasons: Vec<u32>,
    /// The episode last watched, or the next one once it was finished; 0 and 0 for a movie.
    /// Every episode before it counts as watched.
    pub season: u32,
    pub episode: u32,
    /// Seconds into it, and its length (0 when unknown).
    pub position: f64,
    pub duration: f64,
    /// Watched share of episodes seen at some point, "s1e3" → 0..1. Earlier ones aren't listed.
    #[serde(default)]
    pub episodes: BTreeMap<String, f32>,
    /// The movie or the whole series is done: it leaves Continue watching.
    pub finished: bool,
    /// Unix time in milliseconds.
    pub updated_at: i64,
    /// The episode is known for sure: seen in the player, picked by the user or listed in the
    /// service's history. False when only the service's Continue watching row was read (title and
    /// bar, not which episode): the UI then says "Resume" and the service picks the episode.
    #[serde(default = "yes")]
    pub confirmed: bool,
    /// Watched share of the current episode or movie when only a percentage is known.
    #[serde(default)]
    pub share: Option<f32>,
    /// When it was taken out of Continue watching by hand (Unix ms). Playing it brings it back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dismissed_at: Option<i64>,
}

impl Progress {
    /// Watched share of the current episode or movie, 0..1.
    pub fn current_share(&self) -> f32 {
        if self.duration > 0.0 {
            (self.position / self.duration).clamp(0.0, 1.0) as f32
        } else {
            self.share.unwrap_or(0.0)
        }
    }
}

/// A movie or series saved with My list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ListItem {
    pub service: String,
    pub media_type: String,
    pub id: u64,
    pub title: String,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    #[serde(default)]
    pub added_at: i64,
}

/// What a playing session reports.
#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    pub service: String,
    pub media_type: String,
    pub id: u64,
    pub title: String,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    pub seasons: Vec<u32>,
    pub season: u32,
    pub episode: u32,
    pub position: f64,
    pub duration: f64,
    /// Watched share, when only a percentage is known (`duration` 0).
    pub share: Option<f32>,
    /// The episode is known for sure (see `Progress::confirmed`).
    pub confirmed: bool,
}

/// What the service itself says about a title (sync.rs read it from the service's website).
#[derive(Debug, Clone, PartialEq)]
pub struct Synced {
    /// Where the service says the user is. `season` 0: the service didn't say which episode.
    pub update: Update,
    /// When the service says it was watched; None from its Continue watching row, which has no dates.
    pub watched_at: Option<i64>,
    /// Place in the service's list, most recent first: orders titles without dates.
    pub rank: usize,
    /// Other episodes the service lists, with their watched share.
    pub episodes: Vec<(u32, u32, f32)>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub progress: Vec<Progress>,
    #[serde(default)]
    pub watchlist: Vec<ListItem>,
    /// The progress of a service's other profiles, by "service:profile id", while another one is
    /// the one played on (`switch_profile`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    shelved: BTreeMap<String, Vec<Progress>>,
}

pub fn episode_key(season: u32, episode: u32) -> String {
    format!("s{season}e{episode}")
}

/// Watched: the last 8% (the credits), or the last two minutes of anything 15 minutes or longer.
pub fn is_done(position: f64, duration: f64) -> bool {
    duration > 0.0 && (position / duration >= 0.92 || (duration >= 900.0 && duration - position <= 120.0))
}

/// The episode after `season`/`episode`, crossing into the next season.
pub fn next_episode(seasons: &[u32], season: u32, episode: u32) -> Option<(u32, u32)> {
    let count = *seasons.get(season.checked_sub(1)? as usize)?;
    if episode < count {
        return Some((season, episode + 1));
    }
    seasons
        .iter()
        .enumerate()
        .skip(season as usize)
        .find(|(_, &n)| n > 0)
        .map(|(i, _)| (i as u32 + 1, 1))
}

impl Store {
    pub fn load(dir: &Path) -> Self {
        fs::read_to_string(dir.join("watch.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        fs::write(dir.join("watch.json"), serde_json::to_string(self)?)?;
        Ok(())
    }

    pub fn get(&self, service: &str, media_type: &str, id: u64) -> Option<&Progress> {
        self.progress
            .iter()
            .find(|p| p.service == service && p.media_type == media_type && p.id == id)
    }

    /// Takes in where playback is. A finished episode moves the series on to the next one.
    pub fn record(&mut self, u: Update, now: i64) {
        let at = self
            .progress
            .iter()
            .position(|p| p.service == u.service && p.media_type == u.media_type && p.id == u.id);
        let mut p = match at {
            Some(i) => self.progress.remove(i),
            None => Progress {
                service: u.service.clone(),
                media_type: u.media_type.clone(),
                id: u.id,
                title: String::new(),
                poster: None,
                backdrop: None,
                seasons: Vec::new(),
                season: 0,
                episode: 0,
                position: 0.0,
                duration: 0.0,
                episodes: BTreeMap::new(),
                finished: false,
                updated_at: now,
                confirmed: true,
                share: None,
                dismissed_at: None,
            },
        };
        p.title = u.title;
        p.poster = u.poster.or(p.poster);
        p.backdrop = u.backdrop.or(p.backdrop);
        if !u.seasons.is_empty() {
            p.seasons = u.seasons;
        }
        p.season = u.season;
        p.episode = u.episode;
        p.position = u.position.max(0.0);
        p.duration = u.duration.max(0.0);
        p.updated_at = now;
        p.finished = false;
        p.confirmed = u.confirmed;
        p.share = if p.duration > 0.0 { None } else { u.share };
        p.dismissed_at = None;

        let series = p.media_type == "tv";
        // A series' episode that isn't known can't be marked or moved past.
        let known = !series || (p.confirmed && p.season > 0);
        let share = p.current_share();
        let done = is_done(p.position, p.duration) || (p.duration <= 0.0 && share >= 0.92);
        if done && known {
            if series {
                p.episodes.insert(episode_key(p.season, p.episode), 1.0);
                match next_episode(&p.seasons, p.season, p.episode) {
                    Some((s, e)) => {
                        p.season = s;
                        p.episode = e;
                        p.position = 0.0;
                        p.duration = 0.0;
                        p.share = None;
                    }
                    None => p.finished = true,
                }
            } else {
                p.finished = true;
            }
        } else if series && known {
            let key = episode_key(p.season, p.episode);
            let old = p.episodes.get(&key).copied().unwrap_or(0.0);
            p.episodes.insert(key, old.max(share));
        }

        self.progress.insert(0, p);
        self.progress.truncate(MAX_PROGRESS);
    }

    /// What to show in Continue watching, most recent first: started and not finished.
    pub fn continue_watching(&self, service: Option<&str>) -> Vec<Progress> {
        let mut list: Vec<Progress> = self
            .progress
            .iter()
            .filter(|p| service.map_or(true, |s| p.service == s))
            .filter(|p| {
                !p.finished
                    && p.dismissed_at.is_none()
                    && (p.position >= 30.0
                        || p.share.is_some_and(|s| s > 0.01)
                        || !p.confirmed
                        || (p.media_type == "tv" && (p.season, p.episode) > (1, 1)))
            })
            .cloned()
            .collect();
        list.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        list
    }

    /// Takes in what the service says, unless the launcher knows better. True when anything changed.
    pub fn apply_sync(&mut self, s: Synced, now: i64) -> bool {
        let u = &s.update;
        let existing = self.get(&u.service, &u.media_type, u.id).cloned();
        let merged = self.merge_episodes(&u.service, &u.media_type, u.id, &s.episodes);
        let unknown_episode = u.media_type == "tv" && u.season == 0;

        let at = match (s.watched_at, &existing) {
            // The service's history has dates: the newer of the two wins. Taken out of Continue
            // watching by hand, it comes back once the service has it watched since.
            (Some(at), Some(p)) if p.updated_at >= at || p.dismissed_at.is_some_and(|d| d >= at) => return merged,
            (Some(at), _) => at,
            // Taken out by hand: the service's Continue watching row, undated, doesn't bring it back.
            (None, Some(p)) if p.dismissed_at.is_some() => return merged,
            // Its Continue watching row has none: keep what was just played here, and what agrees.
            (None, Some(p)) => {
                if now - p.updated_at < RECENT_MS {
                    return merged;
                }
                let same_episode = unknown_episode || (p.season, p.episode) == (u.season, u.episode);
                let share = if u.duration > 0.0 { (u.position / u.duration) as f32 } else { u.share.unwrap_or(0.0) };
                if same_episode && (p.current_share() - share).abs() < 0.05 {
                    return merged;
                }
                now - s.rank as i64 * 1000
            }
            (None, None) => now - s.rank as i64 * 1000,
        };

        let mut update = s.update.clone();
        if unknown_episode {
            // Keep the launcher's last known episode for the details screen, marked as unsure.
            if let Some(p) = &existing {
                update.season = p.season;
                update.episode = p.episode;
                if update.seasons.is_empty() {
                    update.seasons = p.seasons.clone();
                }
            }
            update.confirmed = false;
        }
        self.record(update, at);
        self.merge_episodes(&s.update.service, &s.update.media_type, s.update.id, &s.episodes);
        true
    }

    /// Raises episodes' watched shares to what the service lists. True when any went up.
    fn merge_episodes(&mut self, service: &str, media_type: &str, id: u64, episodes: &[(u32, u32, f32)]) -> bool {
        let Some(p) = self
            .progress
            .iter_mut()
            .find(|p| p.service == service && p.media_type == media_type && p.id == id)
        else {
            return false;
        };
        let mut changed = false;
        for &(season, episode, share) in episodes {
            let key = episode_key(season, episode);
            let old = p.episodes.get(&key).copied().unwrap_or(0.0);
            let new = if is_done(share as f64, 1.0) { 1.0 } else { share };
            if new > old + 0.01 {
                p.episodes.insert(key, new);
                changed = true;
            }
        }
        changed
    }

    /// `service` now plays on profile `to` instead of `from`: what the store has on it is put aside
    /// as `from`'s, and what was put aside for `to` comes back. Continue watching never mixes them.
    pub fn switch_profile(&mut self, service: &str, from: &str, to: &str) {
        let (mine, others): (Vec<Progress>, Vec<Progress>) =
            std::mem::take(&mut self.progress).into_iter().partition(|p| p.service == service);
        self.progress = others;
        if !mine.is_empty() {
            self.shelved.insert(format!("{service}:{from}"), mine);
        }
        if let Some(back) = self.shelved.remove(&format!("{service}:{to}")) {
            self.progress.extend(back);
            self.progress.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        }
    }

    /// Takes the title out of Continue watching, until it is played again (`record`).
    pub fn dismiss(&mut self, service: &str, media_type: &str, id: u64, now: i64) {
        if let Some(p) = self
            .progress
            .iter_mut()
            .find(|p| p.service == service && p.media_type == media_type && p.id == id)
        {
            p.dismissed_at = Some(now);
        }
    }

    pub fn listed(&self, service: &str, media_type: &str, id: u64) -> bool {
        self.watchlist
            .iter()
            .any(|l| l.service == service && l.media_type == media_type && l.id == id)
    }

    /// Adds the title to My list, or takes it out. True when it is now listed.
    pub fn toggle_listed(&mut self, item: ListItem) -> bool {
        let before = self.watchlist.len();
        self.watchlist
            .retain(|l| !(l.service == item.service && l.media_type == item.media_type && l.id == item.id));
        if self.watchlist.len() < before {
            return false;
        }
        self.watchlist.insert(0, item);
        self.watchlist.truncate(MAX_LIST);
        true
    }

    /// My list on `service`, or on every service; newest first.
    pub fn list(&self, service: Option<&str>) -> Vec<ListItem> {
        self.watchlist
            .iter()
            .filter(|l| service.is_none_or(|s| l.service == s))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(season: u32, episode: u32, position: f64, duration: f64) -> Update {
        Update {
            service: "netflix".into(),
            media_type: "tv".into(),
            id: 7,
            title: "Dark".into(),
            poster: None,
            backdrop: Some("b.jpg".into()),
            seasons: vec![10, 8, 8],
            season,
            episode,
            position,
            duration,
            share: None,
            confirmed: true,
        }
    }

    fn synced(update: Update, watched_at: Option<i64>) -> Synced {
        Synced { update, watched_at, rank: 0, episodes: vec![] }
    }

    #[test]
    fn history_with_dates_wins_only_when_newer() {
        let mut st = Store::default();
        st.record(update(1, 2, 600.0, 3000.0), 1_000);
        // Watched on another TV later: the service's history moves it on.
        assert!(st.apply_sync(synced(update(2, 5, 300.0, 3000.0), Some(2_000)), 50_000_000));
        let p = st.get("netflix", "tv", 7).unwrap();
        assert_eq!((p.season, p.episode, p.position, p.updated_at), (2, 5, 300.0, 2_000));
        // An older history entry doesn't undo it.
        assert!(!st.apply_sync(synced(update(1, 1, 10.0, 3000.0), Some(1_500)), 50_000_000));
        assert_eq!(st.get("netflix", "tv", 7).unwrap().episode, 5);
    }

    #[test]
    fn history_finished_episode_moves_on_and_marks_others() {
        let mut st = Store::default();
        let mut s = synced(update(1, 4, 2990.0, 3000.0), Some(5));
        s.episodes = vec![(1, 4, 1.0), (1, 2, 0.5)];
        st.apply_sync(s, 10);
        let p = st.get("netflix", "tv", 7).unwrap();
        assert_eq!((p.season, p.episode), (1, 5));
        assert_eq!(p.episodes["s1e2"], 0.5);
        assert!(p.confirmed);
    }

    #[test]
    fn row_without_episode_marks_it_unsure() {
        let now = 100_000_000;
        let mut st = Store::default();
        st.record(update(1, 2, 600.0, 3000.0), now - 3_600_000);
        let mut row = update(0, 0, 0.0, 0.0);
        row.share = Some(0.6);
        assert!(st.apply_sync(synced(row.clone(), None), now));
        let p = st.get("netflix", "tv", 7).unwrap();
        assert!(!p.confirmed);
        assert_eq!((p.season, p.episode), (1, 2));
        assert_eq!(p.current_share(), 0.6);
        // Unsure progress still shows in Continue watching; the same bar again changes nothing.
        assert_eq!(st.continue_watching(None).len(), 1);
        assert!(!st.apply_sync(synced(row, None), now + RECENT_MS + 1));
    }

    #[test]
    fn dismissed_stays_out_until_watched_again() {
        let now = 100_000_000;
        let mut st = Store::default();
        st.record(update(1, 2, 600.0, 3000.0), now - 3_600_000);
        st.dismiss("netflix", "tv", 7, now);
        assert!(st.continue_watching(None).is_empty());
        // The service's undated row, or history from before, doesn't bring it back.
        let mut row = update(1, 3, 0.0, 0.0);
        row.share = Some(0.4);
        assert!(!st.apply_sync(synced(row, None), now + RECENT_MS + 1));
        assert!(!st.apply_sync(synced(update(1, 3, 900.0, 3000.0), Some(now - 1)), now + 1));
        assert!(st.continue_watching(None).is_empty());
        // Watched since, on another TV: back.
        assert!(st.apply_sync(synced(update(1, 3, 900.0, 3000.0), Some(now + 5)), now + 10));
        assert_eq!(st.continue_watching(None).len(), 1);
        // Played here: back too.
        st.dismiss("netflix", "tv", 7, now + 20);
        st.record(update(1, 3, 1000.0, 3000.0), now + 30);
        assert_eq!(st.continue_watching(None).len(), 1);
    }

    #[test]
    fn row_leaves_a_title_just_played_here_alone() {
        let now = 100_000_000;
        let mut st = Store::default();
        st.record(update(1, 2, 600.0, 3000.0), now - 60_000);
        let mut row = update(3, 1, 0.0, 0.0);
        row.share = Some(0.1);
        assert!(!st.apply_sync(synced(row, None), now));
        assert_eq!(st.get("netflix", "tv", 7).unwrap().episode, 2);
    }

    #[test]
    fn row_with_episode_label_is_confirmed() {
        let mut st = Store::default();
        let mut row = update(2, 3, 0.0, 0.0);
        row.share = Some(0.4);
        st.apply_sync(synced(row, None), 1_000_000);
        let p = st.get("netflix", "tv", 7).unwrap();
        assert!(p.confirmed);
        assert_eq!((p.season, p.episode), (2, 3));
        assert_eq!(p.episodes["s2e3"], 0.4);
    }

    #[test]
    fn next_episode_crosses_seasons_and_stops_at_the_end() {
        let s = [10, 8, 8];
        assert_eq!(next_episode(&s, 1, 3), Some((1, 4)));
        assert_eq!(next_episode(&s, 1, 10), Some((2, 1)));
        assert_eq!(next_episode(&s, 3, 8), None);
        assert_eq!(next_episode(&s, 0, 1), None);
        assert_eq!(next_episode(&[3, 0, 5], 1, 3), Some((3, 1)));
    }

    #[test]
    fn done_near_the_end_or_in_the_credits() {
        assert!(!is_done(10.0, 0.0));
        assert!(!is_done(2000.0, 3000.0));
        assert!(is_done(2770.0, 3000.0)); // 92%
        assert!(is_done(2890.0, 3000.0)); // two minutes left
        // Short episodes go by the share alone.
        assert!(!is_done(1200.0, 1400.0));
    }

    #[test]
    fn partial_episode_is_kept() {
        let mut st = Store::default();
        st.record(update(1, 3, 600.0, 3000.0), 1);
        let p = st.get("netflix", "tv", 7).unwrap();
        assert_eq!((p.season, p.episode), (1, 3));
        assert_eq!(p.episodes["s1e3"], 0.2);
        assert!(!p.finished);
        assert_eq!(st.continue_watching(Some("netflix")).len(), 1);
        assert!(st.continue_watching(Some("prime")).is_empty());
    }

    #[test]
    fn finished_episode_moves_to_the_next_one() {
        let mut st = Store::default();
        st.record(update(1, 10, 2950.0, 3000.0), 1);
        let p = st.get("netflix", "tv", 7).unwrap();
        assert_eq!((p.season, p.episode), (2, 1));
        assert_eq!(p.position, 0.0);
        assert_eq!(p.episodes["s1e10"], 1.0);
        // Up next counts as started: it stays in Continue watching.
        assert_eq!(st.continue_watching(None).len(), 1);
    }

    #[test]
    fn last_episode_finishes_the_series() {
        let mut st = Store::default();
        st.record(update(3, 8, 2990.0, 3000.0), 1);
        assert!(st.get("netflix", "tv", 7).unwrap().finished);
        assert!(st.continue_watching(None).is_empty());
    }

    #[test]
    fn movie_finishes_and_keeps_art() {
        let mut st = Store::default();
        let mut u = update(0, 0, 300.0, 6000.0);
        u.media_type = "movie".into();
        u.seasons.clear();
        st.record(u.clone(), 1);
        assert_eq!(st.continue_watching(None).len(), 1);
        u.backdrop = None;
        u.position = 5900.0;
        st.record(u, 2);
        let p = st.get("netflix", "movie", 7).unwrap();
        assert!(p.finished);
        assert_eq!(p.backdrop.as_deref(), Some("b.jpg"));
    }

    #[test]
    fn most_recent_first_and_rewatch_keeps_the_best_share() {
        let mut st = Store::default();
        st.record(update(1, 2, 1500.0, 3000.0), 1);
        let mut other = update(1, 1, 100.0, 3000.0);
        other.id = 8;
        st.record(other, 2);
        assert_eq!(st.progress[0].id, 8);
        st.record(update(1, 2, 300.0, 3000.0), 3);
        assert_eq!(st.progress[0].id, 7);
        assert_eq!(st.progress[0].episodes["s1e2"], 0.5);
    }

    #[test]
    fn barely_started_first_episode_is_not_continue_watching() {
        let mut st = Store::default();
        st.record(update(1, 1, 5.0, 3000.0), 1);
        assert!(st.continue_watching(None).is_empty());
    }

    #[test]
    fn profiles_keep_their_own_progress() {
        let mut st = Store::default();
        st.record(update(1, 3, 600.0, 3000.0), 1);
        let mut prime = update(1, 2, 600.0, 3000.0);
        prime.service = "prime".into();
        st.record(prime, 2);

        st.switch_profile("netflix", "A", "B");
        assert!(st.continue_watching(Some("netflix")).is_empty());
        assert_eq!(st.continue_watching(Some("prime")).len(), 1);
        let mut other = update(1, 5, 600.0, 3000.0);
        other.id = 8;
        st.record(other, 3);

        st.switch_profile("netflix", "B", "A");
        let back = st.continue_watching(Some("netflix"));
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].id, 7);
        st.switch_profile("netflix", "A", "B");
        assert_eq!(st.continue_watching(Some("netflix"))[0].id, 8);
    }

    #[test]
    fn my_list_toggles() {
        let mut st = Store::default();
        let item = ListItem {
            service: "hbomax".into(),
            media_type: "movie".into(),
            id: 1,
            title: "Dune".into(),
            poster: None,
            backdrop: None,
            added_at: 1,
        };
        assert!(st.toggle_listed(item.clone()));
        assert!(st.listed("hbomax", "movie", 1));
        assert_eq!(st.list(Some("hbomax")).len(), 1);
        assert!(st.list(Some("netflix")).is_empty());
        assert_eq!(st.list(None).len(), 1);
        assert!(!st.toggle_listed(item));
        assert!(!st.listed("hbomax", "movie", 1));
    }
}
