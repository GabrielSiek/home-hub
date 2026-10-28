//! Game folders: the games in one console's folder, by file extension, and their names.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::systems::System;

#[derive(Debug, Clone, PartialEq)]
pub struct Rom {
    /// "retro:n64:Super Mario 64 (USA).z64": the file's path inside the folder.
    pub id: String,
    pub system: &'static str,
    pub path: PathBuf,
    /// The file name without its extension: what RetroArch names its save states after, and
    /// what libretro's thumbnails are matched to.
    pub stem: String,
    pub name: String,
}

/// How deep games may sit inside a console folder (a disc game in a folder of its own).
const DEPTH: usize = 2;

pub fn scan(system: &'static System, dir: &Path) -> Vec<Rom> {
    let mut files = Vec::new();
    walk(dir, 0, &mut files);
    // Discs listed in a playlist (.m3u) are played through it, not one by one.
    let in_playlists: HashSet<PathBuf> = files
        .iter()
        .filter(|p| ext(p).as_deref() == Some("m3u"))
        .flat_map(|m3u| playlist(m3u))
        .collect();
    let mut roms: Vec<Rom> = files
        .into_iter()
        .filter(|p| ext(p).is_some_and(|e| system.exts.contains(&e.as_str())))
        .filter(|p| !in_playlists.contains(&normalize(p)))
        .filter_map(|path| {
            let rel = path.strip_prefix(dir).ok()?.to_string_lossy().replace('\\', "/");
            let stem = path.file_stem()?.to_string_lossy().into_owned();
            Some(Rom { id: format!("retro:{}:{rel}", system.id), system: system.id, name: display_name(&stem), stem, path })
        })
        .collect();
    roms.sort_by_cached_key(|r| r.name.to_lowercase());
    roms
}

/// How many of `system`'s games `dir` holds (the folder picker's count).
pub fn count(system: &'static System, dir: &Path) -> usize {
    scan(system, dir).len()
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let hidden = e.file_name().to_string_lossy().starts_with('.');
        match e.file_type() {
            Ok(t) if t.is_dir() && depth < DEPTH && !hidden => walk(&path, depth + 1, out),
            Ok(t) if t.is_file() && !hidden => out.push(path),
            _ => {}
        }
    }
}

fn ext(p: &Path) -> Option<String> {
    p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase())
}

fn normalize(p: &Path) -> PathBuf {
    PathBuf::from(p.to_string_lossy().to_lowercase().replace('/', "\\"))
}

/// The discs an .m3u playlist lists, relative to it.
fn playlist(m3u: &Path) -> Vec<PathBuf> {
    let dir = m3u.parent().unwrap_or(Path::new(""));
    fs::read_to_string(m3u)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| normalize(&dir.join(l)))
        .collect()
}

/// The name to show for a file: "Legend of Zelda, The - Ocarina of Time (USA) (Rev 1)" →
/// "The Legend of Zelda: Ocarina of Time". A disc number stays.
pub fn display_name(stem: &str) -> String {
    let (base, tags) = split_tags(stem);
    let mut name = base.replace('_', " ");
    // No-Intro / Redump move a leading article to the end of the first part.
    for art in ["The", "A", "An"] {
        let pat = format!(", {art}");
        if let Some(i) = name.find(&pat) {
            let after = &name[i + pat.len()..];
            if after.is_empty() || after.starts_with(" -") || after.starts_with(':') {
                name = format!("{art} {}{after}", &name[..i]);
                break;
            }
        }
    }
    let mut name = name.replace(" - ", ": ").split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(disc) = tags.iter().find(|t| t.to_lowercase().starts_with("disc ")) {
        name.push_str(&format!(" ({disc})"));
    }
    if name.is_empty() { stem.to_string() } else { name }
}

/// The title before any "(…)" or "[…]" tag, and the tags.
pub fn split_tags(stem: &str) -> (String, Vec<String>) {
    let mut base = String::new();
    let mut tags = Vec::new();
    let mut depth = 0;
    let mut tag = String::new();
    let mut in_tags = false;
    for c in stem.chars() {
        match c {
            '(' | '[' => {
                depth += 1;
                in_tags = true;
                if depth > 1 {
                    tag.push(c);
                }
            }
            ')' | ']' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    tags.push(std::mem::take(&mut tag).trim().to_string());
                } else {
                    tag.push(c);
                }
            }
            _ if depth > 0 => tag.push(c),
            _ if !in_tags => base.push(c),
            _ => {}
        }
    }
    (base.trim().to_string(), tags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retro::systems;

    #[test]
    fn names() {
        assert_eq!(display_name("Legend of Zelda, The - Ocarina of Time (USA) (Rev 1)"), "The Legend of Zelda: Ocarina of Time");
        assert_eq!(display_name("Super Mario 64 (USA)"), "Super Mario 64");
        assert_eq!(display_name("Final Fantasy VII (USA) (Disc 2)"), "Final Fantasy VII (Disc 2)");
        assert_eq!(display_name("Sonic The Hedgehog 2 (World) [!]"), "Sonic The Hedgehog 2");
        assert_eq!(display_name("mario_kart_64"), "mario kart 64");
        assert_eq!(display_name("Legend of Zelda, The (USA)"), "The Legend of Zelda");
    }

    #[test]
    fn scans_by_extension_and_playlists() {
        let dir = std::env::temp_dir().join(format!("tvl-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("Final Fantasy VII")).unwrap();
        for f in ["Crash Bandicoot 2 (USA).chd", "Spyro (USA).cue", "Spyro (USA).bin", "readme.txt"] {
            fs::write(dir.join(f), b"x").unwrap();
        }
        let ff = dir.join("Final Fantasy VII");
        for f in ["FF7 (Disc 1).chd", "FF7 (Disc 2).chd"] {
            fs::write(ff.join(f), b"x").unwrap();
        }
        fs::write(ff.join("Final Fantasy VII (USA).m3u"), "FF7 (Disc 1).chd\nFF7 (Disc 2).chd\n").unwrap();

        let roms = scan(systems::get("ps1").unwrap(), &dir);
        let names: Vec<_> = roms.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Crash Bandicoot 2", "Final Fantasy VII", "Spyro"]);
        assert_eq!(roms[1].id, "retro:ps1:Final Fantasy VII/Final Fantasy VII (USA).m3u");
        let _ = fs::remove_dir_all(&dir);
    }
}
