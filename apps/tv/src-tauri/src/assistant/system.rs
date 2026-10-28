//! Windows bits the assistant drives: the master volume, media keys and Start menu programs.

use std::path::{Path, PathBuf};

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Volume {
    /// 0–100.
    pub level: u8,
    pub muted: bool,
}

/// Reads the speakers' volume, lets `change` edit it, and applies what changed.
#[cfg(windows)]
pub fn volume(change: impl FnOnce(&mut Volume)) -> Result<Volume> {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

    let win = |e: windows::core::Error| Error::msg(format!("Volume: {}", e.message()));
    // SAFETY: plain Core Audio calls; COM is set up on this thread first (a thread that
    // already has it keeps its own mode, which works as well).
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(win)?;
        let speakers = devices.GetDefaultAudioEndpoint(eRender, eConsole).map_err(win)?;
        let endpoint: IAudioEndpointVolume = speakers.Activate(CLSCTX_ALL, None).map_err(win)?;
        let before = Volume {
            level: (endpoint.GetMasterVolumeLevelScalar().map_err(win)? * 100.0).round() as u8,
            muted: endpoint.GetMute().map_err(win)?.as_bool(),
        };
        let mut after = before;
        change(&mut after);
        after.level = after.level.min(100);
        if after.level != before.level {
            endpoint
                .SetMasterVolumeLevelScalar(f32::from(after.level) / 100.0, std::ptr::null())
                .map_err(win)?;
        }
        if after.muted != before.muted {
            endpoint.SetMute(after.muted, std::ptr::null()).map_err(win)?;
        }
        Ok(after)
    }
}

#[cfg(not(windows))]
pub fn volume(_change: impl FnOnce(&mut Volume)) -> Result<Volume> {
    Err(Error::msg("Volume control works on Windows only"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKey {
    PlayPause,
    Next,
    Previous,
}

/// A media key: Windows hands it to whatever is playing (the browser, Spotify…).
pub fn media_key(m: MediaKey) -> Result<()> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| Error::msg(e.to_string()))?;
    let key = match m {
        MediaKey::PlayPause => Key::MediaPlayPause,
        MediaKey::Next => Key::MediaNextTrack,
        MediaKey::Previous => Key::MediaPrevTrack,
    };
    enigo.key(key, Direction::Click).map_err(|e| Error::msg(e.to_string()))
}

/// A program's Start menu shortcut.
#[derive(Debug, Clone)]
pub struct Program {
    pub name: String,
    pub path: PathBuf,
}

/// Every program in the Start menu, for the user and for all users.
pub fn programs() -> Vec<Program> {
    let mut out = Vec::new();
    for var in ["APPDATA", "ProgramData"] {
        if let Some(base) = std::env::var_os(var) {
            let dir = Path::new(&base).join(r"Microsoft\Windows\Start Menu\Programs");
            collect(&dir, 0, &mut out);
        }
    }
    out
}

/// Shortcuts that open documents or uninstallers rather than the program.
const NOT_PROGRAMS: [&str; 6] = ["uninstall", "desinstalar", "readme", "help", "website", "manual"];

fn collect(dir: &Path, depth: u8, out: &mut Vec<Program>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            if depth < 3 {
                collect(&path, depth + 1, out);
            }
            continue;
        }
        let ext = path.extension().and_then(|x| x.to_str()).unwrap_or_default();
        if !ext.eq_ignore_ascii_case("lnk") && !ext.eq_ignore_ascii_case("url") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let lower = name.to_lowercase();
        if NOT_PROGRAMS.iter().any(|w| lower.contains(w)) {
            continue;
        }
        out.push(Program {
            name: name.to_string(),
            path,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the real volume without changing it: `cargo test -- --ignored real_volume`.
    #[test]
    #[ignore]
    fn real_volume() {
        let v = volume(|_| {}).unwrap();
        assert!(v.level <= 100);
        println!("volume {v:?}, {} Start menu programs", programs().len());
    }
}
