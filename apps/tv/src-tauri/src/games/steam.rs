//! Steam: the install's local files (libraries, manifests, play history, art cache),
//! the Web API for the owned library, and steam:// URLs to launch.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::vdf;

const STEAMID64_BASE: u64 = 76_561_197_960_265_728;
const CDN: &str = "https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps";

/// Installed entries that are Steam tooling rather than games.
const TOOL_PREFIXES: [&str; 3] = ["Steamworks", "Proton", "Steam Linux Runtime"];

#[cfg(windows)]
pub fn root() -> Option<PathBuf> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let path: String = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Valve\Steam")
        .ok()?
        .get_value("SteamPath")
        .ok()?;
    Some(PathBuf::from(path)).filter(|p| p.is_dir())
}

#[cfg(not(windows))]
pub fn root() -> Option<PathBuf> {
    None
}

/// The game Steam reports as running, or 0.
#[cfg(windows)]
pub fn running_app() -> u32 {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Valve\Steam")
        .and_then(|k| k.get_value::<u32, _>("RunningAppID"))
        .unwrap_or(0)
}

#[cfg(not(windows))]
pub fn running_app() -> u32 {
    0
}

/// The account to show: the configured SteamID, else the most recent login on this PC.
pub fn account(root: &Path, configured: Option<u64>) -> Option<u64> {
    configured.or_else(|| parse_loginusers(&fs::read_to_string(root.join("config/loginusers.vdf")).ok()?))
}

pub fn parse_loginusers(src: &str) -> Option<u64> {
    let doc = vdf::parse(src);
    let users = doc.get("users")?.entries();
    let recent = users
        .iter()
        .find(|(_, u)| u.get_str("MostRecent") == Some("1"))
        .or_else(|| users.first())?;
    recent.0.parse().ok()
}

fn library_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![root.to_path_buf()];
    let src = fs::read_to_string(root.join("steamapps/libraryfolders.vdf")).unwrap_or_default();
    let doc = vdf::parse(&src);
    for (_, folder) in doc.get("libraryfolders").map(|f| f.entries()).unwrap_or_default() {
        if let Some(p) = folder.get_str("path").map(PathBuf::from) {
            if !dirs.iter().any(|d| same_dir(d, &p)) {
                dirs.push(p);
            }
        }
    }
    dirs
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase();
    norm(a) == norm(b)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Installed {
    pub appid: u32,
    pub name: String,
}

/// Fully installed games across every Steam library folder.
pub fn installed(root: &Path) -> Vec<Installed> {
    let mut out = Vec::new();
    for dir in library_dirs(root) {
        let Ok(entries) = fs::read_dir(dir.join("steamapps")) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            if let Some(app) = fs::read_to_string(e.path()).ok().and_then(|s| parse_manifest(&s)) {
                if !out.iter().any(|a: &Installed| a.appid == app.appid) {
                    out.push(app);
                }
            }
        }
    }
    out
}

/// An appmanifest that is a fully installed game (StateFlags has the "installed" bit 4).
pub fn parse_manifest(src: &str) -> Option<Installed> {
    let doc = vdf::parse(src);
    let state = doc.get("AppState")?;
    let appid = state.get_str("appid")?.parse().ok()?;
    let name = state.get_str("name")?.to_string();
    let flags: u32 = state.get_str("StateFlags")?.parse().ok()?;
    if flags & 4 == 0 || is_tool(&name) {
        return None;
    }
    Some(Installed { appid, name })
}

pub fn is_tool(name: &str) -> bool {
    TOOL_PREFIXES.iter().any(|p| name.starts_with(p))
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Played {
    pub minutes: u32,
    /// Unix seconds.
    pub last: i64,
}

/// Play time and last-played dates the Steam client keeps for this account.
pub fn local_played(root: &Path, steamid64: u64) -> HashMap<u32, Played> {
    let account = steamid64.saturating_sub(STEAMID64_BASE);
    let path = root.join(format!("userdata/{account}/config/localconfig.vdf"));
    fs::read_to_string(path)
        .map(|s| parse_localconfig(&s))
        .unwrap_or_default()
}

pub fn parse_localconfig(src: &str) -> HashMap<u32, Played> {
    let doc = vdf::parse(src);
    let apps = doc.path(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"]);
    let mut out = HashMap::new();
    for (id, app) in apps.map(|a| a.entries()).unwrap_or_default() {
        let Ok(id) = id.parse() else { continue };
        let num = |k| app.get_str(k).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
        let played = Played {
            minutes: num("Playtime") as u32,
            last: num("LastPlayed"),
        };
        if played != Played::default() {
            out.insert(id, played);
        }
    }
    out
}

/// Art the Steam client cached for an app: `librarycache/<appid>/[<hash>/]<file>`,
/// or the older flat `librarycache/<appid>_<file>`.
pub fn local_art(root: &Path, appid: u32, file: &str) -> Option<PathBuf> {
    let cache = root.join("appcache/librarycache");
    let dir = cache.join(appid.to_string());
    let direct = dir.join(file);
    if direct.is_file() {
        return Some(direct);
    }
    let nested = fs::read_dir(&dir).ok().into_iter().flatten().flatten().find_map(|e| {
        let p = e.path().join(file);
        p.is_file().then_some(p)
    });
    nested.or_else(|| {
        let flat = cache.join(format!("{appid}_{file}"));
        flat.is_file().then_some(flat)
    })
}

pub fn cdn(appid: u32, file: &str) -> String {
    format!("{CDN}/{appid}/{file}")
}

pub fn launch_url(appid: u32, installed: bool) -> String {
    if installed {
        format!("steam://rungameid/{appid}")
    } else {
        format!("steam://install/{appid}")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Owned {
    pub appid: u32,
    #[serde(default)]
    pub name: String,
    /// Minutes.
    #[serde(default)]
    pub playtime_forever: u32,
    /// Unix seconds.
    #[serde(default)]
    pub rtime_last_played: i64,
}

#[derive(Deserialize)]
struct OwnedResponse {
    response: OwnedGames,
}

#[derive(Deserialize)]
struct OwnedGames {
    #[serde(default)]
    games: Vec<Owned>,
}

/// Every game the account owns (the Web API key must belong to it, or its profile be public).
pub async fn fetch_owned(client: &reqwest::Client, key: &str, steamid64: u64) -> Result<Vec<Owned>> {
    let steamid = steamid64.to_string();
    let resp: OwnedResponse = client
        .get("https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/")
        .query(&[
            ("key", key),
            ("steamid", steamid.as_str()),
            ("include_appinfo", "1"),
            ("include_played_free_games", "1"),
            ("format", "json"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(resp.response.games)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_installed_vs_partial_vs_tool() {
        let m = |flags: &str, name: &str| {
            parse_manifest(&format!(
                "\"AppState\"\n{{\n\t\"appid\"\t\t\"1145360\"\n\t\"name\"\t\t\"{name}\"\n\t\"StateFlags\"\t\t\"{flags}\"\n}}"
            ))
        };
        assert_eq!(
            m("4", "Hades"),
            Some(Installed { appid: 1145360, name: "Hades".into() })
        );
        assert_eq!(m("1026", "Hades"), None); // downloading, not yet installed
        assert_eq!(m("4", "Steamworks Common Redistributables"), None);
    }

    #[test]
    fn loginusers_prefers_most_recent() {
        let src = "\"users\"\n{\n\"76561198000000001\"\n{\n\"PersonaName\"\t\"a\"\n\"MostRecent\"\t\"0\"\n}\n\"76561198284725586\"\n{\n\"PersonaName\"\t\"b\"\n\"MostRecent\"\t\"1\"\n}\n}";
        assert_eq!(parse_loginusers(src), Some(76561198284725586));
    }

    #[test]
    fn localconfig_play_history() {
        let src = "\"UserLocalConfigStore\"\n{\n\"Software\"\n{\n\"Valve\"\n{\n\"Steam\"\n{\n\"apps\"\n{\n\"730\"\n{\n\"LastPlayed\"\t\"1696808536\"\n\"Playtime\"\t\"238\"\n}\n\"7\"\n{\n\"cloud\"\n{\n\"last_sync_state\"\t\"synchronized\"\n}\n}\n}\n}\n}\n}\n}";
        let played = parse_localconfig(src);
        assert_eq!(played.len(), 1);
        assert_eq!(played[&730], Played { minutes: 238, last: 1696808536 });
    }

    #[test]
    fn owned_response_parses() {
        let r: OwnedResponse = serde_json::from_str(
            r#"{"response":{"game_count":1,"games":[{"appid":570,"name":"Dota 2","playtime_forever":61,"rtime_last_played":1700000000,"img_icon_url":"x"}]}}"#,
        )
        .unwrap();
        assert_eq!(r.response.games[0].appid, 570);
        let empty: OwnedResponse = serde_json::from_str(r#"{"response":{}}"#).unwrap();
        assert!(empty.response.games.is_empty());
    }

    #[test]
    fn urls() {
        assert_eq!(launch_url(570, true), "steam://rungameid/570");
        assert_eq!(launch_url(570, false), "steam://install/570");
        assert!(cdn(570, "library_600x900.jpg").ends_with("/570/library_600x900.jpg"));
    }
}
