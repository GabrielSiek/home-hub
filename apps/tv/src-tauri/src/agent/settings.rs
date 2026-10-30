//! This TV PC's settings: its own (browser, pad, screensaver, voice) kept here, and the
//! household's (keys, region, language, the assistant's, the calendar's) kept by the server. The
//! server sends the keys this TV's media modules need (the `settings` event), and they're kept here
//! too, for when it's away. The TV UI's Settings shows the household's as the server has them (its
//! keys masked) and saves them there.

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::config::{Config, Settings};
use crate::error::{Error, Result};
use crate::state::AppState;

/// The settings the household shares (the server's `settings.toml`), by name.
const HOUSE: &[&str] = &[
    "tmdb_api_key",
    "region",
    "language",
    "steam_api_key",
    "steamgriddb_api_key",
    "youtube_api_key",
    "deepinfra_api_key",
    "assistant_model",
    "assistant_location",
    "spotify_client_id",
    "lastfm_api_key",
    "twitch_client_id",
    "google_client_id",
    "google_client_secret",
    "calendar_reminders",
    "home_address",
    "library_on",
    "radarr_url",
    "radarr_api_key",
    "sonarr_url",
    "sonarr_api_key",
    "qbittorrent_url",
    "qbittorrent_username",
    "qbittorrent_password",
    "library_quality",
];

/// `base` with `house`'s household fields over it.
fn with_house(base: &Settings, house: &Value) -> Option<Settings> {
    let mut v = serde_json::to_value(base).ok()?;
    for k in HOUSE {
        if let Some(x) = house.get(*k) {
            v[*k] = x.clone();
        }
    }
    serde_json::from_value(v).ok()
}

/// What Settings shows: this TV's own, and the household's as the server has them.
pub async fn get(app: &AppHandle) -> Settings {
    let local = app.state::<AppState>().config.read().unwrap().settings.clone();
    match crate::agent::server_call(app, "server_settings_get", json!({})).await {
        Ok(house) => with_house(&local, &house).unwrap_or(local),
        Err(_) => local,
    }
}

/// Saves Settings: the household's on the server (a key still masked keeps its value there), this
/// TV's own here.
pub async fn save(app: &AppHandle, settings: Settings) -> Result<()> {
    let house = crate::agent::server_call(app, "server_settings_save", json!({ "settings": settings })).await;
    let st = app.state::<AppState>();
    let (old, new) = {
        let mut cfg = st.config.write().unwrap();
        let old = cfg.settings.clone();
        // The household's stay as the server sent them: never a masked key.
        let kept = serde_json::to_value(&old)?;
        let new = with_house(&settings, &kept).ok_or_else(|| Error::msg("Those settings don't read"))?;
        cfg.settings = new.clone();
        (old, new)
    };
    Config::save_settings(&st.data_dir, &new)?;
    crate::commands::settings_changed(app, &old, &new);
    house.map(|_| ()).map_err(|e| Error::msg(format!("This TV's settings are saved, the household's aren't: {e}")))
}

/// The server's `settings` event: the household's keys this TV's media modules use.
pub fn apply_house(app: &AppHandle, house: &Value) {
    let st = app.state::<AppState>();
    let (old, new) = {
        let mut cfg = st.config.write().unwrap();
        let old = cfg.settings.clone();
        let Some(new) = with_house(&old, house) else { return };
        if serde_json::to_value(&new).ok() == serde_json::to_value(&old).ok() {
            return;
        }
        cfg.settings = new.clone();
        (old, new)
    };
    if let Err(e) = Config::save_settings(&st.data_dir, &new) {
        tracing::warn!("can't keep the household's settings here: {e}");
    }
    crate::commands::settings_changed(app, &old, &new);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn household_fields_come_from_the_server_and_the_rest_stays() {
        let mine = Settings { steam_id: "7656".into(), tmdb_api_key: "old".into(), ..Settings::default() };
        let house = json!({ "tmdb_api_key": "new", "region": "PT", "steam_id": "not the server's", "browser": "chrome" });
        let s = with_house(&mine, &house).unwrap();
        assert_eq!((s.tmdb_api_key.as_str(), s.region.as_str()), ("new", "PT"));
        assert_eq!(s.steam_id, "7656", "a TV PC's own setting");
        assert_eq!(s.browser, crate::config::Browser::Edge);
    }
}
