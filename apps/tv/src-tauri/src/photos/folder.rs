//! Photos in a folder on this PC, e.g. a Google Drive for desktop or Google Takeout folder.

use std::fs;
use std::path::{Path, PathBuf};

/// What the webview can show. HEIC isn't here: WebView2 can't decode it.
const EXTENSIONS: [&str; 6] = ["jpg", "jpeg", "png", "webp", "avif", "gif"];
const MAX_DEPTH: usize = 8;
const MAX_PHOTOS: usize = 20_000;

/// The photos under `dir`, subfolders included. No dates: a file's is usually when it was
/// copied or downloaded, not when the photo was taken.
pub fn scan(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(dir, 0, &mut out);
    out
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        if out.len() >= MAX_PHOTOS {
            return;
        }
        let name = e.file_name().to_string_lossy().to_lowercase();
        // Hidden folders, the recycle bin, thumbnails caches.
        if name.starts_with('.') || name.starts_with('$') {
            continue;
        }
        let Ok(kind) = e.file_type() else { continue };
        let path = e.path();
        if kind.is_dir() {
            if depth < MAX_DEPTH {
                walk(&path, depth + 1, out);
            }
        } else if is_photo(&name) {
            out.push(path);
        }
    }
}

fn is_photo(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| EXTENSIONS.contains(&ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_photos_in_subfolders_only() {
        let dir = std::env::temp_dir().join(format!("tvl-photos-{}", std::process::id()));
        fs::create_dir_all(dir.join("2024/trip")).unwrap();
        fs::create_dir_all(dir.join(".thumbs")).unwrap();
        for f in ["a.JPG", "2024/b.webp", "2024/trip/c.png", "notes.txt", "d.heic", ".thumbs/e.jpg"] {
            fs::write(dir.join(f), b"x").unwrap();
        }
        let mut found: Vec<String> = scan(&dir)
            .into_iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        found.sort();
        assert_eq!(found, ["a.JPG", "b.webp", "c.png"]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
