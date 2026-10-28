//! Title screens from thumbnails.libretro.com, matched by file name. The site's folder listing
//! is read once a week per console, so a ROM named "Super Mario 64 (U) [!]" still finds
//! "Super Mario 64 (USA).png".

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::scan::split_tags;
use super::systems::System;
use crate::error::Result;

const BASE: &str = "https://thumbnails.libretro.com";
const ART_DIR: &str = "retro-art";
const LISTING_AGE: Duration = Duration::from_secs(7 * 24 * 3600);
/// Title screens first (they fill a 4:3 tile like the game does); in-game shots when there's none.
pub const KINDS: [&str; 2] = ["Named_Titles", "Named_Snaps"];

/// Where a game's picture is (or will be) kept.
pub fn art_path(data_dir: &Path, system: &str, stem: &str) -> PathBuf {
    data_dir.join(ART_DIR).join(system).join(format!("{}.png", safe(stem)))
}

/// A game whose picture was looked for and doesn't exist.
pub fn none_marker(data_dir: &Path, system: &str, stem: &str) -> PathBuf {
    art_path(data_dir, system, stem).with_extension("none")
}

fn safe(s: &str) -> String {
    s.chars().map(|c| if r#"&*/:`<>?\|""#.contains(c) { '_' } else { c }).collect()
}

/// The names (without ".png") in one of the site's folders for `system`.
pub async fn listing(http: &reqwest::Client, data_dir: &Path, system: &System, kind: &str) -> Result<Vec<String>> {
    let cache = data_dir.join(ART_DIR).join(system.id).join(format!("{kind}.txt"));
    let fresh = fs::metadata(&cache)
        .and_then(|m| m.modified())
        .is_ok_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() < LISTING_AGE);
    if fresh {
        if let Ok(text) = fs::read_to_string(&cache) {
            return Ok(text.lines().map(String::from).collect());
        }
    }
    let url = format!("{BASE}/{}/{kind}/", urlencoding::encode(system.thumbs));
    let html = http.get(url).send().await?.error_for_status()?.text().await?;
    let names = parse_listing(&html);
    if let Some(dir) = cache.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(&cache, names.join("\n"));
    Ok(names)
}

/// The .png links of the site's folder page.
fn parse_listing(html: &str) -> Vec<String> {
    html.split("href=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .filter_map(|href| href.strip_suffix(".png"))
        .filter(|h| !h.contains('/'))
        .filter_map(|h| urlencoding::decode(h).ok().map(|s| s.replace("&amp;", "&")))
        .collect()
}

/// A title reduced to what tells it apart: "Legend of Zelda, The - Ocarina of Time" and
/// "The Legend of Zelda: Ocarina of Time" both become "thelegendofzeldaocarinaoftime".
pub fn key(name: &str) -> String {
    let (base, _) = split_tags(name);
    let mut s = base.to_lowercase().replace('_', " ");
    for art in ["the", "a", "an"] {
        let pat = format!(", {art}");
        if let Some(i) = s.find(&pat) {
            let rest = &s[i + pat.len()..];
            if rest.is_empty() || rest.starts_with(" -") || rest.starts_with(':') {
                s = format!("{art} {}{rest}", &s[..i]);
                break;
            }
        }
    }
    s.replace('&', "and").chars().filter(|c| c.is_alphanumeric()).collect()
}

/// How good a pick a listed name is: lower is better. Brazil's own releases come right after the
/// usual English ones; betas, demos and prototypes last.
fn rank(name: &str) -> u32 {
    let (_, tags) = split_tags(name);
    let tags = tags.join(" ").to_lowercase();
    let region = ["usa", "world", "brazil", "europe", "japan"]
        .iter()
        .position(|r| tags.contains(r))
        .unwrap_or(5) as u32;
    let rough = ["beta", "proto", "demo", "sample", "kiosk", "pirate", "unl"].iter().any(|w| tags.contains(w));
    region + if rough { 10 } else { 0 } + if tags.contains("rev") { 1 } else { 0 }
}

/// Index of a listing by `key`, each key's best name first.
pub fn index(names: &[String]) -> HashMap<String, String> {
    let mut by: HashMap<String, String> = HashMap::new();
    for n in names {
        let k = key(n);
        match by.get(&k) {
            Some(cur) if rank(cur) <= rank(n) => {}
            _ => {
                by.insert(k, n.clone());
            }
        }
    }
    by
}

/// The listed name for a ROM: the exact file name if it's there, else the best by title.
pub fn pick<'a>(stem: &str, names: &'a [String], index: &'a HashMap<String, String>) -> Option<&'a str> {
    let exact = safe(stem);
    if let Some(n) = names.iter().find(|n| **n == exact) {
        return Some(n);
    }
    index.get(&key(stem)).map(String::as_str)
}

pub async fn download(http: &reqwest::Client, system: &System, kind: &str, name: &str, to: &Path) -> Result<()> {
    let url = format!("{BASE}/{}/{kind}/{}.png", urlencoding::encode(system.thumbs), urlencoding::encode(name));
    let bytes = http.get(url).send().await?.error_for_status()?.bytes().await?;
    if let Some(dir) = to.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(to, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_links() {
        let html = r#"<a href="..">..</a><a href="Super%20Mario%2064%20(USA).png">x</a>
            <a href="Mario%20Kart%2064%20(Europe)%20(Rev%201).png">y</a><a href="/_h5ai/x.png">z</a>"#;
        assert_eq!(parse_listing(html), ["Super Mario 64 (USA)", "Mario Kart 64 (Europe) (Rev 1)"]);
    }

    #[test]
    fn matches_other_namings() {
        let names: Vec<String> = [
            "Legend of Zelda, The - Ocarina of Time (Europe) (En,Fr,De)",
            "Legend of Zelda, The - Ocarina of Time (USA)",
            "Legend of Zelda, The - Ocarina of Time (USA) (Beta)",
            "Super Mario 64 (USA)",
            "Sonic & Knuckles (World)",
        ]
        .map(String::from)
        .into();
        let idx = index(&names);
        assert_eq!(pick("The Legend of Zelda - Ocarina of Time (U) [!]", &names, &idx), Some("Legend of Zelda, The - Ocarina of Time (USA)"));
        assert_eq!(pick("Super Mario 64 (USA)", &names, &idx), Some("Super Mario 64 (USA)"));
        assert_eq!(pick("Sonic and Knuckles", &names, &idx), Some("Sonic & Knuckles (World)"));
        assert_eq!(pick("Paper Mario", &names, &idx), None);
    }

    /// Reads the real site: `cargo test real_thumbnails -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_thumbnails() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let http = reqwest::Client::new();
            let dir = std::env::temp_dir().join("tvl-thumbs");
            let sys = crate::retro::systems::get("n64").unwrap();
            let names = listing(&http, &dir, sys, KINDS[0]).await.unwrap();
            println!("{} titles", names.len());
            let idx = index(&names);
            let n = pick("Mario Kart 64 (U) [!]", &names, &idx).unwrap();
            println!("Mario Kart 64 → {n}");
            download(&http, sys, KINDS[0], n, &dir.join("mk64.png")).await.unwrap();
        });
    }
}
