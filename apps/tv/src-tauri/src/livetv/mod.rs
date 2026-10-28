//! The Live tab: every channel the launcher plays live, in one lineup (design canvas: Live, Live ·
//! All channels, Live · Player overlay).
//!
//! - Globoplay's whole lineup, ESPN 1–6 on Disney+ and CazéTV: the live programme the Sports tab
//!   reads too (sports/tv.rs), when a screen asks.
//! - News channels on YouTube (news.rs), from a list the user changes in Settings or through the
//!   assistant.
//! - The Twitch channels followed and the YouTube subscriptions' live streams, as on the Home's
//!   Live now row (live.rs).
//! - Favorite channels, and reminders of what's on later: the calendar's reminder card shows five
//!   minutes before, and A on it tunes in.
//!
//! A channel plays with the live overlay, its rail holding the whole lineup (sports::Lineup::All).
//! Prime Video's live events aren't here: it has no public listing.

mod news;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

pub use news::{Found, NewsSource};

use crate::error::{Error, Result};
use crate::history::now_ms;
use crate::sports::{self, words, Lineup, TvChannel};
use crate::state::{AppState, Mode};
use crate::live;

const SETTINGS_FILE: &str = "live-tv.json";
const MIN: i64 = 60_000;
/// The reminder card shows this long before a programme starts.
const REMIND_BEFORE: i64 = 5 * MIN;
/// A reminder missed by more than this (the launcher was off) isn't shown.
const REMIND_LATE: i64 = 30 * MIN;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reminder {
    /// "<channel key>:<start>", as the screens name a programme.
    pub id: String,
    pub key: String,
    pub start: i64,
    pub title: String,
    pub channel: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Saved {
    #[serde(default = "news::defaults")]
    news: Vec<NewsSource>,
    /// Channel keys, in the order they were pinned.
    #[serde(default)]
    favorites: Vec<String>,
    #[serde(default)]
    reminders: Vec<Reminder>,
}

impl Default for Saved {
    fn default() -> Self {
        Self { news: news::defaults(), favorites: Vec::new(), reminders: Vec::new() }
    }
}

pub struct State {
    path: PathBuf,
    saved: Mutex<Saved>,
    news: news::Cache,
}

impl State {
    pub fn new(data_dir: &Path) -> Self {
        let path = data_dir.join(SETTINGS_FILE);
        let saved = fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Self { saved: Mutex::new(saved), news: news::Cache::new(data_dir), path }
    }

    fn save(&self, saved: &Saved) -> Result<()> {
        fs::write(&self.path, serde_json::to_string_pretty(saved)?)?;
        Ok(())
    }
}

fn state(app: &AppHandle) -> &State {
    &app.state::<AppState>().inner().livetv
}

fn sources(app: &AppHandle) -> Vec<NewsSource> {
    state(app).saved.lock().unwrap().news.clone()
}

// ---------- The lineup ----------

/// Reads the news channels again, when that's due.
pub async fn refresh(app: &AppHandle) {
    news::refresh(app, &state(app).news, &sources(app)).await;
}

/// Every channel with its programme, as last read: Globoplay's, ESPN's, CazéTV's, then the news
/// channels.
fn lineup(app: &AppHandle, now: i64) -> Vec<TvChannel> {
    let mut all = sports::tv_lineup(app, now);
    all.extend(news::channels(&state(app).news, &sources(app), now));
    all
}

/// The overlay's channels: those with something to play, news first, then sports, then the rest
/// of TV, as the Live tab's rows go.
pub fn channels(app: &AppHandle, now: i64) -> Vec<TvChannel> {
    let mut all = lineup(app, now);
    all.retain(|c| c.url.is_some());
    all.sort_by_key(|c| match c.group.as_str() {
        "news" => 0,
        "tv" => 2,
        _ => 1,
    });
    all
}

#[derive(Debug, Clone, Serialize)]
pub struct LiveTv {
    /// Every channel with its programme: Globoplay's, ESPN's, CazéTV's, then the news channels
    /// on YouTube; the games they show filled in.
    pub channels: Vec<TvChannel>,
    /// Live sports (games on the services too), Twitch and the YouTube subscriptions, as on Home.
    pub now: live::LiveNow,
    pub favorites: Vec<String>,
    /// The programmes with a reminder, as "<channel key>:<start>".
    pub reminders: Vec<String>,
    pub news: Vec<NewsSource>,
    /// What isn't loading.
    pub errors: Vec<String>,
}

/// The Live tab.
pub async fn tab(app: &AppHandle) -> LiveTv {
    let channels = async {
        let (mut channels, ()) = futures_util::join!(sports::lineup(app), refresh(app));
        channels.extend(news::channels(&state(app).news, &sources(app), now_ms()));
        sports::with_games(app, &mut channels).await;
        channels
    };
    let (channels, now) = futures_util::join!(channels, live::now(app));
    let st = state(app);
    let mut errors = sports::tv_errors(app);
    errors.extend(st.news.error());
    let saved = st.saved.lock().unwrap().clone();
    LiveTv {
        channels,
        now,
        favorites: saved.favorites,
        reminders: saved.reminders.into_iter().map(|r| r.id).collect(),
        news: saved.news,
        errors,
    }
}

/// Plays channel `key` with the live overlay, its rail holding the whole lineup.
pub async fn watch(app: &AppHandle, key: &str) -> Result<()> {
    sports::watch_tv(app, key, Lineup::All).await
}

/// What's called `name` among `items`: the same name (spaces aside: "Globo News"), else one
/// holding it as words ("cnn" → CNN Brasil), else as letters.
fn named<'a, T>(items: &'a [T], name_of: impl Fn(&T) -> &str, name: &str) -> Option<&'a T> {
    let want = words(name);
    if want.is_empty() {
        return None;
    }
    let tight = |s: &str| s.replace(' ', "");
    let (padded, want_tight) = (format!(" {want} "), tight(&want));
    let find = |hit: &dyn Fn(&str) -> bool| items.iter().find(|x| hit(&words(name_of(x))));
    find(&|n| n == want)
        .or_else(|| find(&|n| tight(n) == want_tight))
        .or_else(|| find(&|n| format!(" {n} ").contains(&padded)))
        .or_else(|| find(&|n| tight(n).contains(&want_tight)))
}

fn named_channel<'a>(list: &'a [TvChannel], name: &str) -> Option<&'a TvChannel> {
    named(list, |c| &c.name, name)
}

/// Tunes in to the channel named like `name` ("GloboNews", "cnn", "sportv 2"); resolves to its
/// name.
pub async fn watch_named(app: &AppHandle, name: &str) -> Result<String> {
    futures_util::join!(sports::lineup(app), refresh(app));
    let now = now_ms();
    let playing = channels(app, now);
    let Some(c) = named_channel(&playing, name) else {
        let all = lineup(app, now);
        return Err(Error::msg(match named_channel(&all, name) {
            Some(c) => format!("{} has nothing live right now", c.name),
            None => format!("No live channel is called {name}"),
        }));
    };
    let (key, found) = (c.key.clone(), c.name.clone());
    watch(app, &key).await?;
    Ok(found)
}

// ---------- Favorites and reminders ----------

/// Pins or unpins a channel; resolves to the favorites.
pub fn favorite(app: &AppHandle, key: &str, on: bool) -> Result<Vec<String>> {
    let st = state(app);
    let mut saved = st.saved.lock().unwrap();
    saved.favorites.retain(|k| k != key);
    if on {
        saved.favorites.push(key.to_string());
    }
    st.save(&saved)?;
    Ok(saved.favorites.clone())
}

/// Turns on or off the reminder of what starts on channel `key` at `start`; resolves to whether
/// it's on now.
pub fn remind(app: &AppHandle, key: &str, start: i64, on: bool) -> Result<bool> {
    let st = state(app);
    let id = format!("{key}:{start}");
    let reminder = if on {
        let all = lineup(app, now_ms());
        let found = all.iter().filter(|c| c.key == key).find_map(|c| c.slots.iter().find(|s| s.start == start).map(|s| (c, s)));
        let (c, s) = found.ok_or_else(|| Error::msg("That programme isn't listed any more"))?;
        Some(Reminder {
            id: id.clone(),
            key: key.to_string(),
            start,
            title: s.title.clone(),
            channel: c.name.clone(),
            color: c.color.clone(),
        })
    } else {
        None
    };
    let mut saved = st.saved.lock().unwrap();
    saved.reminders.retain(|r| r.id != id);
    saved.reminders.extend(reminder);
    st.save(&saved)?;
    Ok(on)
}

/// Shows the reminders due as the calendar's card: over the launcher, and over the page of the
/// app in front.
fn check_reminders(app: &AppHandle) {
    let now = now_ms();
    let st = state(app);
    let due: Vec<Reminder> = {
        let mut saved = st.saved.lock().unwrap();
        let (due, keep): (Vec<Reminder>, Vec<Reminder>) =
            saved.reminders.drain(..).partition(|r| now >= r.start - REMIND_BEFORE);
        saved.reminders = keep;
        if due.is_empty() {
            return;
        }
        let _ = st.save(&saved);
        due
    };
    let streaming = app.state::<AppState>().mode() == Mode::Streaming;
    for r in due.iter().filter(|r| now < r.start + REMIND_LATE) {
        let title = format!("{} · {}", r.title, r.channel);
        if streaming {
            crate::overlay::card_over_app(app, &title, r.start, &r.color);
        }
        // The calendar event the card expects, with the channel A tunes in to.
        let _ = app.emit(
            "calendar-reminder",
            json!({
                "id": format!("tv:{}", r.id), "calendar": "tv", "title": title, "start": r.start, "end": r.start + 60 * MIN,
                "date": null, "end_date": null, "color": r.color, "location": null, "description": null, "meet": null,
                "declined": false, "remind": true, "game": null, "tv": r.key,
            }),
        );
    }
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            check_reminders(&app);
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}

// ---------- The news channels ----------

pub fn news_list(app: &AppHandle) -> Vec<NewsSource> {
    sources(app)
}

pub async fn search_news(app: &AppHandle, query: &str) -> Result<Vec<Found>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    news::search(app, query).await
}

/// Lists YouTube channel `id` among the news channels; resolves to the list.
pub fn add_news(app: &AppHandle, id: &str, name: &str) -> Result<Vec<NewsSource>> {
    let st = state(app);
    let mut saved = st.saved.lock().unwrap();
    if saved.news.iter().any(|s| s.id == id) {
        return Err(Error::msg(format!("{name} is in the news channels already")));
    }
    let color = news::color_for(saved.news.len());
    saved.news.push(NewsSource { id: id.to_string(), name: name.to_string(), color });
    st.save(&saved)?;
    Ok(saved.news.clone())
}

pub fn remove_news(app: &AppHandle, id: &str) -> Result<Vec<NewsSource>> {
    let st = state(app);
    let mut saved = st.saved.lock().unwrap();
    saved.news.retain(|s| s.id != id);
    let key = format!("yt-{id}");
    saved.favorites.retain(|k| *k != key);
    saved.reminders.retain(|r| r.key != key);
    st.save(&saved)?;
    Ok(saved.news.clone())
}

/// Finds the YouTube channel named like `name` and lists it; resolves to its name on YouTube.
pub async fn add_news_named(app: &AppHandle, name: &str) -> Result<String> {
    let found = search_news(app, name).await?;
    let first = found.first().ok_or_else(|| Error::msg(format!("No YouTube channel is called {name}")))?;
    add_news(app, &first.id, &first.name)?;
    Ok(first.name.clone())
}

/// Takes the news channel named like `name` off the list; resolves to its name.
pub fn remove_news_named(app: &AppHandle, name: &str) -> Result<String> {
    let list = sources(app);
    let found = named(&list, |s| &s.name, name).ok_or_else(|| Error::msg(format!("{name} isn't among the news channels")))?;
    remove_news(app, &found.id)?;
    Ok(found.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(key: &str, name: &str) -> TvChannel {
        TvChannel {
            key: key.into(),
            name: name.into(),
            color: "#000000".into(),
            group: "news".into(),
            provider: "youtube".into(),
            access: String::new(),
            free: true,
            frame: None,
            app: "youtube".into(),
            url: None,
            slots: Vec::new(),
            viewers: None,
        }
    }

    #[test]
    fn channels_by_name() {
        let list = [ch("globonews", "GloboNews"), ch("tv-globo", "TV Globo"), ch("yt-band", "Band Jornalismo"), ch("yt-bnfm", "BandNews FM"), ch("sportv-2", "sportv 2"), ch("sportv", "sportv")];
        let key = |n: &str| named_channel(&list, n).map(|c| c.key.as_str());
        assert_eq!(key("globo"), Some("tv-globo"));
        assert_eq!(key("Globo News"), Some("globonews"));
        assert_eq!(key("band"), Some("yt-band"));
        assert_eq!(key("bandnews"), Some("yt-bnfm"));
        assert_eq!(key("SporTV 2"), Some("sportv-2"));
        assert_eq!(key("sportv"), Some("sportv"));
        assert_eq!(key("cnn"), None);
    }
}
