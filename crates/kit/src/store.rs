//! JSON files that survive a crash. The old launcher wrote each file in place and read a file it
//! couldn't parse as empty, so a write cut short (a power cut, OneDrive holding the file) lost that
//! file's data at the next save. Here a save goes to a temporary file that then replaces the old
//! one, and a file that won't parse is set aside and logged, never written over.
//!
//! Files keep the old launcher's shapes and names, so the import copies them as they are.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::Result;
use crate::time::now_ms;

/// Windows refuses a rename while another program (an antivirus, a backup, OneDrive) has the file
/// open: try again for a moment.
const RENAME_TRIES: u32 = 10;
const RENAME_WAIT: Duration = Duration::from_millis(30);

/// The value in `path`. None when there's no file, or it couldn't be read: a file that doesn't
/// parse is renamed `<name>.corrupt-<unix ms>` next to it and logged, so nothing writes over it.
pub fn load<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == ErrorKind::NotFound => return None,
        Err(e) => {
            tracing::error!(file = %path.display(), "can't read: {e}");
            return None;
        }
    };
    match serde_json::from_str(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            let aside = set_aside(path);
            tracing::error!(file = %path.display(), kept_as = %aside.display(), "doesn't parse, starting empty: {e}");
            None
        }
    }
}

fn set_aside(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let aside = path.with_file_name(format!("{name}.corrupt-{}", now_ms()));
    if let Err(e) = fs::rename(path, &aside) {
        tracing::error!(file = %path.display(), "can't set the unreadable file aside: {e}");
    }
    aside
}

/// Writes `value` to `path` through a temporary file in the same folder, flushed to disk, that then
/// replaces the old file. A reader sees the old file or the new one, never half of one.
pub fn save<T: Serialize + ?Sized>(path: &Path, value: &T, pretty: bool) -> Result<()> {
    let text = if pretty { serde_json::to_string_pretty(value)? } else { serde_json::to_string(value)? };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!("{name}.{}.tmp", std::process::id()));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    let mut tries = 0;
    loop {
        match fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) if tries < RENAME_TRIES && busy(&e) => {
                tries += 1;
                std::thread::sleep(RENAME_WAIT);
            }
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                return Err(e.into());
            }
        }
    }
}

/// Access denied (5) or sharing violation (32): another program has the file open.
fn busy(e: &std::io::Error) -> bool {
    matches!(e.raw_os_error(), Some(5) | Some(32))
}

/// A value kept in memory and in a JSON file, saved on every change.
pub struct JsonStore<T> {
    path: PathBuf,
    pretty: bool,
    value: Mutex<T>,
}

impl<T: Serialize + DeserializeOwned + Default + Clone> JsonStore<T> {
    /// Loads `path` (indented on disk, for files people may open), or starts from the default.
    pub fn open(path: impl Into<PathBuf>) -> Self {
        Self::with_format(path.into(), true)
    }

    /// The same, one line on disk: for large or frequently written files.
    pub fn open_compact(path: impl Into<PathBuf>) -> Self {
        Self::with_format(path.into(), false)
    }

    fn with_format(path: PathBuf, pretty: bool) -> Self {
        let value = load(&path).unwrap_or_default();
        Self { path, pretty, value: Mutex::new(value) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn get(&self) -> T {
        self.value.lock().clone()
    }

    pub fn read<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&self.value.lock())
    }

    /// Changes the value and saves it. Nothing changes when `f` fails or the file can't be
    /// written, so memory and disk always agree. Saves happen in the order the changes were made.
    pub fn change<R>(&self, f: impl FnOnce(&mut T) -> Result<R>) -> Result<R> {
        let mut value = self.value.lock();
        let mut next = value.clone();
        let out = f(&mut next)?;
        save(&self.path, &next, self.pretty)?;
        *value = next;
        Ok(out)
    }

    pub fn set(&self, next: T) -> Result<()> {
        self.change(|v| {
            *v = next;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
    struct Rooms {
        #[serde(default)]
        names: Vec<String>,
    }

    fn temp_dir(name: &str) -> PathBuf {
        crate::testing::temp_dir(name)
    }

    #[test]
    fn a_change_is_saved_and_read_back() {
        let dir = temp_dir("round-trip");
        let path = dir.join("rooms.json");
        let store: JsonStore<Rooms> = JsonStore::open(&path);
        assert_eq!(store.get(), Rooms::default());
        store.change(|r| {
            r.names.push("Quarto".into());
            Ok(())
        })
        .unwrap();
        let again: JsonStore<Rooms> = JsonStore::open(&path);
        assert_eq!(again.get().names, ["Quarto"]);
        // No temporary file left behind.
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_that_does_not_parse_is_set_aside_not_overwritten() {
        let dir = temp_dir("corrupt");
        let path = dir.join("scenes.json");
        // What a write cut short leaves behind.
        fs::write(&path, r#"{"names": ["Sala", "Quar"#).unwrap();
        let store: JsonStore<Rooms> = JsonStore::open(&path);
        assert_eq!(store.get(), Rooms::default());
        store.change(|r| {
            r.names.push("Cozinha".into());
            Ok(())
        })
        .unwrap();
        let kept: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("scenes.json.corrupt-"))
            .collect();
        assert_eq!(kept.len(), 1, "the unreadable file is kept for recovery");
        assert_eq!(fs::read_to_string(dir.join(&kept[0])).unwrap(), r#"{"names": ["Sala", "Quar"#);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_change_leaves_memory_and_disk_as_they_were() {
        let dir = temp_dir("failed");
        let path = dir.join("rooms.json");
        let store: JsonStore<Rooms> = JsonStore::open(&path);
        store.set(Rooms { names: vec!["Sala".into()] }).unwrap();
        let r: Result<()> = store.change(|r| {
            r.names.clear();
            Err(crate::Error::msg("nope"))
        });
        assert!(r.is_err());
        assert_eq!(store.get().names, ["Sala"]);
        assert_eq!(load::<Rooms>(&path).unwrap().names, ["Sala"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_file_is_none_and_an_empty_one_is_set_aside() {
        let dir = temp_dir("missing");
        assert!(load::<Rooms>(&dir.join("nothing.json")).is_none());
        fs::write(dir.join("empty.json"), "").unwrap();
        assert!(load::<Rooms>(&dir.join("empty.json")).is_none());
        assert!(!dir.join("empty.json").exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
