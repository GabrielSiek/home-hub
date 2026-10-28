//! The Windows calls the retro shelf needs: program versions, known folders, drives, and the
//! emulator's windows (to close it the way its own close button would, and to give it focus).

use std::path::PathBuf;

pub struct Drive {
    /// "D:\"
    pub root: String,
    pub label: String,
    pub free: Option<u64>,
    pub removable: bool,
}

#[cfg(windows)]
mod imp {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};

    use windows::core::{BOOL, GUID, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetFileVersionInfoSizeW, GetFileVersionInfoW, GetLogicalDrives,
        GetVolumeInformationW, VerQueryValueW,
    };
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{SHGetKnownFolderPath, FOLDERID_Documents, FOLDERID_Downloads, KF_FLAG_DEFAULT};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, SetForegroundWindow, WM_CLOSE,
    };

    use super::Drive;

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    fn from_wide(buf: &[u16]) -> String {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        OsString::from_wide(&buf[..end]).to_string_lossy().into_owned()
    }

    pub fn file_version(exe: &Path) -> Option<String> {
        let path: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: `path` is NUL-terminated and outlives the calls; `data` is sized as Windows asks,
        // and the pointers VerQueryValueW hands back point into it.
        unsafe {
            let size = GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None);
            if size == 0 {
                return None;
            }
            let mut data = vec![0u8; size as usize];
            GetFileVersionInfoW(PCWSTR(path.as_ptr()), None, size, data.as_mut_ptr().cast()).ok()?;
            // The string version first: Dolphin's is "2609a", which the numbers can't say.
            let mut ptr = std::ptr::null_mut();
            let mut len = 0u32;
            let translation = wide("\\VarFileInfo\\Translation");
            let mut lang = (0x0409u16, 0x04b0u16);
            if VerQueryValueW(data.as_ptr().cast(), PCWSTR(translation.as_ptr()), &mut ptr, &mut len).as_bool() && len >= 4 {
                let pair = ptr as *const u16;
                lang = (*pair, *pair.add(1));
            }
            for key in ["ProductVersion", "FileVersion"] {
                let sub = wide(&format!("\\StringFileInfo\\{:04x}{:04x}\\{key}", lang.0, lang.1));
                if VerQueryValueW(data.as_ptr().cast(), PCWSTR(sub.as_ptr()), &mut ptr, &mut len).as_bool() && len > 0 {
                    let s = from_wide(std::slice::from_raw_parts(ptr as *const u16, len as usize));
                    if !s.trim().is_empty() {
                        return Some(s);
                    }
                }
            }
            let root = wide("\\");
            if VerQueryValueW(data.as_ptr().cast(), PCWSTR(root.as_ptr()), &mut ptr, &mut len).as_bool() && len >= 16 {
                // VS_FIXEDFILEINFO: dwProductVersionMS / LS at offsets 16 and 20.
                let words = ptr as *const u32;
                let (ms, ls) = (*words.add(4), *words.add(5));
                return Some(format!("{}.{}.{}.{}", ms >> 16, ms & 0xffff, ls >> 16, ls & 0xffff));
            }
            None
        }
    }

    fn known(id: &GUID) -> Option<PathBuf> {
        // SAFETY: the returned string is ours to free, and is read before it is.
        unsafe {
            let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const _));
            s.map(PathBuf::from)
        }
    }

    pub fn documents() -> Option<PathBuf> {
        known(&FOLDERID_Documents)
    }

    pub fn downloads() -> Option<PathBuf> {
        known(&FOLDERID_Downloads)
    }

    pub fn dolphin_user_path() -> Option<PathBuf> {
        use winreg::enums::HKEY_CURRENT_USER;
        let key = winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Dolphin Emulator").ok()?;
        let path: String = key.get_value("UserConfigPath").ok()?;
        Some(PathBuf::from(path)).filter(|p| p.is_dir())
    }

    pub fn drives() -> Vec<Drive> {
        // SAFETY: plain queries; the buffers outlive each call.
        let mask = unsafe { GetLogicalDrives() };
        (0..26)
            .filter(|i| mask & (1 << i) != 0)
            .filter_map(|i| {
                let root = format!("{}:\\", (b'A' + i as u8) as char);
                let w = wide(&root);
                let kind = unsafe { GetDriveTypeW(PCWSTR(w.as_ptr())) };
                // 2 removable, 3 fixed, 4 network; skip CD drives (5) and unknowns.
                if !matches!(kind, 2..=4) {
                    return None;
                }
                let mut label = [0u16; 261];
                let _ = unsafe { GetVolumeInformationW(PCWSTR(w.as_ptr()), Some(&mut label), None, None, None, None) };
                let mut free = 0u64;
                let free = unsafe { GetDiskFreeSpaceExW(PCWSTR(w.as_ptr()), Some(&mut free), None, None) }
                    .ok()
                    .map(|_| free);
                Some(Drive { root, label: from_wide(&label), free, removable: kind == 2 })
            })
            .collect()
    }

    /// The shown top-level windows of process `pid`.
    pub fn windows_of(pid: u32) -> Vec<isize> {
        struct Search {
            pid: u32,
            found: Vec<isize>,
        }
        unsafe extern "system" fn visit(hwnd: HWND, search: LPARAM) -> BOOL {
            // SAFETY: `search` points at the Search below, alive for the whole EnumWindows call.
            let search = unsafe { &mut *(search.0 as *mut Search) };
            let mut owner = 0;
            // SAFETY: plain Win32 queries on a window EnumWindows just handed us.
            let ours = unsafe {
                GetWindowThreadProcessId(hwnd, Some(&mut owner));
                owner == search.pid && IsWindowVisible(hwnd).as_bool()
            };
            if ours {
                search.found.push(hwnd.0 as isize);
            }
            true.into()
        }
        let mut search = Search { pid, found: Vec::new() };
        // SAFETY: the callback only touches `search` through the pointer passed here.
        let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut search as *mut Search as isize)) };
        search.found
    }

    /// Asks each of the process's windows to close, as its title bar's X would.
    pub fn close_windows(pid: u32) -> bool {
        let windows = windows_of(pid);
        for &w in &windows {
            // SAFETY: a window handle we just read; a stale one makes this fail harmlessly.
            let _ = unsafe { PostMessageW(Some(HWND(w as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        }
        !windows.is_empty()
    }

    pub fn focus(pid: u32) -> bool {
        windows_of(pid).first().is_some_and(|&w| {
            // SAFETY: as above.
            unsafe { SetForegroundWindow(HWND(w as *mut _)) }.as_bool()
        })
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::{Path, PathBuf};

    use super::Drive;

    pub fn file_version(_exe: &Path) -> Option<String> {
        None
    }
    pub fn documents() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Documents"))
    }
    pub fn downloads() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Downloads"))
    }
    pub fn dolphin_user_path() -> Option<PathBuf> {
        None
    }
    pub fn drives() -> Vec<Drive> {
        vec![Drive { root: "/".into(), label: String::new(), free: None, removable: false }]
    }
    pub fn windows_of(_pid: u32) -> Vec<isize> {
        Vec::new()
    }
    pub fn close_windows(_pid: u32) -> bool {
        false
    }
    pub fn focus(_pid: u32) -> bool {
        false
    }
}

pub use imp::{close_windows, documents, dolphin_user_path, downloads, drives, file_version, focus};

const PREFERRED_ROOT: &str = r"C:\TVLauncher\emulators";

fn fallback_root() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("TVLauncher").join("emulators"))
}

/// Where the launcher puts the emulators it installs: C:\TVLauncher\emulators, as the design
/// has it, or under %LOCALAPPDATA% when that can't be made. Makes the folder.
pub fn install_root() -> PathBuf {
    let preferred = PathBuf::from(PREFERRED_ROOT);
    if std::fs::create_dir_all(&preferred).is_ok() {
        return preferred;
    }
    fallback_root().unwrap_or(preferred)
}

/// The same, without making it: for the cards, before anything is installed.
pub fn install_root_shown() -> PathBuf {
    let preferred = PathBuf::from(PREFERRED_ROOT);
    if preferred.is_dir() || fallback_root().is_none_or(|f| !f.is_dir()) {
        preferred
    } else {
        fallback_root().unwrap_or(preferred)
    }
}
