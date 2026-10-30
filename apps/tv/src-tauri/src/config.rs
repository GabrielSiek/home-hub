use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;

const DEFAULT_APPS: &str = include_str!("../defaults/apps.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppKind {
    Web,
    Uri,
    Exec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppEntry {
    pub id: String,
    pub name: String,
    pub kind: AppKind,
    pub url: Option<String>,
    pub search_url: Option<String>,
    pub exec: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub user_agent: Option<String>,
    #[serde(default)]
    pub nav: Nav,
    #[serde(default)]
    pub tmdb_providers: Vec<u32>,
    /// False keeps a streaming service's Home tile opening its website. Otherwise the launcher
    /// shows its own copy of the service's screens, and the website opens only to play.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emulate: Option<bool>,
    /// Profile to pick when the service asks "Who's watching?" before playing. Once picked, its
    /// id is learned (watch/profiles.rs) and Play opens it without the picker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

/// How the pad drives a web app.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Nav {
    /// TV mode: a highlight moved between the site's links and buttons (tvnav.js).
    #[default]
    Tv,
    /// Plain keys (arrows, Enter, Esc), for sites with a TV interface of their own.
    Native,
}

#[derive(Deserialize)]
struct AppsFile {
    #[serde(default)]
    apps: Vec<AppEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Browser {
    Edge,
    Chrome,
}

/// Controller button that returns to the launcher's Home screen from anywhere.
/// Select + Start held together always works as well.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HomeButton {
    /// The controller's own Home/Guide button (8BitDo Home in X-input mode).
    Guide,
    Select,
    Start,
    L3,
    R3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// TMDB v3 API key or v4 read access token.
    pub tmdb_api_key: String,
    /// Watch-provider region (ISO 3166-1).
    pub region: String,
    /// TMDB result language.
    pub language: String,
    pub browser: Browser,
    /// Overrides the browser executable found in the registry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser_path: Option<String>,
    pub home_button: HomeButton,
    /// Steam Web API key: lists the whole owned library.
    pub steam_api_key: String,
    /// SteamID64 to list instead of the account last signed in to Steam.
    pub steam_id: String,
    /// SteamGridDB key: covers for games without local art.
    pub steamgriddb_api_key: String,
    /// YouTube Data API key: CazéTV's live and scheduled streams on the Sports tab.
    pub youtube_api_key: String,
    /// eWeLink developer app (dev.ewelink.cc): APPID and APP SECRET, for Sonoff devices.
    pub ewelink_app_id: String,
    pub ewelink_app_secret: String,
    /// Tuya cloud project (iot.tuya.com): Access ID and Access Secret, for Positivo/Tuya devices.
    pub tuya_access_id: String,
    pub tuya_access_secret: String,
    /// The project's data center: us, us-e, eu, eu-w, in, cn or sg. Empty: TUYA_REGION, else us.
    pub tuya_region: String,
    /// DeepInfra key for the assistant: the LLM and speech recognition.
    pub deepinfra_api_key: String,
    /// DeepInfra model id for the assistant. Empty: `assistant::llm::DEFAULT_MODEL`.
    pub assistant_model: String,
    /// How replies are read out.
    pub assistant_speech: Speech,
    /// Kokoro voice, e.g. pf_dora. Empty: one for `language`.
    pub assistant_voice: String,
    /// City for the weather. Empty: located by IP address.
    pub assistant_location: String,
    /// Spotify app (developer.spotify.com) Client ID. The sign-in uses PKCE: no secret.
    pub spotify_client_id: String,
    /// Last.fm API key: everyone's most played songs on an artist's screen.
    pub lastfm_api_key: String,
    /// Twitch app (dev.twitch.tv) Client ID, of the Public type. The sign-in uses the device
    /// code flow: no secret.
    pub twitch_client_id: String,
    /// Google Cloud OAuth client of the "Desktop app" type, with the Photos Picker API enabled:
    /// albums for the screensaver. Desktop clients need the secret too; Google doesn't treat
    /// it as secret for installed apps.
    pub google_client_id: String,
    pub google_client_secret: String,
    /// Minutes without input, with no music playing, before the screensaver starts. 0: never.
    pub screensaver_minutes: u32,
    /// How long each photo stays up.
    pub screensaver_seconds: u32,
    /// Moves the mouse pointer to the top-right corner and hides it while the launcher is in
    /// front: when it opens and after the mouse sits still for a moment.
    pub hide_cursor: bool,
    /// A card on the TV ten minutes before each event on a calendar shown on the TV.
    pub calendar_reminders: bool,
    /// Where the drive to an event's place starts. Empty: no travel times.
    pub home_address: String,
    /// The home server has a library (its keys stay there).
    pub library_on: bool,
    /// The house's library, in Settings only: kept by the server, its keys masked.
    pub radarr_url: String,
    pub radarr_api_key: String,
    pub sonarr_url: String,
    pub sonarr_api_key: String,
    pub qbittorrent_url: String,
    pub qbittorrent_username: String,
    pub qbittorrent_password: String,
    pub library_quality: String,
}

/// The assistant's spoken replies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Speech {
    /// Kokoro on this PC; Windows' own voices until it's downloaded or when it fails. Replaces
    /// Kokoro on DeepInfra, which took seconds per reply.
    #[serde(alias = "deepinfra")]
    Local,
    /// Windows' voices (speech synthesis in the webview).
    Windows,
    Off,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            tmdb_api_key: String::new(),
            region: "BR".into(),
            language: "pt-BR".into(),
            browser: Browser::Edge,
            browser_path: None,
            home_button: HomeButton::Guide,
            steam_api_key: String::new(),
            steam_id: String::new(),
            steamgriddb_api_key: String::new(),
            youtube_api_key: String::new(),
            ewelink_app_id: String::new(),
            ewelink_app_secret: String::new(),
            tuya_access_id: String::new(),
            tuya_access_secret: String::new(),
            tuya_region: String::new(),
            deepinfra_api_key: String::new(),
            assistant_model: String::new(),
            assistant_speech: Speech::Local,
            assistant_voice: String::new(),
            assistant_location: String::new(),
            spotify_client_id: String::new(),
            lastfm_api_key: String::new(),
            twitch_client_id: String::new(),
            google_client_id: String::new(),
            google_client_secret: String::new(),
            screensaver_minutes: 5,
            screensaver_seconds: 15,
            hide_cursor: true,
            calendar_reminders: true,
            home_address: String::new(),
            library_on: false,
            radarr_url: String::new(),
            radarr_api_key: String::new(),
            sonarr_url: String::new(),
            sonarr_api_key: String::new(),
            qbittorrent_url: String::new(),
            qbittorrent_username: String::new(),
            qbittorrent_password: String::new(),
            library_quality: String::new(),
        }
    }
}

/// A value from Settings, else the environment variable (`.env`). Empty means unset.
pub fn setting_or_env(value: &str, env: &str) -> Option<String> {
    let v = value.trim();
    if !v.is_empty() {
        return Some(v.to_string());
    }
    std::env::var(env).ok().map(|e| e.trim().to_string()).filter(|e| !e.is_empty())
}

impl Settings {
    pub fn tmdb_key(&self) -> Option<String> {
        setting_or_env(&self.tmdb_api_key, "TMDB_API_KEY")
    }

    pub fn steam_key(&self) -> Option<String> {
        setting_or_env(&self.steam_api_key, "STEAM_API_KEY")
    }

    pub fn steam_id(&self) -> Option<u64> {
        setting_or_env(&self.steam_id, "STEAM_ID").and_then(|s| s.parse().ok())
    }

    pub fn steamgriddb_key(&self) -> Option<String> {
        setting_or_env(&self.steamgriddb_api_key, "STEAMGRIDDB_API_KEY")
    }

    pub fn youtube_key(&self) -> Option<String> {
        setting_or_env(&self.youtube_api_key, "YOUTUBE_API_KEY")
    }

    pub fn spotify_client_id(&self) -> Option<String> {
        setting_or_env(&self.spotify_client_id, "SPOTIFY_CLIENT_ID")
    }

    pub fn lastfm_key(&self) -> Option<String> {
        setting_or_env(&self.lastfm_api_key, "LASTFM_API_KEY")
    }

    pub fn twitch_client_id(&self) -> Option<String> {
        setting_or_env(&self.twitch_client_id, "TWITCH_CLIENT_ID")
    }

    /// Google OAuth Client ID and secret, when both are set.
    pub fn google_client(&self) -> Option<(String, String)> {
        Some((
            setting_or_env(&self.google_client_id, "GOOGLE_CLIENT_ID")?,
            setting_or_env(&self.google_client_secret, "GOOGLE_CLIENT_SECRET")?,
        ))
    }
}

pub struct Config {
    pub apps: Vec<AppEntry>,
    pub settings: Settings,
}

impl Config {
    /// Loads apps.toml and settings.toml from `dir`, writing the bundled apps.toml on first run.
    pub fn load_or_init(dir: &Path) -> Result<Self> {
        let apps_path = dir.join("apps.toml");
        if !apps_path.exists() {
            fs::write(&apps_path, DEFAULT_APPS)?;
        }
        let apps = parse_apps(&fs::read_to_string(&apps_path)?)?;

        let settings_path = dir.join("settings.toml");
        let settings = if settings_path.exists() {
            toml::from_str(&fs::read_to_string(&settings_path)?)?
        } else {
            Settings::default()
        };
        Ok(Self { apps, settings })
    }

    pub fn save_settings(dir: &Path, settings: &Settings) -> Result<()> {
        fs::write(dir.join("settings.toml"), toml::to_string_pretty(settings)?)?;
        Ok(())
    }
}

/// The house's library as a Home tile. It isn't in apps.toml: it's there when the server has one.
pub fn library_app() -> AppEntry {
    AppEntry {
        id: crate::watch::library::SERVICE.to_string(),
        name: "Library".to_string(),
        kind: AppKind::Web,
        url: None,
        search_url: None,
        exec: None,
        args: Vec::new(),
        icon: None,
        color: Some("#6d5dfc".to_string()),
        user_agent: None,
        nav: Nav::Tv,
        tmdb_providers: Vec::new(),
        emulate: None,
        profile: None,
    }
}

pub fn parse_apps(src: &str) -> Result<Vec<AppEntry>> {
    Ok(toml::from_str::<AppsFile>(src)?.apps)
}

const IMAGE_EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

/// The tile image for an app: its `icon` from apps.toml, else `icons/<id>.<png|jpg|jpeg|webp>`
/// in the data folder. None (the tile shows the name) when neither file exists.
pub fn tile_image(dir: &Path, app: &AppEntry) -> Option<PathBuf> {
    if let Some(icon) = app.icon.as_deref() {
        let p = dir.join(icon);
        if p.is_file() {
            return Some(p);
        }
    }
    IMAGE_EXTENSIONS
        .iter()
        .map(|ext| dir.join("icons").join(format!("{}.{ext}", app.id)))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_apps_parse() {
        let apps = parse_apps(DEFAULT_APPS).unwrap();
        assert!(apps.len() >= 5);
        let netflix = apps.iter().find(|a| a.id == "netflix").unwrap();
        assert_eq!(netflix.kind, AppKind::Web);
        assert_eq!(netflix.tmdb_providers, vec![8]);
        assert!(netflix.search_url.as_deref().unwrap().contains("{q}"));
        let ids: std::collections::HashSet<_> = apps.iter().map(|a| &a.id).collect();
        assert_eq!(ids.len(), apps.len(), "app ids must be unique");
    }

    #[test]
    fn minimal_entry_uses_defaults() {
        let apps = parse_apps(
            r#"
            [[apps]]
            id = "steam"
            name = "Steam"
            kind = "uri"
            url = "steam://open/bigpicture"
            "#,
        )
        .unwrap();
        assert_eq!(apps[0].kind, AppKind::Uri);
        assert!(apps[0].args.is_empty());
        assert!(apps[0].search_url.is_none());
        assert_eq!(apps[0].nav, Nav::Tv);
    }

    #[test]
    fn youtube_uses_its_own_tv_interface() {
        let apps = parse_apps(DEFAULT_APPS).unwrap();
        let yt = apps.iter().find(|a| a.id == "youtube").unwrap();
        assert_eq!(yt.nav, Nav::Native);
        assert!(yt.user_agent.as_deref().unwrap().contains("Cobalt/"));
        assert!(apps.iter().filter(|a| a.id != "youtube").all(|a| a.nav == Nav::Tv));
    }

    #[test]
    fn tile_image_prefers_icon_then_id_convention() {
        let dir = std::env::temp_dir().join(format!("tvl-tile-{}", std::process::id()));
        fs::create_dir_all(dir.join("icons")).unwrap();
        let mut app = parse_apps(DEFAULT_APPS).unwrap().remove(0); // netflix

        assert_eq!(tile_image(&dir, &app), None);

        fs::write(dir.join("icons/netflix.webp"), b"x").unwrap();
        assert_eq!(tile_image(&dir, &app), Some(dir.join("icons/netflix.webp")));

        // An icon that doesn't exist falls back to the convention.
        app.icon = Some("art/missing.png".into());
        assert_eq!(tile_image(&dir, &app), Some(dir.join("icons/netflix.webp")));

        fs::create_dir_all(dir.join("art")).unwrap();
        fs::write(dir.join("art/missing.png"), b"x").unwrap();
        assert_eq!(tile_image(&dir, &app), Some(dir.join("art/missing.png")));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn settings_round_trip_and_partial_file() {
        let partial: Settings = toml::from_str(r#"tmdb_api_key = "abc""#).unwrap();
        assert_eq!(partial.tmdb_api_key, "abc");
        assert_eq!(partial.region, "BR");
        assert_eq!(partial.browser, Browser::Edge);
        assert_eq!(partial.home_button, HomeButton::Guide);
        assert_eq!(partial.assistant_speech, Speech::Local);
        // Saved before the voice moved to this PC.
        let old: Settings = toml::from_str(r#"assistant_speech = "deepinfra""#).unwrap();
        assert_eq!(old.assistant_speech, Speech::Local);
        assert!(partial.hide_cursor);
        assert_eq!(partial.screensaver_minutes, 5);

        let text = toml::to_string_pretty(&partial).unwrap();
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back.tmdb_api_key, "abc");
        assert!(back.browser_path.is_none());

        let r3: Settings = toml::from_str(r#"home_button = "r3""#).unwrap();
        assert_eq!(r3.home_button, HomeButton::R3);
    }
}
