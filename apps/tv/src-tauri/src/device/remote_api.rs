//! The commands the home server forwards to this TV PC from a phone or tablet: the TV's own
//! (commands.rs), with the arguments named as the TV's invoke() sends them (camelCase). Which ones
//! may come is the server's table (`tvl_protocol::commands`, the Public device commands); settings,
//! sign-ins and anything that opens a window on the PC stay with the TV's own screen.
//!
//! What a forwarded call can do is capped here too: `launch_app_at` only opens the app's own site,
//! and `remote_text` types at most a short text.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};

use super::tv;
use crate::commands as c;
use crate::error::Error;
use crate::state::AppState;

/// Longer than any search box or password needs.
const MOST_TEXT: usize = 500;

pub enum Failure {
    /// Not a command a phone or tablet can call.
    Unknown,
    Failed(Error),
}

impl From<Error> for Failure {
    fn from(e: Error) -> Self {
        Failure::Failed(e)
    }
}

impl From<serde_json::Error> for Failure {
    fn from(e: serde_json::Error) -> Self {
        Failure::Failed(e.into())
    }
}

type Answer = std::result::Result<Value, Failure>;

/// Argument `key` of `args`; a missing one reads as null (None for options).
fn arg<T: DeserializeOwned>(args: &Value, key: &str) -> std::result::Result<T, Failure> {
    serde_json::from_value(args.get(key).cloned().unwrap_or(Value::Null))
        .map_err(|e| Failure::Failed(Error::msg(format!("argument {key}: {e}"))))
}

fn json(v: impl Serialize) -> Answer {
    Ok(serde_json::to_value(v)?)
}

/// What `tv_play` may run: the commands that put something on the TV.
const PLAYS: &[&str] = &["watch_play", "launch_app", "launch_app_at", "live_watch", "youtube_watch", "sports_watch", "sports_watch_tv"];

/// `url` is an https address on web app `id`'s own site (or a subdomain of it): a phone may open
/// a trailer in the YouTube app, not any page in the PC's signed-in browser.
fn on_the_apps_site(app: &AppHandle, id: &str, url: &str) -> bool {
    let home = app.state::<AppState>().config.read().unwrap().apps.iter().find(|a| a.id == id).and_then(|a| a.url.clone());
    let (Some(home), Ok(url)) = (home, reqwest::Url::parse(url)) else { return false };
    let Some(site) = reqwest::Url::parse(&home).ok().and_then(|h| h.host_str().map(|s| s.trim_start_matches("www.").to_string())) else {
        return false;
    };
    let host = url.host_str().unwrap_or_default();
    url.scheme() == "https" && (host == site || host.ends_with(&format!(".{site}")))
}

pub async fn dispatch(app: &AppHandle, cmd: &str, a: Value) -> Answer {
    let h = || app.clone();
    let st = || app.state::<AppState>();
    match cmd {
        // ---------- The TV, as a phone's remote ----------
        "tv_now" => json(tv::now(app)),
        "tv_volume" => json(tv::volume(app, arg(&a, "level")?, arg(&a, "muted")?)?),
        "tv_off" => json(tv::off()?),
        "remote_key" => json(tv::key(app, arg(&a, "key")?)?),
        "remote_text" => {
            let text: String = arg(&a, "text")?;
            if text.chars().count() > MOST_TEXT {
                return Err(Failure::Failed(Error::msg("That's too long to type on the TV")));
            }
            json(tv::text(&text)?)
        }
        "tv_play" => {
            let inner: String = arg(&a, "cmd")?;
            if !PLAYS.contains(&inner.as_str()) {
                return Err(Failure::Unknown);
            }
            let show: tv::Showing = arg(&a, "show")?;
            Box::pin(dispatch(app, &inner, a.get("args").cloned().unwrap_or(Value::Null))).await?;
            tv::showing(app, show);
            json(())
        }

        // ---------- What's on, apps, the catalog ----------
        "get_status" => json(c::get_status(st())),
        // The library's tile opens a TV screen; a phone plays from it through its titles.
        "list_apps" => json(c::list_apps(st()).into_iter().filter(|a| a.id != crate::watch::library::SERVICE).collect::<Vec<_>>()),
        "launch_app" => json(c::launch_app(h(), arg(&a, "id")?, arg(&a, "title")?, arg(&a, "backdrop")?).await?),
        "launch_app_at" => {
            let (id, url): (String, String) = (arg(&a, "id")?, arg(&a, "url")?);
            if !on_the_apps_site(app, &id, &url) {
                return Err(Failure::Failed(Error::msg("Only that app's own pages can be opened from another screen")));
            }
            json(c::launch_app_at(h(), id, url).await?)
        }
        "close_app" => {
            c::close_app(h()).await;
            json(())
        }
        "get_history" => json(c::get_history(st())),
        "tmdb_search" => json(c::tmdb_search(st(), arg(&a, "query")?, arg(&a, "page")?, arg(&a, "mine")?).await?),
        "tmdb_providers" => json(c::tmdb_providers(st(), arg(&a, "mediaType")?, arg(&a, "id")?).await?),
        "tmdb_details" => json(c::tmdb_details(st(), arg(&a, "mediaType")?, arg(&a, "id")?).await?),
        "tmdb_season" => json(c::tmdb_season(st(), arg(&a, "id")?, arg(&a, "number")?).await?),
        "tmdb_search_on" => json(c::tmdb_search_on(st(), arg(&a, "service")?, arg(&a, "query")?, arg(&a, "page")?).await?),
        "tmdb_discover" => json(c::tmdb_discover(st(), arg(&a, "service")?, arg(&a, "query")?).await?),
        "watch_play" => json(
            c::watch_play(h(), arg(&a, "service")?, arg(&a, "mediaType")?, arg(&a, "id")?, arg(&a, "season")?, arg(&a, "episode")?)
                .await?,
        ),
        "watch_progress" => json(c::watch_progress(st(), arg(&a, "service")?)),
        "watch_title" => json(c::watch_title(st(), arg(&a, "service")?, arg(&a, "mediaType")?, arg(&a, "id")?)),
        "watchlist" => json(c::watchlist(st(), arg(&a, "service")?)),
        "watchlist_toggle" => json(c::watchlist_toggle(st(), arg(&a, "item")?)?),
        "recs_shelves" => json(c::recs_shelves(h(), arg(&a, "service")?).await?),
        "recs_row" => json(c::recs_row(h(), arg(&a, "service")?, arg(&a, "shelf")?).await?),


        // ---------- Live TV, sports, streams ----------
        "live_now" => json(c::live_now(h()).await),
        "live_tv" => json(c::live_tv(h()).await),
        "live_watch" => json(c::live_watch(h(), arg(&a, "key")?).await?),
        "live_favorite" => json(c::live_favorite(h(), arg(&a, "key")?, arg(&a, "on")?)?),
        "youtube_watch" => json(c::youtube_watch(h(), arg(&a, "id")?).await?),
        "sports_home" => json(c::sports_home(h()).await?),
        "sports_tv" => json(c::sports_tv(h()).await?),
        "sports_watch" => json(c::sports_watch(h(), arg(&a, "channel")?, arg(&a, "game")?).await?),
        "sports_watch_tv" => json(c::sports_watch_tv(h(), arg(&a, "key")?).await?),
        "twitch_get" => json(c::twitch_get(h())),
        "twitch_streams" => json(c::twitch_streams(h(), arg(&a, "source")?, arg(&a, "gameId")?).await?),

        // ---------- The microphone ----------
        // A phone without its own microphone (no HTTPS, an iOS 12 home screen app) talks through
        // this TV PC's.
        "assistant_listen" => json(c::assistant_listen(h()).await?),
        "assistant_listen_stop" => {
            c::assistant_listen_stop(st());
            json(())
        }
        "assistant_listen_cancel" => {
            c::assistant_listen_cancel(st());
            json(())
        }

        _ => Err(Failure::Unknown),
    }
}

#[cfg(test)]
mod tests {
    /// Every command the server may forward here is one this executor runs, and nothing else is
    /// reachable from another screen.
    #[test]
    fn runs_every_device_command_a_phone_may_send() {
        let src = include_str!("remote_api.rs");
        let arms: std::collections::BTreeSet<&str> = src
            .lines()
            .filter_map(|l| l.trim().strip_prefix('"')?.split_once("\" =>").map(|(name, _)| name))
            .collect();
        let forwarded: std::collections::BTreeSet<&str> = tvl_protocol::COMMANDS
            .iter()
            .filter(|c| c.owner == tvl_protocol::Owner::Device && c.access == tvl_protocol::Access::Public)
            .map(|c| c.name)
            .collect();
        assert_eq!(arms, forwarded);
    }
}
