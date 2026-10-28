//! The household's settings and keys, kept by the server (settings.toml in its data folder). The
//! old launcher mixed these with each TV PC's own settings in one file and read its keys from a
//! `.env` found by walking up from its working folder; a service has no such folder, so the import
//! copies the keys here. Each TV PC keeps its own settings (browser, pad, screensaver, voice).
//!
//! Keys never reach a browser: they're masked ("••••3f9a"), and saving a masked value keeps the
//! old one. TV PCs get the ones their media modules need, unmasked.

use std::path::{Path, PathBuf};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tvl_kit::log::masked;
use tvl_kit::Result;

pub const FILE: &str = "settings.toml";

/// The old launcher's `Settings` fields that belong to the household, same names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct House {
    pub tmdb_api_key: String,
    /// Watch-provider region (ISO 3166-1).
    pub region: String,
    /// TMDB result language; also the assistant's.
    pub language: String,
    pub steam_api_key: String,
    pub steamgriddb_api_key: String,
    pub youtube_api_key: String,
    pub deepinfra_api_key: String,
    pub assistant_model: String,
    /// City for the weather. Empty: located by IP address.
    pub assistant_location: String,
    pub spotify_client_id: String,
    pub lastfm_api_key: String,
    pub twitch_client_id: String,
    pub google_client_id: String,
    pub google_client_secret: String,
    /// A card on the TVs ten minutes before each event.
    pub calendar_reminders: bool,
    /// Where the drive to an event's place starts. Empty: no travel times.
    pub home_address: String,
}

impl Default for House {
    fn default() -> Self {
        Self {
            tmdb_api_key: String::new(),
            region: "BR".into(),
            language: "pt-BR".into(),
            steam_api_key: String::new(),
            steamgriddb_api_key: String::new(),
            youtube_api_key: String::new(),
            deepinfra_api_key: String::new(),
            assistant_model: String::new(),
            assistant_location: String::new(),
            spotify_client_id: String::new(),
            lastfm_api_key: String::new(),
            twitch_client_id: String::new(),
            google_client_id: String::new(),
            google_client_secret: String::new(),
            calendar_reminders: true,
            home_address: String::new(),
        }
    }
}

/// The fields that are keys or secrets, by name, with the `.env` variable the old launcher read.
pub const KEYS: &[(&str, &str)] = &[
    ("tmdb_api_key", "TMDB_API_KEY"),
    ("steam_api_key", "STEAM_API_KEY"),
    ("steamgriddb_api_key", "STEAMGRIDDB_API_KEY"),
    ("youtube_api_key", "YOUTUBE_API_KEY"),
    ("deepinfra_api_key", "DEEPINFRA_API_KEY"),
    ("spotify_client_id", "SPOTIFY_CLIENT_ID"),
    ("lastfm_api_key", "LASTFM_API_KEY"),
    ("twitch_client_id", "TWITCH_CLIENT_ID"),
    ("google_client_id", "GOOGLE_CLIENT_ID"),
    ("google_client_secret", "GOOGLE_CLIENT_SECRET"),
];

impl House {
    fn field_mut(&mut self, name: &str) -> Option<&mut String> {
        Some(match name {
            "tmdb_api_key" => &mut self.tmdb_api_key,
            "steam_api_key" => &mut self.steam_api_key,
            "steamgriddb_api_key" => &mut self.steamgriddb_api_key,
            "youtube_api_key" => &mut self.youtube_api_key,
            "deepinfra_api_key" => &mut self.deepinfra_api_key,
            "spotify_client_id" => &mut self.spotify_client_id,
            "lastfm_api_key" => &mut self.lastfm_api_key,
            "twitch_client_id" => &mut self.twitch_client_id,
            "google_client_id" => &mut self.google_client_id,
            "google_client_secret" => &mut self.google_client_secret,
            _ => return None,
        })
    }

    /// Fills each empty key from `env` (variable → value), as the old launcher's
    /// `setting_or_env` did: a value set in Settings wins.
    pub fn fill_from_env(&mut self, env: &[(String, String)]) -> Vec<&'static str> {
        let mut filled = Vec::new();
        for (field, var) in KEYS {
            let Some(value) = env.iter().find(|(k, _)| k == var).map(|(_, v)| v.trim()).filter(|v| !v.is_empty()) else { continue };
            let slot = self.field_mut(field).expect("every key is a field");
            if slot.trim().is_empty() {
                *slot = value.to_string();
                filled.push(*field);
            }
        }
        filled
    }

    /// What a browser may see: every key masked.
    pub fn masked(&self) -> House {
        let mut out = self.clone();
        for (field, _) in KEYS {
            let slot = out.field_mut(field).expect("every key is a field");
            *slot = masked(slot);
        }
        out
    }

    /// `next` as saved from a screen: a key still showing its mask keeps the value it had.
    pub fn merged(&self, mut next: House) -> House {
        let mut old = self.clone();
        for (field, _) in KEYS {
            let kept = old.field_mut(field).expect("every key is a field").clone();
            let slot = next.field_mut(field).expect("every key is a field");
            if slot.contains('•') {
                *slot = kept;
            }
        }
        next
    }
}

/// What a TV PC's media modules need (TMDB, Steam, YouTube, Spotify, Last.fm, Twitch, and the
/// Google client its Photos and YouTube sign-ins use), unmasked: TV PCs only, never a page.
pub fn for_tvs(h: &House) -> serde_json::Value {
    serde_json::json!({
        "tmdb_api_key": h.tmdb_api_key,
        "region": h.region,
        "language": h.language,
        "steam_api_key": h.steam_api_key,
        "steamgriddb_api_key": h.steamgriddb_api_key,
        "youtube_api_key": h.youtube_api_key,
        "spotify_client_id": h.spotify_client_id,
        "lastfm_api_key": h.lastfm_api_key,
        "twitch_client_id": h.twitch_client_id,
        "google_client_id": h.google_client_id,
        "google_client_secret": h.google_client_secret,
        "assistant_location": h.assistant_location,
    })
}

pub struct Settings {
    path: PathBuf,
    house: RwLock<House>,
}

impl Settings {
    pub fn open(dir: &Path) -> Result<Self> {
        let path = dir.join(FILE);
        let house = match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text)?,
            Err(_) => House::default(),
        };
        Ok(Self { path, house: RwLock::new(house) })
    }

    pub fn get(&self) -> House {
        self.house.read().clone()
    }

    pub fn save(&self, next: House) -> Result<()> {
        let merged = self.house.read().merged(next);
        write(&self.path, &merged)?;
        *self.house.write() = merged;
        Ok(())
    }
}

/// settings.toml through a temporary file, as `tvl_kit::store` does for JSON.
pub fn write(path: &Path, house: &House) -> Result<()> {
    let text = toml::to_string_pretty(house)?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_fill_only_empty_settings_and_reach_browsers_masked() {
        let mut h = House { tmdb_api_key: "from-settings-1234".into(), ..Default::default() };
        let env = vec![
            ("TMDB_API_KEY".to_string(), "from-env".to_string()),
            ("DEEPINFRA_API_KEY".to_string(), "  di-abcdefgh9876 ".to_string()),
            ("POSITIVO_PASSWORD".to_string(), "never".to_string()),
        ];
        assert_eq!(h.fill_from_env(&env), ["deepinfra_api_key"]);
        assert_eq!(h.tmdb_api_key, "from-settings-1234");
        assert_eq!(h.deepinfra_api_key, "di-abcdefgh9876");
        let m = h.masked();
        assert_eq!(m.deepinfra_api_key, "••••9876");
        assert_eq!(m.region, "BR");
        // Saving the masked form back changes nothing; a new value replaces.
        let mut edited = m.clone();
        edited.lastfm_api_key = "new-lastfm".into();
        let saved = h.merged(edited);
        assert_eq!(saved.deepinfra_api_key, "di-abcdefgh9876");
        assert_eq!(saved.lastfm_api_key, "new-lastfm");
    }

    #[test]
    fn the_old_launchers_settings_file_reads_and_its_tv_fields_are_left_out() {
        let old = "tmdb_api_key = \"\"\nregion = \"BR\"\nlanguage = \"pt-BR\"\nbrowser = \"edge\"\nhome_button = \"guide\"\nassistant_speech = \"local\"\nassistant_location = \"Porto Alegre\"\n";
        // Unknown (per-TV) fields are ignored here.
        let h: House = toml::from_str(old).unwrap();
        assert_eq!(h.assistant_location, "Porto Alegre");
        assert!(h.calendar_reminders);
        assert!(!toml::to_string(&h).unwrap().contains("home_button"));
    }
}
