//! Brings the old launcher's household data over: `tvl-server import`. The old data folder
//! (%APPDATA%\dev.tvlauncher.app) is only ever read: every file read is hashed before and after,
//! and the import stops if one changed meanwhile (the old launcher still running). Nothing is
//! moved or deleted there.
//!
//! The old data lives on the TV PC, not on the server laptop, so it can go in two steps: a bundle
//! made on the TV PC (`--out bundle.zip`), unpacked on the server (`--bundle bundle.zip`).
//!
//! Each TV PC copies its own files (games, emulators, watch progress, streaming sign-ins) when its
//! new desktop app first starts; those aren't the server's.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tvl_kit::{Error, Result};

use crate::settings::{self, House};

/// Household files the server takes as they are, under the same names.
const AS_IS: &[&str] = &[
    "smarthome-local.json",
    "smarthome-ir.json",
    "smarthome-layout.json",
    "companion.json",
    "clock.json",
    "scenes.json",
    "automations.json",
    "calendar.json",
    "calendar-prefs.json",
    "calendar-events.json",
    "calendar-places.json",
    "companion-ca.pem",
    "companion-ca.key",
];

/// Household files left behind, and why.
const LEFT: &[(&str, &str)] = &[
    ("ewelink.json", "the eWeLink cloud account isn't used on the server"),
    ("tuya-specs.json", "the Tuya cloud account isn't used on the server"),
];

/// A file the server takes, as it will be written.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// What was gathered, and what was noticed on the way (for the person running the import).
pub struct Gathered {
    pub items: Vec<Item>,
    pub notes: Vec<String>,
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Every source file's size, hash and time, to prove nothing changed while it was read.
fn manifest(paths: &[PathBuf]) -> BTreeMap<PathBuf, (u64, String, Option<std::time::SystemTime>)> {
    paths
        .iter()
        .filter_map(|p| {
            let bytes = std::fs::read(p).ok()?;
            let modified = std::fs::metadata(p).ok().and_then(|m| m.modified().ok());
            Some((p.clone(), (bytes.len() as u64, sha256(&bytes), modified)))
        })
        .collect()
}

/// The `.env` lines as variable → value (comments and blanks skipped).
fn read_env(path: &Path) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for item in dotenvy::from_path_iter(path).map_err(|e| Error::msg(format!("{}: {e}", path.display())))? {
        let (k, v) = item.map_err(|e| Error::msg(format!("{}: {e}", path.display())))?;
        out.push((k, v));
    }
    Ok(out)
}

/// The old rooms.json split in two: its tasks stay (rooms.json), its screens go to devices.json.
/// The old fixed "tv" becomes the slot the first TV PC to pair takes over (its name and room).
fn convert_rooms(text: &str) -> Result<(Item, Item, Option<String>)> {
    let v: Value = serde_json::from_str(text)?;
    let rooms = json!({ "tasks": v.get("tasks").cloned().unwrap_or_else(|| json!([])) });
    let mut screens = Vec::new();
    let mut unclaimed = None;
    for s in v.get("screens").and_then(Value::as_array).into_iter().flatten() {
        if s["id"] == "tv" || s["kind"] == "tv" {
            unclaimed = Some(json!({ "name": s["name"].as_str().unwrap_or("TV"), "room": s.get("room").cloned().unwrap_or(Value::Null) }));
        } else {
            screens.push(s.clone());
        }
    }
    let note = unclaimed.as_ref().map(|u| match u["room"].as_str() {
        Some(room) => format!("the old TV (in {room}) is waiting for the first TV PC that pairs"),
        None => "the old TV is waiting for the first TV PC that pairs".to_string(),
    });
    let devices = json!({ "screens": screens, "unclaimed_tv": unclaimed });
    Ok((
        Item { name: "rooms.json".into(), bytes: serde_json::to_vec_pretty(&rooms)? },
        Item { name: crate::devices::FILE.into(), bytes: serde_json::to_vec_pretty(&devices)? },
        note,
    ))
}

/// Reads the household's files from the old data folder `from`, and its keys from `.env` files
/// (`env`, and `from\.env` if there is one).
pub fn gather(from: &Path, env: Option<&Path>) -> Result<Gathered> {
    if !from.is_dir() {
        return Err(Error::msg(format!("No data folder at {}", from.display())));
    }
    let mut envs: Vec<PathBuf> = env.map(Path::to_path_buf).into_iter().collect();
    if from.join(".env").is_file() {
        envs.push(from.join(".env"));
    }
    let mut sources: Vec<PathBuf> = AS_IS.iter().chain(["rooms.json", settings::FILE].iter()).map(|n| from.join(n)).collect();
    sources.extend(envs.iter().cloned());
    let before = manifest(&sources);

    let mut items = Vec::new();
    let mut notes = Vec::new();
    for name in AS_IS {
        let Ok(bytes) = std::fs::read(from.join(name)) else { continue };
        if name.ends_with(".json") {
            if let Err(e) = serde_json::from_slice::<Value>(&bytes) {
                notes.push(format!("{name} left behind: it doesn't parse ({e})"));
                continue;
            }
        }
        items.push(Item { name: name.to_string(), bytes });
    }
    if let Ok(text) = std::fs::read_to_string(from.join("rooms.json")) {
        let (rooms, devices, note) = convert_rooms(&text).map_err(|e| Error::msg(format!("rooms.json: {e}")))?;
        items.extend([rooms, devices]);
        notes.extend(note);
    }
    let mut house: House = match std::fs::read_to_string(from.join(settings::FILE)) {
        Ok(text) => toml::from_str(&text).map_err(|e| Error::msg(format!("{}: {e}", settings::FILE)))?,
        Err(_) => House::default(),
    };
    for path in &envs {
        let vars = read_env(path)?;
        let filled = house.fill_from_env(&vars);
        if !filled.is_empty() {
            notes.push(format!("keys from {}: {}", path.display(), filled.join(", ")));
        }
        let unused: Vec<&str> = vars.iter().map(|(k, _)| k.as_str()).filter(|k| k.starts_with("POSITIVO_")).collect();
        if !unused.is_empty() {
            notes.push(format!("{} in {} not copied: nothing uses them (consider deleting them)", unused.join(", "), path.display()));
        }
    }
    items.push(Item { name: settings::FILE.into(), bytes: toml::to_string_pretty(&house)?.into_bytes() });
    for (name, why) in LEFT {
        if from.join(name).is_file() {
            notes.push(format!("{name} left behind: {why}"));
        }
    }

    if manifest(&sources) != before {
        return Err(Error::msg("The old launcher's files changed while they were read: close it and import again"));
    }
    Ok(Gathered { items, notes })
}

/// What happened to one file.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Written,
    Unchanged,
    /// Already there with other contents; `--force` replaces it.
    Kept,
}

/// Writes `items` into the server's data folder `dir`. A file already there is kept unless
/// `force`, so running the import twice changes nothing. `dry_run` only says what would happen.
pub fn write_into(dir: &Path, items: &[Item], force: bool, dry_run: bool) -> Result<Vec<(String, Outcome)>> {
    if !dry_run {
        std::fs::create_dir_all(dir)?;
    }
    let mut out = Vec::new();
    for item in items {
        let target = dir.join(&item.name);
        let outcome = match std::fs::read(&target) {
            Ok(existing) if existing == item.bytes => Outcome::Unchanged,
            Ok(_) if !force => Outcome::Kept,
            _ => {
                if !dry_run {
                    let tmp = target.with_file_name(format!("{}.import.tmp", item.name));
                    std::fs::File::create(&tmp).and_then(|mut f| f.write_all(&item.bytes).and_then(|_| f.sync_all()))?;
                    std::fs::rename(&tmp, &target)?;
                }
                Outcome::Written
            }
        };
        out.push((item.name.clone(), outcome));
    }
    Ok(out)
}

/// Packs `items` into a zip to carry to the server.
pub fn bundle(items: &[Item], out: &Path) -> Result<()> {
    let file = std::fs::File::create(out)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    for item in items {
        zip.start_file(&item.name, options).map_err(|e| Error::msg(e.to_string()))?;
        zip.write_all(&item.bytes)?;
    }
    zip.finish().map_err(|e| Error::msg(e.to_string()))?;
    Ok(())
}

/// The files of a bundle made with `bundle`. Only plain names: nothing in a bundle can land
/// outside the data folder.
pub fn unbundle(path: &Path) -> Result<Vec<Item>> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?).map_err(|e| Error::msg(e.to_string()))?;
    let mut items = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| Error::msg(e.to_string()))?;
        let name = f.name().map_err(|e| Error::msg(e.to_string()))?.to_string();
        let plain = !name.is_empty() && !name.contains(['/', '\\', ':']) && name != "." && name != "..";
        if !plain || !(AS_IS.contains(&name.as_str()) || ["rooms.json", crate::devices::FILE, settings::FILE].contains(&name.as_str())) {
            return Err(Error::msg(format!("Not an import bundle: {name}")));
        }
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes)?;
        items.push(Item { name, bytes });
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An old data folder like the house's: the TV in Quarto, a tablet, a lamp layout, keys in .env.
    fn old_folder() -> (PathBuf, PathBuf) {
        let dir = tvl_kit::testing::temp_dir("import-old");
        std::fs::write(
            dir.join("rooms.json"),
            r#"{"tasks":[{"id":"t1","room":"Quarto","title":"Water plants","created_at":1}],
               "screens":[{"id":"tv","name":"TV","kind":"tv","room":"Quarto","last_seen":0},
                          {"id":"s1","name":"iPad","kind":"tablet","room":"Sala","last_seen":5}]}"#,
        )
        .unwrap();
        std::fs::write(dir.join("smarthome-layout.json"), r#"{"names":{"local:tuya:a":"Abajur"}}"#).unwrap();
        std::fs::write(dir.join("clock.json"), r#"{"timers":[],"alarms":[]}"#).unwrap();
        std::fs::write(dir.join("companion-ca.pem"), "-----BEGIN CERTIFICATE-----\nx\n").unwrap();
        std::fs::write(dir.join("settings.toml"), "region = \"BR\"\nbrowser = \"edge\"\nassistant_location = \"Porto Alegre\"\n").unwrap();
        std::fs::write(dir.join("watch.json"), "{}").unwrap();
        std::fs::write(dir.join("ewelink.json"), "{}").unwrap();
        let env = dir.join("repo.env");
        std::fs::write(&env, "# keys\nTMDB_API_KEY=tmdb-123456789\nPOSITIVO_EMAIL=me@example.com\nPOSITIVO_PASSWORD=hunter2\n").unwrap();
        (dir, env)
    }

    fn hashes(dir: &Path) -> BTreeMap<String, String> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), sha256(&std::fs::read(&p).unwrap())))
            .collect()
    }

    #[test]
    fn the_household_comes_over_and_the_old_folder_is_untouched() {
        let (old, env) = old_folder();
        let before = hashes(&old);
        let g = gather(&old, Some(&env)).unwrap();
        let names: Vec<&str> = g.items.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"smarthome-layout.json") && names.contains(&"companion-ca.pem"));
        assert!(!names.contains(&"watch.json"), "a TV PC's own file stays with it");
        assert!(!names.contains(&"ewelink.json"));
        assert!(g.notes.iter().any(|n| n.contains("POSITIVO_EMAIL, POSITIVO_PASSWORD")));
        assert!(g.notes.iter().any(|n| n.contains("old TV (in Quarto)")));

        let server = tvl_kit::testing::temp_dir("import-server");
        let report = write_into(&server, &g.items, false, false).unwrap();
        assert!(report.iter().all(|(_, o)| *o == Outcome::Written));
        assert_eq!(hashes(&old), before, "nothing in the old folder changed");

        // The keys came from .env; the TV's own settings didn't come at all.
        let house: House = toml::from_str(&std::fs::read_to_string(server.join("settings.toml")).unwrap()).unwrap();
        assert_eq!(house.tmdb_api_key, "tmdb-123456789");
        assert_eq!(house.assistant_location, "Porto Alegre");
        let text = std::fs::read_to_string(server.join("settings.toml")).unwrap();
        assert!(!text.contains("hunter2") && !text.contains("browser"));

        // The screens moved to devices.json, the old TV waiting for its PC.
        let devices = crate::devices::Devices::open(
            &server,
            std::sync::Arc::new(tvl_kit::time::FakeClock::new(1)),
            crate::bus::Bus::new(),
        );
        assert_eq!(devices.get("s1").unwrap().room.as_deref(), Some("Sala"));
        let tv = devices.add_tv("pc-1", "DESKTOP-1").unwrap();
        assert_eq!((tv.name.as_str(), tv.room.as_deref()), ("TV", Some("Quarto")));
        let rooms: Value = serde_json::from_slice(&std::fs::read(server.join("rooms.json")).unwrap()).unwrap();
        assert_eq!(rooms["tasks"][0]["title"], "Water plants");
        assert!(rooms.get("screens").is_none());
    }

    #[test]
    fn importing_twice_changes_nothing_and_existing_files_need_force() {
        let (old, env) = old_folder();
        let server = tvl_kit::testing::temp_dir("import-twice");
        let g = gather(&old, Some(&env)).unwrap();
        write_into(&server, &g.items, false, false).unwrap();
        let again = write_into(&server, &g.items, false, false).unwrap();
        assert!(again.iter().all(|(_, o)| *o == Outcome::Unchanged));

        std::fs::write(server.join("clock.json"), r#"{"timers":[1]}"#).unwrap();
        let kept = write_into(&server, &g.items, false, false).unwrap();
        assert!(kept.contains(&("clock.json".to_string(), Outcome::Kept)));
        assert_eq!(std::fs::read_to_string(server.join("clock.json")).unwrap(), r#"{"timers":[1]}"#);
        // A dry run says what --force would do, and does nothing.
        let dry = write_into(&server, &g.items, true, true).unwrap();
        assert!(dry.contains(&("clock.json".to_string(), Outcome::Written)));
        assert_eq!(std::fs::read_to_string(server.join("clock.json")).unwrap(), r#"{"timers":[1]}"#);
    }

    #[test]
    fn an_unreadable_json_file_is_left_behind() {
        let (old, _env) = old_folder();
        std::fs::write(old.join("scenes.json"), "[{\"id\":").unwrap();
        let g = gather(&old, None).unwrap();
        assert!(!g.items.iter().any(|i| i.name == "scenes.json"));
        assert!(g.notes.iter().any(|n| n.starts_with("scenes.json left behind")));
    }

    #[test]
    fn a_bundle_carries_the_same_files_and_nothing_else() {
        let (old, env) = old_folder();
        let g = gather(&old, Some(&env)).unwrap();
        let zip_path = tvl_kit::testing::temp_dir("import-bundle").join("bundle.zip");
        bundle(&g.items, &zip_path).unwrap();
        assert_eq!(unbundle(&zip_path).unwrap(), g.items);

        // A zip with a path in a name, or a file the import doesn't make, is refused.
        let bad = tvl_kit::testing::temp_dir("import-bad").join("bad.zip");
        bundle(&[Item { name: "../evil.json".into(), bytes: b"{}".to_vec() }], &bad).unwrap();
        assert!(unbundle(&bad).is_err());
        bundle(&[Item { name: "admin.json".into(), bytes: b"{}".to_vec() }], &bad).unwrap();
        assert!(unbundle(&bad).is_err());
    }
}
