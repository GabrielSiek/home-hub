//! Steam answers the Guide button by showing itself or opening Big Picture, even while
//! the launcher is in front. Steam owns its settings while it runs, so the launcher
//! can't turn that off; instead, after each Guide press it watches for a Steam window
//! taking the foreground and takes the screen back.
//!
//! Windows answers it too, with Game Bar (and the Xbox app, from Game Bar's controller
//! bar). That one is a registry switch, so the launcher turns it off while it runs.

use std::path::Path;

use tauri::AppHandle;

/// Game Bar's "Allow your controller to open Game Bar" setting; missing means on.
#[cfg(windows)]
const GAME_BAR_KEY: &str = r"Software\Microsoft\GameBar";
#[cfg(windows)]
const GAME_BAR_VALUE: &str = "UseNexusForGameBarEnabled";
/// The user's own setting, kept until it's put back so a crash doesn't make the
/// launcher's "off" look like theirs on the next start.
const SAVED_GAME_BAR: &str = "gamebar-guide.txt";
const UNSET: &str = "unset";

fn encode_saved(v: Option<u32>) -> String {
    v.map_or_else(|| UNSET.to_string(), |v| v.to_string())
}

/// `None` for a file that isn't ours to trust; `Some(None)` for "the value was missing".
fn decode_saved(s: &str) -> Option<Option<u32>> {
    match s.trim() {
        UNSET => Some(None),
        n => n.parse().ok().map(Some),
    }
}

/// Stops the Guide button from opening Game Bar until [`restore_game_bar`].
#[cfg(windows)]
pub fn disable_game_bar(data_dir: &Path) {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
    use winreg::RegKey;
    let saved = data_dir.join(SAVED_GAME_BAR);
    let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey_with_flags(GAME_BAR_KEY, KEY_READ | KEY_SET_VALUE)
    else {
        return;
    };
    if !saved.exists() {
        let current = key.get_value::<u32, _>(GAME_BAR_VALUE).ok();
        // Without a record of the user's setting there'd be nothing to put back.
        if std::fs::write(&saved, encode_saved(current)).is_err() {
            return;
        }
    }
    if let Err(e) = key.set_value(GAME_BAR_VALUE, &0u32) {
        eprintln!("couldn't keep Game Bar off the Guide button: {e}");
    }
}

#[cfg(not(windows))]
pub fn disable_game_bar(_data_dir: &Path) {}

/// Puts back the Game Bar setting [`disable_game_bar`] found.
#[cfg(windows)]
pub fn restore_game_bar(data_dir: &Path) {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;
    let saved = data_dir.join(SAVED_GAME_BAR);
    let Some(original) = std::fs::read_to_string(&saved).ok().and_then(|s| decode_saved(&s)) else {
        return;
    };
    let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(GAME_BAR_KEY, KEY_SET_VALUE) else {
        return;
    };
    let restored = match original {
        Some(v) => key.set_value(GAME_BAR_VALUE, &v),
        None => key.delete_value(GAME_BAR_VALUE),
    };
    if restored.is_ok() {
        let _ = std::fs::remove_file(saved);
    }
}

#[cfg(not(windows))]
pub fn restore_game_bar(_data_dir: &Path) {}

/// Big Picture can take a few seconds to come up.
#[cfg(windows)]
const WATCH_FOR: std::time::Duration = std::time::Duration::from_secs(4);
#[cfg(windows)]
const POLL: std::time::Duration = std::time::Duration::from_millis(50);

#[cfg(windows)]
static WATCHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Called on each Guide press; one watch runs at a time.
#[cfg(windows)]
pub fn after_guide(app: &AppHandle) {
    use std::sync::atomic::Ordering;
    if WATCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("guide-guard".into()).spawn(move || {
        watch(&app);
        WATCHING.store(false, Ordering::SeqCst);
    });
    if spawned.is_err() {
        WATCHING.store(false, Ordering::SeqCst);
    }
}

#[cfg(not(windows))]
pub fn after_guide(_app: &AppHandle) {}

#[cfg(windows)]
fn watch(app: &AppHandle) {
    use std::time::Instant;
    use tauri_plugin_opener::OpenerExt;

    // Big Picture the user already had open stays; only the one this press opened goes.
    let mut close_big_picture = !win::big_picture_open();
    let until = Instant::now() + WATCH_FOR;
    while Instant::now() < until {
        std::thread::sleep(POLL);
        let Some(fg) = win::steam_foreground() else { continue };
        if fg.big_picture {
            if std::mem::take(&mut close_big_picture) {
                let _ = app.opener().open_url("steam://close/bigpicture", None::<&str>);
            }
        } else {
            win::minimize(fg.hwnd);
        }
        crate::focus_win::bring_to_front(app);
    }
}

pub fn is_steam_exe(path: &str) -> bool {
    let name = path.rsplit(['\\', '/']).next().unwrap_or(path).to_ascii_lowercase();
    name == "steam.exe" || name == "steamwebhelper.exe"
}

pub fn is_big_picture(title: &str) -> bool {
    title.contains("Big Picture")
}

#[cfg(windows)]
mod win {
    use windows::core::{BOOL, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, ShowWindow,
        SW_MINIMIZE,
    };

    pub struct SteamWindow {
        pub hwnd: HWND,
        pub big_picture: bool,
    }

    /// The foreground window, if a Steam process owns it.
    pub fn steam_foreground() -> Option<SteamWindow> {
        // SAFETY: plain Win32 query; a null result is handled.
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_invalid() || !is_steam(hwnd) {
            return None;
        }
        Some(SteamWindow { hwnd, big_picture: super::is_big_picture(&title(hwnd)) })
    }

    pub fn big_picture_open() -> bool {
        unsafe extern "system" fn visit(hwnd: HWND, found: LPARAM) -> BOOL {
            // SAFETY: `found` points at the bool below, alive for the whole EnumWindows call.
            let found = unsafe { &mut *(found.0 as *mut bool) };
            if unsafe { IsWindowVisible(hwnd) }.as_bool() && super::is_big_picture(&title(hwnd)) && is_steam(hwnd) {
                *found = true;
                return false.into();
            }
            true.into()
        }
        let mut found = false;
        // SAFETY: the callback only touches `found` through the pointer passed here.
        let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut found as *mut bool as isize)) };
        found
    }

    pub fn minimize(hwnd: HWND) {
        // SAFETY: a window handle we just read; a stale one makes this a no-op.
        let _ = unsafe { ShowWindow(hwnd, SW_MINIMIZE) };
    }

    fn title(hwnd: HWND) -> String {
        let mut buf = [0u16; 256];
        // SAFETY: the buffer outlives the call, which writes at most its length.
        let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    fn is_steam(hwnd: HWND) -> bool {
        let mut pid = 0;
        // SAFETY: plain Win32 calls; the process handle is closed before returning.
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 {
                return false;
            }
            let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return false;
            };
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
            let _ = CloseHandle(process);
            ok && super::is_steam_exe(&String::from_utf16_lossy(&buf[..len as usize]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_processes_and_big_picture_titles() {
        assert!(is_steam_exe(r"C:\Program Files (x86)\Steam\steam.exe"));
        assert!(is_steam_exe(r"C:\Program Files (x86)\Steam\bin\cef\cef.win7x64\steamwebhelper.exe"));
        assert!(is_steam_exe("C:/Program Files (x86)/Steam/STEAM.EXE"));
        assert!(!is_steam_exe(r"C:\Games\steamgame.exe"));
        assert!(is_big_picture("Steam Big Picture Mode"));
        assert!(!is_big_picture("Steam"));
    }

    #[test]
    fn saved_game_bar_setting_round_trips() {
        for v in [None, Some(0), Some(1)] {
            assert_eq!(decode_saved(&encode_saved(v)), Some(v));
        }
        assert_eq!(decode_saved("1\r\n"), Some(Some(1)));
        assert_eq!(decode_saved(""), None);
        assert_eq!(decode_saved("yes"), None);
    }
}
