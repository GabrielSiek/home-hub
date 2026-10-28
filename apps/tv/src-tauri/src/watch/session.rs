use std::collections::HashMap;

use serde::Deserialize;

use super::store::{next_episode, Update};

/// What watch.js sees in the page, every couple of seconds and on play, pause and ended.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Sample {
    #[serde(default)]
    pub url: String,
    /// A video fills the screen.
    #[serde(default)]
    pub video: bool,
    /// The page is the service's player (by its address), video or not.
    #[serde(default)]
    pub player: bool,
    /// Seconds into the video, and its length; missing or NaN when the page doesn't tell.
    pub t: Option<f64>,
    pub d: Option<f64>,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub ended: bool,
    /// Episode text shown by the player ("T1:E3", "Season 1 Episode 3"), when found.
    pub label: Option<String>,
    /// The page went from the player to the service's own pages (its Back arrow) after playing.
    #[serde(default)]
    pub left: bool,
}

impl Sample {
    /// Position and length the page gives for real (a stream without a length gives none).
    fn timing(&self) -> Option<(f64, f64)> {
        let (t, d) = (self.t?, self.d?);
        (self.video && t.is_finite() && d.is_finite() && d > 60.0).then_some((t.max(0.0), d))
    }
}

/// What the caller does after a sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    /// The video started: time to show the browser.
    Started,
    /// Playback is over (the player went away, or the last video ended): back to the launcher.
    Close,
}

/// Seconds without the player after watching, and after the end with nothing new, before closing.
const AWAY_CLOSE: f64 = 6.0;
const ENDED_CLOSE: f64 = 25.0;

/// One play of a movie or series, from pressing Play to coming back.
#[derive(Debug, Clone)]
pub struct Session {
    pub service: String,
    pub media_type: String,
    pub id: u64,
    pub title: String,
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    pub seasons: Vec<u32>,
    pub season: u32,
    pub episode: u32,
    /// Seconds, by (season, episode); `runtime` for the ones not known.
    pub runtimes: HashMap<(u32, u32), f64>,
    pub runtime: f64,
    position: f64,
    duration: f64,
    last: Option<Sample>,
    last_at: Option<f64>,
    /// Counted play time, for a video that doesn't tell its position.
    clock: f64,
    started: bool,
    ended_at: Option<f64>,
    away_since: Option<f64>,
    /// When `update` was last saved, in the caller's seconds.
    pub saved_at: f64,
    /// The episode is known for sure: picked by the user, read from the player, or the store's
    /// own confirmed one. False when the service resumes where it alone knows (see store.rs).
    pub confirmed: bool,
    /// When Play was pressed, in the caller's seconds (for the startup timing log).
    pub opened_at: f64,
}

impl Session {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        service: String,
        media_type: String,
        id: u64,
        title: String,
        poster: Option<String>,
        backdrop: Option<String>,
        seasons: Vec<u32>,
        season: u32,
        episode: u32,
    ) -> Self {
        Self {
            service,
            media_type,
            id,
            title,
            poster,
            backdrop,
            seasons,
            season,
            episode,
            runtimes: HashMap::new(),
            runtime: 0.0,
            position: 0.0,
            duration: 0.0,
            last: None,
            last_at: None,
            clock: 0.0,
            started: false,
            ended_at: None,
            away_since: None,
            saved_at: 0.0,
            confirmed: true,
            opened_at: 0.0,
        }
    }

    pub fn started(&self) -> bool {
        self.started
    }

    fn series(&self) -> bool {
        self.media_type == "tv"
    }

    fn episode_runtime(&self) -> f64 {
        self.runtimes
            .get(&(self.season, self.episode))
            .copied()
            .unwrap_or(self.runtime)
    }

    fn valid(&self, season: u32, episode: u32) -> bool {
        self.seasons
            .get((season as usize).wrapping_sub(1))
            .is_some_and(|&n| episode >= 1 && episode <= n)
    }

    fn go_to(&mut self, season: u32, episode: u32) {
        self.season = season;
        self.episode = episode;
        self.position = 0.0;
        self.duration = 0.0;
        self.clock = 0.0;
    }

    /// The video moved on by itself (autoplay of the next episode).
    fn advance(&mut self) {
        if let Some((s, e)) = next_episode(&self.seasons, self.season, self.episode) {
            self.go_to(s, e);
        }
    }

    /// Takes in a sample taken at `now` (seconds, any clock).
    pub fn on_sample(&mut self, s: Sample, now: f64) -> Outcome {
        let mut out = Outcome::Nothing;
        let timing = s.timing();
        let playing = s.video && !s.paused && !s.ended;

        if !self.started && playing && timing.is_none_or_moving() {
            self.started = true;
            out = Outcome::Started;
        }

        // Which episode is on: what the player says, else a new video after the last one.
        if self.series() {
            let labelled = s.label.as_deref().and_then(parse_label).filter(|&(a, b)| self.valid(a, b));
            if let Some((ls, le)) = labelled {
                if (ls, le) != (self.season, self.episode) {
                    self.go_to(ls, le);
                }
                self.confirmed = true;
            } else if let (Some(prev), Some((t, d))) = (&self.last, timing) {
                if let Some((pt, pd)) = prev.timing() {
                    let other_length = (pd - d).abs() > 60.0;
                    let restarted = pt > pd * 0.85 && t < 90.0 && pt - t > 300.0;
                    let other_page = prev.player && s.player && strip(&prev.url) != strip(&s.url);
                    if other_length || restarted || other_page {
                        self.advance();
                    }
                }
            }
        }

        match timing {
            Some((t, d)) => {
                self.position = t;
                self.duration = d;
            }
            None if s.video => {
                // The page hides the position: count play time against the episode's length.
                if let (Some(at), Some(prev)) = (self.last_at, &self.last) {
                    if prev.video && !prev.paused && !prev.ended {
                        self.clock += (now - at).clamp(0.0, 30.0);
                    }
                }
                let length = self.episode_runtime();
                if self.series() && length > 0.0 && self.clock >= length {
                    let carry = self.clock - length;
                    self.advance();
                    self.clock = carry;
                }
                self.position = self.clock;
                self.duration = self.episode_runtime();
            }
            None => {}
        }

        if self.started {
            if s.ended {
                self.ended_at.get_or_insert(now);
            } else if playing {
                self.ended_at = None;
            }
            if !s.video && !s.player {
                self.away_since.get_or_insert(now);
            } else {
                self.away_since = None;
            }
            let ended_long = self.ended_at.is_some_and(|at| now - at >= ENDED_CLOSE);
            let away_long = self.away_since.is_some_and(|at| now - at >= AWAY_CLOSE);
            if ended_long || away_long {
                out = Outcome::Close;
            }
        }
        // No waiting on this one: watch.js only says so once the video played and the address
        // left the player, which moving on to another episode never does.
        if s.left {
            out = Outcome::Close;
        }

        self.last = Some(s);
        self.last_at = Some(now);
        out
    }

    /// Where playback is, for the store. None before anything played.
    pub fn update(&self) -> Option<Update> {
        if !self.started {
            return None;
        }
        Some(Update {
            service: self.service.clone(),
            media_type: self.media_type.clone(),
            id: self.id,
            title: self.title.clone(),
            poster: self.poster.clone(),
            backdrop: self.backdrop.clone(),
            seasons: self.seasons.clone(),
            season: self.season,
            episode: self.episode,
            position: self.position,
            duration: self.duration,
            share: None,
            confirmed: self.confirmed,
        })
    }
}

trait MovingTiming {
    fn is_none_or_moving(&self) -> bool;
}

impl MovingTiming for Option<(f64, f64)> {
    /// Started counts once the position is past the very start, or when there's no position at all.
    fn is_none_or_moving(&self) -> bool {
        self.map_or(true, |(t, _)| t > 0.3)
    }
}

/// The address without its query and fragment: players add tracking to it as they go.
fn strip(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

/// Season and episode from the player's text: "T1:E3", "S01E03", "Season 1 Episode 3",
/// "Temporada 1, Episódio 3", "T1 · E3".
pub fn parse_label(text: &str) -> Option<(u32, u32)> {
    // Runs of letters and of digits, in order: "S01E03" → s, 01, e, 03.
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut digits = false;
    for c in text.to_lowercase().chars() {
        let is_digit = c.is_ascii_digit();
        if c.is_alphanumeric() {
            if !cur.is_empty() && is_digit != digits {
                parts.push(std::mem::take(&mut cur));
            }
            digits = is_digit;
            cur.push(c);
        } else if !cur.is_empty() {
            parts.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }

    const SEASON: [&str; 5] = ["s", "t", "season", "temporada", "temp"];
    const EPISODE: [&str; 7] = ["e", "ep", "episode", "episódio", "episodio", "cap", "capítulo"];
    let number_after = |i: usize| parts.get(i + 1).and_then(|p| p.parse::<u32>().ok());
    let season_at = (0..parts.len()).find(|&i| SEASON.contains(&parts[i].as_str()) && number_after(i).is_some())?;
    let season = number_after(season_at)?;
    let episode = (season_at + 2..parts.len())
        .find(|&i| EPISODE.contains(&parts[i].as_str()) && number_after(i).is_some())
        .and_then(number_after)?;
    (season > 0 && episode > 0).then_some((season, episode))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series() -> Session {
        Session::new(
            "netflix".into(),
            "tv".into(),
            7,
            "Dark".into(),
            None,
            None,
            vec![10, 8],
            1,
            3,
        )
    }

    fn at(t: f64, d: f64) -> Sample {
        Sample {
            url: "https://www.netflix.com/watch/111".into(),
            video: true,
            player: true,
            t: Some(t),
            d: Some(d),
            ..Default::default()
        }
    }

    #[test]
    fn labels() {
        assert_eq!(parse_label("Dark T1:E3 Passado e presente"), Some((1, 3)));
        assert_eq!(parse_label("S01E10"), Some((1, 10)));
        assert_eq!(parse_label("Season 2 Episode 4"), Some((2, 4)));
        assert_eq!(parse_label("Temporada 2, Episódio 7 - Nome"), Some((2, 7)));
        assert_eq!(parse_label("T3 · E8"), Some((3, 8)));
        assert_eq!(parse_label("E3 · Nome do episódio"), None);
        assert_eq!(parse_label("Dune: Part Two"), None);
        assert_eq!(parse_label("Stranger Things 4"), None);
    }

    #[test]
    fn starts_once_the_video_moves() {
        let mut s = series();
        assert_eq!(s.on_sample(Sample { player: true, ..Default::default() }, 0.0), Outcome::Nothing);
        assert!(s.update().is_none());
        assert_eq!(s.on_sample(at(0.0, 3000.0), 1.0), Outcome::Nothing);
        assert_eq!(s.on_sample(at(2.0, 3000.0), 3.0), Outcome::Started);
        assert_eq!(s.on_sample(at(4.0, 3000.0), 5.0), Outcome::Nothing);
        let u = s.update().unwrap();
        assert_eq!((u.season, u.episode, u.position, u.duration), (1, 3, 4.0, 3000.0));
    }

    #[test]
    fn label_wins() {
        let mut s = series();
        s.on_sample(at(10.0, 3000.0), 0.0);
        let mut x = at(12.0, 3000.0);
        x.label = Some("T2:E1".into());
        s.confirmed = false;
        s.on_sample(x, 2.0);
        assert_eq!((s.season, s.episode), (2, 1));
        assert!(s.update().unwrap().confirmed);
        // Out of range labels are ignored.
        let mut y = at(14.0, 3000.0);
        y.label = Some("T9:E1".into());
        s.on_sample(y, 4.0);
        assert_eq!((s.season, s.episode), (2, 1));
    }

    #[test]
    fn autoplay_of_the_next_episode_moves_on() {
        let mut s = series();
        s.on_sample(at(2900.0, 3000.0), 0.0);
        // Same length, back at the start: the next episode.
        s.on_sample(at(5.0, 3000.0), 2.0);
        assert_eq!((s.season, s.episode), (1, 4));
        // Different length.
        s.on_sample(at(100.0, 2400.0), 4.0);
        assert_eq!((s.season, s.episode), (1, 5));
        // A new watch page.
        let mut next = at(110.0, 2400.0);
        next.url = "https://www.netflix.com/watch/222?trackId=1".into();
        s.on_sample(next, 6.0);
        assert_eq!((s.season, s.episode), (1, 6));
        // Seeking back a little isn't a new episode.
        let mut seek = at(50.0, 2400.0);
        seek.url = "https://www.netflix.com/watch/222".into();
        s.on_sample(seek, 8.0);
        assert_eq!((s.season, s.episode), (1, 6));
    }

    #[test]
    fn last_episode_of_a_season_goes_to_the_next_season() {
        let mut s = series();
        s.episode = 10;
        s.on_sample(at(2900.0, 3000.0), 0.0);
        s.on_sample(at(1.0, 2000.0), 2.0);
        assert_eq!((s.season, s.episode), (2, 1));
    }

    #[test]
    fn hidden_position_counts_play_time_into_the_next_episode() {
        let mut s = series();
        s.runtime = 100.0;
        let blind = |paused| Sample { video: true, player: true, paused, ..Default::default() };
        assert_eq!(s.on_sample(blind(false), 0.0), Outcome::Started);
        s.on_sample(blind(false), 20.0);
        s.on_sample(blind(true), 40.0); // paused from here: not counted
        s.on_sample(blind(true), 70.0);
        s.on_sample(blind(false), 80.0);
        assert_eq!(s.update().unwrap().position, 40.0);
        for i in 1..=4 {
            s.on_sample(blind(false), 80.0 + 20.0 * i as f64);
        }
        // 40 + 80 = 120 s played: 20 s into the next episode.
        let u = s.update().unwrap();
        assert_eq!((u.season, u.episode, u.position, u.duration), (1, 4, 20.0, 100.0));
    }

    #[test]
    fn closes_when_the_player_goes_away_or_the_end_lingers() {
        let mut s = series();
        s.on_sample(at(10.0, 3000.0), 0.0);
        let away = Sample { url: "https://www.netflix.com/browse".into(), ..Default::default() };
        assert_eq!(s.on_sample(away.clone(), 2.0), Outcome::Nothing);
        assert_eq!(s.on_sample(away, 8.0), Outcome::Close);

        let mut s = series();
        s.on_sample(at(10.0, 3000.0), 0.0);
        let mut end = at(3000.0, 3000.0);
        end.ended = true;
        s.on_sample(end.clone(), 1.0);
        assert_eq!(s.on_sample(end.clone(), 20.0), Outcome::Nothing);
        assert_eq!(s.on_sample(end, 27.0), Outcome::Close);
    }

    #[test]
    fn leaving_the_player_closes_at_once() {
        let mut s = series();
        s.on_sample(at(10.0, 3000.0), 0.0);
        let left = Sample { url: "https://www.netflix.com/title/7".into(), left: true, ..Default::default() };
        assert_eq!(s.on_sample(left, 0.5), Outcome::Close);
        assert_eq!(s.update().unwrap().position, 10.0);
    }

    #[test]
    fn nothing_closes_before_playing() {
        let mut s = series();
        let away = Sample { url: "https://www.netflix.com/browse".into(), ..Default::default() };
        s.on_sample(away.clone(), 0.0);
        assert_eq!(s.on_sample(away, 60.0), Outcome::Nothing);
    }
}
