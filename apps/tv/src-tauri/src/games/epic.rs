//! Epic Games: installed games from the launcher's manifest files. Epic has no official
//! library API, so games that aren't installed don't show up.

use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Item {
    pub display_name: String,
    pub app_name: String,
    pub catalog_namespace: String,
    pub catalog_item_id: String,
    pub launch_executable: String,
    #[serde(default)]
    pub app_categories: Vec<String>,
    #[serde(default, rename = "bIsIncompleteInstall")]
    pub incomplete: bool,
}

pub fn manifests_dir() -> PathBuf {
    let program_data = std::env::var_os("ProgramData").unwrap_or_else(|| r"C:\ProgramData".into());
    PathBuf::from(program_data).join(r"Epic\EpicGamesLauncher\Data\Manifests")
}

pub fn installed() -> Vec<Item> {
    let Ok(entries) = fs::read_dir(manifests_dir()) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("item")))
        .filter_map(|e| fs::read_to_string(e.path()).ok())
        .filter_map(|s| parse_item(&s))
        .collect()
}

/// A manifest for a fully installed game (not a plugin, engine or half-done install).
pub fn parse_item(src: &str) -> Option<Item> {
    let item: Item = serde_json::from_str(src.trim_start_matches('\u{feff}')).ok()?;
    let is_game = item.app_categories.iter().any(|c| c.eq_ignore_ascii_case("games"));
    (is_game && !item.incomplete).then_some(item)
}

pub fn launch_url(i: &Item) -> String {
    format!(
        "com.epicgames.launcher://apps/{}%3A{}%3A{}?action=launch&silent=true",
        i.catalog_namespace, i.catalog_item_id, i.app_name
    )
}

/// File name of the game's executable, used to tell whether it is running.
pub fn exe_name(i: &Item) -> Option<String> {
    PathBuf::from(&i.launch_executable)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ITEM: &str = r#"{
        "FormatVersion": 0,
        "bIsIncompleteInstall": false,
        "LaunchExecutable": "FortniteGame/Binaries/Win64/FortniteLauncher.exe",
        "DisplayName": "Fortnite",
        "InstallLocation": "C:\\Program Files\\Epic Games\\Fortnite",
        "CatalogNamespace": "fn",
        "CatalogItemId": "4fe75bbc5a674f4f9b356b5c90567da5",
        "AppName": "Fortnite",
        "AppCategories": ["public", "games", "applications"]
    }"#;

    #[test]
    fn parses_game_manifest() {
        let i = parse_item(ITEM).unwrap();
        assert_eq!(i.display_name, "Fortnite");
        assert_eq!(exe_name(&i).as_deref(), Some("FortniteLauncher.exe"));
        assert_eq!(
            launch_url(&i),
            "com.epicgames.launcher://apps/fn%3A4fe75bbc5a674f4f9b356b5c90567da5%3AFortnite?action=launch&silent=true"
        );
    }

    #[test]
    fn skips_plugins_and_incomplete_installs() {
        assert!(parse_item(&ITEM.replace(r#""games", "#, "")).is_none());
        assert!(parse_item(&ITEM.replace(r#""bIsIncompleteInstall": false"#, r#""bIsIncompleteInstall": true"#)).is_none());
        assert!(parse_item("not json").is_none());
    }
}
