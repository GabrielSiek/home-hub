//! Save states, as each emulator writes them, for the library's "Resume" and its state strip.
//!
//! RetroArch names them after the game's file ("Game.state.auto", "Game.state2", each with a
//! ".png" beside it). PCSX2 and Dolphin name them after the disc's serial, which the launcher
//! doesn't read: it learns it instead from the files a game's session wrote (`learn`).

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

use super::emu::{Emu, DOLPHIN_SLOT};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SaveState {
    /// "Auto-save", "State 2".
    pub label: String,
    /// None: the one written on the way out, which Resume loads.
    pub slot: Option<u32>,
    /// Unix seconds.
    pub at: i64,
    /// Its screenshot, if the emulator keeps one.
    pub shot: Option<String>,
    #[serde(skip)]
    pub path: PathBuf,
}

/// Every file in the states folder and one level down (RetroArch can sort states by core).
pub fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else { return out };
    for e in entries.flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_file() => out.push(p),
            Ok(t) if t.is_dir() => {
                if let Ok(inner) = fs::read_dir(&p) {
                    out.extend(inner.flatten().map(|e| e.path()).filter(|p| p.is_file()));
                }
            }
            _ => {}
        }
    }
    out
}

fn modified(p: &Path) -> Option<SystemTime> {
    fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn unix(t: SystemTime) -> i64 {
    t.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn label(slot: Option<u32>) -> String {
    slot.map_or_else(|| "Auto-save".to_string(), |n| format!("State {n}"))
}

/// The states of the game whose file is `stem` (RetroArch), newest first.
pub fn retroarch(all: &[PathBuf], stem: &str) -> Vec<SaveState> {
    let prefix = format!("{stem}.state");
    let mut out: Vec<SaveState> = all
        .iter()
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().into_owned();
            let rest = name.strip_prefix(&prefix)?;
            let slot = match rest {
                ".auto" => None,
                "" => Some(0),
                n => Some(n.parse::<u32>().ok()?),
            };
            let shot = PathBuf::from(format!("{}.png", p.display()));
            Some(SaveState {
                label: label(slot),
                slot,
                at: unix(modified(p)?),
                shot: shot.is_file().then(|| shot.to_string_lossy().into_owned()),
                path: p.clone(),
            })
        })
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.at));
    out
}

/// PCSX2's states for a learned "SLUS-20946 (A9B3F0A1)": its resume state and numbered slots.
/// The screenshot inside each (a .p2s is a zip) is copied out to `shots`.
pub fn pcsx2(all: &[PathBuf], prefix: &str, shots: &Path) -> Vec<SaveState> {
    let mut out: Vec<SaveState> = all
        .iter()
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().into_owned();
            let rest = name.strip_prefix(prefix)?.strip_prefix('.')?.strip_suffix(".p2s")?;
            let slot = if rest == "resume" { None } else { Some(rest.parse::<u32>().ok()?) };
            let at = modified(p)?;
            Some(SaveState { label: label(slot), slot, at: unix(at), shot: p2s_shot(p, at, shots), path: p.clone() })
        })
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.at));
    out
}

fn p2s_shot(state: &Path, at: SystemTime, shots: &Path) -> Option<String> {
    let name = state.file_name()?.to_string_lossy().replace(['(', ')', ' '], "_");
    let out = shots.join(format!("{name}-{}.png", unix(at)));
    if out.is_file() {
        return Some(out.to_string_lossy().into_owned());
    }
    let mut zip = zip::ZipArchive::new(fs::File::open(state).ok()?).ok()?;
    let mut entry = zip.by_name("Screenshot.png").ok()?;
    let mut png = Vec::new();
    entry.read_to_end(&mut png).ok()?;
    fs::create_dir_all(shots).ok()?;
    // Older copies of the same state's picture go.
    if let Ok(old) = fs::read_dir(shots) {
        for o in old.flatten() {
            if o.file_name().to_string_lossy().starts_with(&format!("{name}-")) {
                let _ = fs::remove_file(o.path());
            }
        }
    }
    fs::write(&out, png).ok()?;
    Some(out.to_string_lossy().into_owned())
}

/// Dolphin's states for a learned game id ("GALE01"): slots ".s01"–".s10". The launcher saves
/// to slot 8 on the way out, so that one is the auto-save.
pub fn dolphin(all: &[PathBuf], prefix: &str) -> Vec<SaveState> {
    let mut out: Vec<SaveState> = all
        .iter()
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().into_owned();
            let n: u32 = name.strip_prefix(prefix)?.strip_prefix(".s")?.parse().ok()?;
            let slot = (n != DOLPHIN_SLOT).then_some(n);
            Some(SaveState { label: label(slot), slot, at: unix(modified(p)?), shot: None, path: p.clone() })
        })
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.at));
    out
}

/// What names the states a session of a PCSX2 or Dolphin game wrote, after it ends: the serial
/// part of the newest state file changed since `since`.
pub fn learn(emu: Emu, all: &[PathBuf], since: SystemTime) -> Option<String> {
    let serial = |p: &PathBuf| -> Option<String> {
        let name = p.file_name()?.to_string_lossy().into_owned();
        match emu {
            // "SLUS-20946 (A9B3F0A1).resume.p2s" → "SLUS-20946 (A9B3F0A1)"
            Emu::Pcsx2 => name.strip_suffix(".p2s").and_then(|n| n.rsplit_once('.')).map(|(p, _)| p.to_string()),
            // "GALE01.s08" → "GALE01"
            Emu::Dolphin => name.rsplit_once(".s").filter(|(_, n)| n.parse::<u32>().is_ok()).map(|(p, _)| p.to_string()),
            Emu::Retroarch => None,
        }
    };
    all.iter()
        .filter_map(|p| Some((modified(p)?, serial(p)?)))
        .filter(|(t, _)| *t >= since)
        .max_by_key(|(t, _)| *t)
        .map(|(_, s)| s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, b"x").unwrap();
        p
    }

    #[test]
    fn retroarch_states_and_shots() {
        let dir = std::env::temp_dir().join(format!("tvl-states-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("mupen64plus_next")).unwrap();
        let sub = dir.join("mupen64plus_next");
        touch(&sub, "Mario Kart 64 (USA).state.auto");
        touch(&sub, "Mario Kart 64 (USA).state.auto.png");
        touch(&dir, "Mario Kart 64 (USA).state2");
        touch(&dir, "Mario Kart 64 (USA).state");
        touch(&dir, "Mario Kart 64 (USA) (Beta).state");
        let all = files(&dir);
        let mut got = retroarch(&all, "Mario Kart 64 (USA)");
        got.sort_by_key(|s| s.slot.map_or(-1, |n| n as i64));
        assert_eq!(got.iter().map(|s| s.slot).collect::<Vec<_>>(), [None, Some(0), Some(2)]);
        assert_eq!(got[0].label, "Auto-save");
        assert!(got[0].shot.as_deref().is_some_and(|s| s.ends_with(".state.auto.png")));
        assert!(got[2].shot.is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn learns_serials() {
        let dir = std::env::temp_dir().join(format!("tvl-learn-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let since = SystemTime::now() - std::time::Duration::from_secs(5);
        touch(&dir, "SLUS-20946 (A9B3F0A1).resume.p2s");
        assert_eq!(learn(Emu::Pcsx2, &files(&dir), since).as_deref(), Some("SLUS-20946 (A9B3F0A1)"));
        touch(&dir, "GALE01.s08");
        let all = files(&dir);
        assert_eq!(learn(Emu::Dolphin, &all, since).as_deref(), Some("GALE01"));
        assert_eq!(dolphin(&all, "GALE01")[0].label, "Auto-save");
        assert_eq!(pcsx2(&all, "SLUS-20946 (A9B3F0A1)", &dir)[0].slot, None);
        assert!(learn(Emu::Pcsx2, &all, SystemTime::now() + std::time::Duration::from_secs(60)).is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
