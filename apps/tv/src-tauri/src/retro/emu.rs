//! The three emulators: finding them in a folder, where each keeps its save states, BIOS and
//! settings, and the TV settings the launcher gives each one.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::win;
use crate::config::HomeButton;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Emu {
    Retroarch,
    Pcsx2,
    Dolphin,
}

impl Emu {
    pub const ALL: [Emu; 3] = [Emu::Retroarch, Emu::Pcsx2, Emu::Dolphin];

    pub fn name(self) -> &'static str {
        match self {
            Emu::Retroarch => "RetroArch",
            Emu::Pcsx2 => "PCSX2",
            Emu::Dolphin => "Dolphin",
        }
    }

    pub fn exe(self) -> &'static str {
        match self {
            Emu::Retroarch => "retroarch.exe",
            Emu::Pcsx2 => "pcsx2-qt.exe",
            Emu::Dolphin => "Dolphin.exe",
        }
    }

    /// Where "Install for me" downloads it from, for the card's footer.
    pub fn source(self) -> &'static str {
        match self {
            Emu::Retroarch => "buildbot.libretro.com · stable",
            Emu::Pcsx2 => "GitHub releases · PCSX2/pcsx2",
            Emu::Dolphin => "dolphin-emu.org · release builds",
        }
    }
}

/// The emulator program in `dir`, or in a folder right inside it (an archive's own top folder).
pub fn find_exe(dir: &Path, emu: Emu) -> Option<PathBuf> {
    let direct = exe_in(dir, emu);
    if direct.is_some() {
        return direct;
    }
    fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .find_map(|e| exe_in(&e.path(), emu))
}

fn exe_in(dir: &Path, emu: Emu) -> Option<PathBuf> {
    fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.is_file() && p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(emu.exe()))
    })
}

/// Which of the three emulators `dir` holds, with its program.
pub fn identify(dir: &Path) -> Option<(Emu, PathBuf)> {
    Emu::ALL.into_iter().find_map(|e| find_exe(dir, e).map(|exe| (e, exe)))
}

/// The program's version: "1.22.2", "2.8.2", "2609a". From its file properties (PCSX2), else
/// from the version text built into it (RetroArch and Dolphin have no file version).
pub fn version(emu: Emu, exe: &Path) -> Option<String> {
    let v = win::file_version(exe)
        .map(|v| v.trim().trim_start_matches(['v', 'V']).to_string())
        .filter(|v| !v.is_empty() && v != "0.0.0.0")
        .or_else(|| built_in_version(emu, exe))?;
    // "1.22.2.0" → "1.22.2"; Dolphin's "2609a" and "5.0-21088" stay as they are.
    Some(match v.strip_suffix(".0") {
        Some(short) if short.matches('.').count() >= 2 => short.to_string(),
        _ => v,
    })
}

/// The version after one of the markers the program carries ("RetroArch-WIMP/1.22.2",
/// "Dolphin 2609a"). Dolphin also quotes old "5.0-…" versions; its own is a year-month one
/// ("2609a", or "2609-127" for a dev build) since 2407, so that kind wins.
fn built_in_version(emu: Emu, exe: &Path) -> Option<String> {
    let bytes = fs::read(exe).ok()?;
    let markers: &[&[u8]] = match emu {
        Emu::Retroarch => &[b"RetroArch-WIMP/", b"[INFO] Version: "],
        Emu::Dolphin => &[b"Dolphin "],
        Emu::Pcsx2 => &[b"PCSX2 v"],
    };
    let mut found = Vec::new();
    for m in markers {
        let mut from = 0;
        while let Some(i) = bytes[from..].windows(m.len()).position(|w| w == *m) {
            let start = from + i + m.len();
            let v: String = bytes[start..]
                .iter()
                .take(16)
                .take_while(|b| b.is_ascii_alphanumeric() || **b == b'.' || **b == b'-')
                .map(|&b| b as char)
                .collect();
            if v.len() >= 3 && v.starts_with(|c: char| c.is_ascii_digit()) {
                found.push(v);
            }
            from = start;
        }
    }
    let year_month = |v: &&String| v.len() >= 4 && v[..4].chars().all(|c| c.is_ascii_digit()) && !v[4..].starts_with('.');
    match emu {
        Emu::Dolphin => found.iter().find(year_month).or(found.first()).cloned(),
        _ => found.into_iter().next(),
    }
}

// ---------- Where each one keeps things ----------

/// The folders the launcher reads from an emulator.
#[derive(Debug, Clone, PartialEq)]
pub struct Dirs {
    pub states: PathBuf,
    /// RetroArch: the system folder (PlayStation BIOS); PCSX2: its bios folder.
    pub bios: Option<PathBuf>,
    pub cores: Option<PathBuf>,
    /// The settings in use: RetroArch's retroarch.cfg, PCSX2's data folder, Dolphin's user folder.
    pub settings: PathBuf,
}

pub fn dirs(emu: Emu, root: &Path) -> Dirs {
    match emu {
        Emu::Retroarch => {
            let cfg = root.join("retroarch.cfg");
            let text = fs::read_to_string(&cfg).unwrap_or_default();
            let dir = |key: &str, default: &str| {
                cfg_get(&text, key)
                    .and_then(|v| ra_path(root, &v))
                    .unwrap_or_else(|| root.join(default))
            };
            Dirs {
                states: dir("savestate_directory", "states"),
                bios: Some(dir("system_directory", "system")),
                cores: Some(dir("libretro_directory", "cores")),
                settings: cfg,
            }
        }
        Emu::Pcsx2 => {
            let data = pcsx2_data(root);
            let ini = fs::read_to_string(data.join("inis").join("PCSX2.ini")).unwrap_or_default();
            let dir = |key: &str, default: &str| {
                let v = ini_get(&ini, "Folders", key).filter(|v| !v.is_empty()).unwrap_or_else(|| default.into());
                let p = PathBuf::from(&v);
                if p.is_absolute() { p } else { data.join(p) }
            };
            Dirs { states: dir("Savestates", "sstates"), bios: Some(dir("Bios", "bios")), cores: None, settings: data }
        }
        Emu::Dolphin => {
            let user = dolphin_user(root);
            Dirs { states: user.join("StateSaves"), bios: None, cores: None, settings: user }
        }
    }
}

/// RetroArch writes folders as ":\states" (next to retroarch.exe) or "default".
fn ra_path(root: &Path, v: &str) -> Option<PathBuf> {
    let v = v.trim();
    if v.is_empty() || v == "default" {
        return None;
    }
    match v.strip_prefix(":\\").or_else(|| v.strip_prefix(":/")) {
        Some(rel) => Some(root.join(rel)),
        None => Some(PathBuf::from(v)),
    }
}

fn is_portable_pcsx2(root: &Path) -> bool {
    root.join("portable.ini").exists() || root.join("portable.txt").exists()
}

/// PCSX2's data: next to the program in portable mode, else Documents\PCSX2.
pub fn pcsx2_data(root: &Path) -> PathBuf {
    if is_portable_pcsx2(root) {
        return root.to_path_buf();
    }
    win::documents().map(|d| d.join("PCSX2")).unwrap_or_else(|| root.to_path_buf())
}

/// Dolphin's user folder: User next to the program with portable.txt, else where the
/// registry points, else Documents\Dolphin Emulator, else %APPDATA%\Dolphin Emulator.
pub fn dolphin_user(root: &Path) -> PathBuf {
    if root.join("portable.txt").exists() {
        return root.join("User");
    }
    if let Some(p) = win::dolphin_user_path() {
        return p;
    }
    let docs = win::documents().map(|d| d.join("Dolphin Emulator"));
    if let Some(d) = docs.as_ref().filter(|d| d.is_dir()) {
        return d.clone();
    }
    std::env::var_os("APPDATA")
        .map(|a| PathBuf::from(a).join("Dolphin Emulator"))
        .or(docs)
        .unwrap_or_else(|| root.join("User"))
}

/// Where a linked emulator's settings are, in words for the card: "Yours, from Documents\PCSX2".
pub fn settings_label(emu: Emu, root: &Path) -> String {
    let d = dirs(emu, root);
    let at = match emu {
        Emu::Retroarch => return "Yours, from retroarch.cfg in this folder".into(),
        _ => d.settings,
    };
    if at.starts_with(root) {
        return "Yours, kept in this folder".into();
    }
    let home = win::documents().and_then(|d| d.parent().map(Path::to_path_buf));
    let shown = home
        .and_then(|h| at.strip_prefix(&h).ok().map(|p| p.display().to_string()))
        .unwrap_or_else(|| at.display().to_string());
    format!("Yours, from {shown}")
}

// ---------- Small config file readers ----------

/// A `key = "value"` line from retroarch.cfg.
pub fn cfg_get(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
    })
}

/// `key = value` in `[section]` of an ini file.
pub fn ini_get(text: &str, section: &str, key: &str) -> Option<String> {
    let mut inside = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            inside = t.trim_matches(['[', ']']).eq_ignore_ascii_case(section);
        } else if inside {
            if let Some((k, v)) = t.split_once('=') {
                if k.trim().eq_ignore_ascii_case(key) {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    None
}

/// Sets `key = value` in `[section]`, keeping every other line as it was.
pub fn ini_set(text: &str, section: &str, key: &str, value: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut inside = false;
    let mut seen_section = false;
    let mut done = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            if inside && !done {
                // The section ends without the key: add it at its end.
                while out.last().is_some_and(|l| l.trim().is_empty()) {
                    out.pop();
                }
                out.push(format!("{key} = {value}"));
                out.push(String::new());
                done = true;
            }
            inside = t.trim_matches(['[', ']']).eq_ignore_ascii_case(section);
            seen_section |= inside;
        } else if inside && !done {
            if let Some((k, _)) = t.split_once('=') {
                if k.trim().eq_ignore_ascii_case(key) {
                    out.push(format!("{key} = {value}"));
                    done = true;
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }
    if !done {
        if !seen_section {
            if out.last().is_some_and(|l| !l.trim().is_empty()) {
                out.push(String::new());
            }
            out.push(format!("[{section}]"));
        }
        out.push(format!("{key} = {value}"));
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

// ---------- TV settings ----------

/// What every emulator is set to, as the Emulators screen lists it.
pub const TV_SETTINGS: &[&str] = &[
    "Fullscreen on the TV",
    "Auto save & load state",
    "SELECT + START → launcher",
    "Controller handed to the game",
    "8BitDo mapped",
];

/// RetroArch's binds for the launcher's session, added on top of the user's own settings with
/// `--appendconfig` (so a linked copy's retroarch.cfg is never changed).
///
/// The pad: RetroArch's XInput profiles open its menu on the Guide button (`input_menu_toggle_btn
/// = "10"`), which is the launcher's Home by default. A bind left empty falls back to that
/// profile, so it is pointed at a button no pad has instead; L3 + R3 opens the menu then.
/// SELECT + START quits by itself, saving first, and the launcher sends QUIT over the network
/// command port for Home.
pub fn retroarch_append(home: HomeButton, resume: bool, linked: bool, monitor: Option<u32>) -> String {
    let mut lines = vec![
        ("video_fullscreen", "true".to_string()),
        ("savestate_auto_save", "true".into()),
        ("savestate_auto_load", resume.to_string()),
        ("savestate_thumbnail_enable", "true".into()),
        ("network_cmd_enable", "true".into()),
        ("network_cmd_port", super::play::RETROARCH_PORT.to_string()),
        // 4 = START + SELECT; quitting asks nothing.
        ("input_quit_gamepad_combo", "4".into()),
        ("quit_press_twice", "false".into()),
        ("pause_nonactive", "false".into()),
        ("notification_show_autoconfig", "false".into()),
    ];
    if home == HomeButton::Guide {
        lines.push(("input_menu_toggle_btn", "99".into()));
        // 2 = L3 + R3.
        lines.push(("input_menu_toggle_gamepad_combo", "2".into()));
    }
    // L3 or R3 as Home: they can't open the menu as well; L2 + R2 (10) does.
    if matches!(home, HomeButton::L3 | HomeButton::R3) {
        lines.push(("input_menu_toggle_gamepad_combo", "10".into()));
    }
    if linked {
        // Otherwise RetroArch would save these into the user's own retroarch.cfg on exit.
        lines.push(("config_save_on_exit", "false".into()));
    }
    if let Some(m) = monitor {
        lines.push(("video_monitor_index", m.to_string()));
    }
    let mut s = String::from("# Written by TV Launcher for each game it starts.\n");
    for (k, v) in lines {
        s.push_str(&format!("{k} = \"{v}\"\n"));
    }
    s
}

/// PCSX2's TV settings, set in its PCSX2.ini (also a linked copy's: PCSX2 has no way to take
/// them for one session). Without them, closing a game asks "Are you sure?" in a dialog the pad
/// can't answer, and a newer version is offered in another.
pub const PCSX2_TV_KEYS: &[(&str, &str, &str)] = &[
    ("UI", "ConfirmShutdown", "false"),
    ("UI", "SetupWizardIncomplete", "false"),
    ("UI", "HideMouseCursor", "true"),
    ("EmuCore", "SaveStateOnShutdown", "true"),
    ("AutoUpdater", "CheckAtStartup", "false"),
];

/// A fresh PCSX2.ini for the launcher's own copy: the TV settings plus the 8BitDo (XInput) as
/// the first controller. Guide is left unbound (the launcher's Home), and SELECT + START go to
/// the game as Select and Start.
pub fn pcsx2_ini() -> String {
    let mut s = String::from("[UI]\nSettingsVersion = 1\n");
    for (section, key, value) in PCSX2_TV_KEYS {
        s = ini_set(&s, section, key, value);
    }
    s = ini_set(&s, "Folders", "Bios", "bios");
    s = ini_set(&s, "Folders", "Savestates", "sstates");
    s = ini_set(&s, "InputSources", "XInput", "true");
    s = ini_set(&s, "InputSources", "SDL", "false");
    let pad = [
        ("Type", "DualShock2"),
        ("Up", "XInput-0/DPadUp"),
        ("Right", "XInput-0/DPadRight"),
        ("Down", "XInput-0/DPadDown"),
        ("Left", "XInput-0/DPadLeft"),
        ("Triangle", "XInput-0/Y"),
        ("Circle", "XInput-0/B"),
        ("Cross", "XInput-0/A"),
        ("Square", "XInput-0/X"),
        ("Select", "XInput-0/Back"),
        ("Start", "XInput-0/Start"),
        ("L1", "XInput-0/LeftShoulder"),
        ("L2", "XInput-0/+LeftTrigger"),
        ("R1", "XInput-0/RightShoulder"),
        ("R2", "XInput-0/+RightTrigger"),
        ("L3", "XInput-0/LeftStick"),
        ("R3", "XInput-0/RightStick"),
        // XInput's Y axes are flipped to SDL's way in PCSX2: up is negative.
        ("LUp", "XInput-0/-LeftY"),
        ("LRight", "XInput-0/+LeftX"),
        ("LDown", "XInput-0/+LeftY"),
        ("LLeft", "XInput-0/-LeftX"),
        ("RUp", "XInput-0/-RightY"),
        ("RRight", "XInput-0/+RightX"),
        ("RDown", "XInput-0/+RightY"),
        ("RLeft", "XInput-0/-RightX"),
        ("LargeMotor", "XInput-0/LargeMotor"),
        ("SmallMotor", "XInput-0/SmallMotor"),
    ];
    for (k, v) in pad {
        s = ini_set(&s, "Pad1", k, v);
    }
    s
}

/// Sets PCSX2's TV settings in an existing PCSX2.ini, saving a copy of it first (once).
pub fn pcsx2_tv(root: &Path) -> std::io::Result<()> {
    let ini = pcsx2_data(root).join("inis").join("PCSX2.ini");
    let Ok(text) = fs::read_to_string(&ini) else {
        // Never started: PCSX2 writes its defaults on the first run, and the next launch adds these.
        return Ok(());
    };
    let mut new = text.clone();
    for (section, key, value) in PCSX2_TV_KEYS {
        new = ini_set(&new, section, key, value);
    }
    if new == text {
        return Ok(());
    }
    let backup = ini.with_extension("ini.before-tv-launcher");
    if !backup.exists() {
        fs::copy(&ini, &backup)?;
    }
    fs::write(&ini, new)
}

/// Dolphin's GameCube controller 1 on the 8BitDo (XInput 0), for the launcher's own copy.
/// Guide stays unbound: it is the launcher's Home.
pub const DOLPHIN_GCPAD: &str = "[GCPad1]
Device = XInput/0/Gamepad
Buttons/A = `Button A`
Buttons/B = `Button X`
Buttons/X = `Button B`
Buttons/Y = `Button Y`
Buttons/Z = `Shoulder R`
Buttons/Start = Start
Main Stick/Up = `Left Y+`
Main Stick/Down = `Left Y-`
Main Stick/Left = `Left X-`
Main Stick/Right = `Left X+`
Main Stick/Modifier = `Thumb L`
C-Stick/Up = `Right Y+`
C-Stick/Down = `Right Y-`
C-Stick/Left = `Right X-`
C-Stick/Right = `Right X+`
Triggers/L = `Trigger L`
Triggers/R = `Trigger R`
Triggers/L-Analog = `Trigger L`
Triggers/R-Analog = `Trigger R`
D-Pad/Up = `Pad N`
D-Pad/Down = `Pad S`
D-Pad/Left = `Pad W`
D-Pad/Right = `Pad E`
Rumble/Motor = `Motor L` | `Motor R`
";

/// Dolphin's TV settings, for this session only (`-C`), so nothing in a linked copy changes.
pub fn dolphin_config(window: Option<(i32, i32)>) -> Vec<String> {
    let mut c = vec![
        "Dolphin.Display.Fullscreen=True",
        "Dolphin.Interface.ConfirmStop=False",
        "Dolphin.Interface.ShowActiveTitle=False",
        // The launcher's cover is in front while Dolphin starts; the pad works either way.
        "Dolphin.Input.BackgroundInput=True",
        "Dolphin.Analytics.PermissionAsked=True",
        "Dolphin.AutoUpdate.UpdateTrack=",
    ]
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();
    if let Some((x, y)) = window {
        c.push(format!("Dolphin.Display.RenderWindowXPos={x}"));
        c.push(format!("Dolphin.Display.RenderWindowYPos={y}"));
    }
    c
}

/// The slot the launcher saves Dolphin's state to on the way out (Shift + F8 by default).
pub const DOLPHIN_SLOT: u32 = 8;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_set_replaces_adds_and_keeps_the_rest() {
        let text = "[UI]\nConfirmShutdown = true\nTheme = Dark\n\n[EmuCore]\nCdvdPrecache = false\n";
        let t = ini_set(text, "UI", "ConfirmShutdown", "false");
        assert_eq!(ini_get(&t, "UI", "ConfirmShutdown").as_deref(), Some("false"));
        assert_eq!(ini_get(&t, "UI", "Theme").as_deref(), Some("Dark"));
        let t = ini_set(&t, "EmuCore", "SaveStateOnShutdown", "true");
        assert_eq!(ini_get(&t, "EmuCore", "SaveStateOnShutdown").as_deref(), Some("true"));
        assert_eq!(ini_get(&t, "EmuCore", "CdvdPrecache").as_deref(), Some("false"));
        let t = ini_set(&t, "AutoUpdater", "CheckAtStartup", "false");
        assert!(t.contains("\n[AutoUpdater]\nCheckAtStartup = false\n"));
        // A key in another section with the same name is left alone.
        let t2 = ini_set("[A]\nK = 1\n[B]\nK = 2\n", "B", "K", "3");
        assert_eq!(ini_get(&t2, "A", "K").as_deref(), Some("1"));
        assert_eq!(ini_get(&t2, "B", "K").as_deref(), Some("3"));
    }

    #[test]
    fn retroarch_paths() {
        let root = Path::new(r"C:\RA");
        assert_eq!(ra_path(root, r":\states"), Some(root.join("states")));
        assert_eq!(ra_path(root, "default"), None);
        assert_eq!(ra_path(root, r"D:\saves"), Some(PathBuf::from(r"D:\saves")));
        assert_eq!(cfg_get("savestate_directory = \":\\states\"\n", "savestate_directory").as_deref(), Some(r":\states"));
    }

    #[test]
    fn retroarch_keeps_guide_for_the_launcher() {
        let guide = retroarch_append(HomeButton::Guide, true, true, None);
        assert!(guide.contains("input_menu_toggle_btn = \"99\""));
        assert!(guide.contains("input_quit_gamepad_combo = \"4\""));
        assert!(guide.contains("config_save_on_exit = \"false\""));
        assert!(guide.contains("savestate_auto_load = \"true\""));
        let fresh = retroarch_append(HomeButton::Select, false, false, Some(2));
        assert!(!fresh.contains("input_menu_toggle_btn"));
        assert!(!fresh.contains("config_save_on_exit"));
        assert!(fresh.contains("savestate_auto_load = \"false\""));
        assert!(fresh.contains("video_monitor_index = \"2\""));
    }

    #[test]
    fn pcsx2_ini_has_version_pad_and_no_guide() {
        let s = pcsx2_ini();
        assert_eq!(ini_get(&s, "UI", "SettingsVersion").as_deref(), Some("1"));
        assert_eq!(ini_get(&s, "UI", "ConfirmShutdown").as_deref(), Some("false"));
        assert_eq!(ini_get(&s, "Pad1", "Cross").as_deref(), Some("XInput-0/A"));
        assert_eq!(ini_get(&s, "Pad1", "LUp").as_deref(), Some("XInput-0/-LeftY"));
        assert!(!s.contains("Guide"));
    }
}

#[cfg(test)]
mod real {
    use super::*;

    /// Reads the versions of the copies `install::tests::real_unpack` left in the temp folder:
    /// `cargo test real_versions -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_versions() {
        let dir = std::env::temp_dir().join("tvl-unpack");
        for emu in Emu::ALL {
            let exe = find_exe(&dir.join(emu.name()), emu).expect("run real_unpack first");
            let v = version(emu, &exe);
            println!("{}: {v:?}", emu.name());
            assert!(v.is_some());
        }
    }
}
