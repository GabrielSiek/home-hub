//! "Use my folder": a folder picker for the pad (design canvas: Games · Use my folder). It lists
//! a folder's folders, says what each holds (an emulator and its version, game folders, BIOS
//! files), and checks the focused one against what's being picked.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::emu::{self, Emu};
use super::systems::{self, System};
use super::{bios, scan, win};

/// What the picker is choosing a folder for.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Target {
    Emulator { emu: Emu },
    Folder { system: String },
    Bios,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaceKind {
    Drive,
    Folder,
    Net,
}

#[derive(Debug, Clone, Serialize)]
pub struct Place {
    pub label: String,
    pub path: String,
    /// "412 GB free"
    pub note: String,
    pub kind: PlaceKind,
    /// "THIS PC", "RECENT", "NETWORK"
    pub group: &'static str,
}

/// What a folder holds, as far as the picker cares.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Holds {
    Emulator { emu: Emu, version: Option<String> },
    /// Console folders inside: (console, its folder, how many games).
    Roms { folders: Vec<(String, String, usize)> },
    Games { system: String, count: usize },
    Bios { files: Vec<String> },
    Nothing,
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
    /// "7 folders · 114 files · changed Oct 3"
    pub sub: String,
    pub holds: Holds,
}

#[derive(Debug, Clone, Serialize)]
pub struct Listing {
    /// None: the top, where the drives are.
    pub path: Option<String>,
    pub parent: Option<String>,
    pub places: Vec<Place>,
    pub entries: Vec<Entry>,
}

pub fn gb(bytes: u64) -> String {
    let gb = bytes as f64 / 1e9;
    if gb >= 1000.0 { format!("{:.1} TB", gb / 1000.0) } else { format!("{gb:.0} GB") }
}

pub fn places(recent: &[PathBuf]) -> Vec<Place> {
    let mut out: Vec<Place> = win::drives()
        .into_iter()
        .map(|d| {
            let letter = d.root.trim_end_matches('\\').to_string();
            let label = if d.label.is_empty() {
                if d.removable { "USB drive".into() } else { "Local disk".into() }
            } else {
                d.label
            };
            Place {
                label: format!("{letter}  {label}"),
                path: d.root,
                note: d.free.map(|f| format!("{} free", gb(f))).unwrap_or_default(),
                kind: PlaceKind::Drive,
                group: "THIS PC",
            }
        })
        .collect();
    let net = |p: &Path| p.to_string_lossy().starts_with(r"\\");
    let mut folders: Vec<PathBuf> = recent.iter().filter(|p| !net(p)).take(4).cloned().collect();
    if let Some(d) = win::downloads() {
        if !folders.contains(&d) {
            folders.push(d);
        }
    }
    for p in folders {
        let label = if Some(&p) == win::downloads().as_ref() { "Downloads".into() } else { p.display().to_string() };
        out.push(Place { label, path: p.display().to_string(), note: String::new(), kind: PlaceKind::Folder, group: "RECENT" });
    }
    for p in recent.iter().filter(|p| net(p)).take(3) {
        out.push(Place { label: p.display().to_string(), path: p.display().to_string(), note: String::new(), kind: PlaceKind::Net, group: "NETWORK" });
    }
    out
}

fn short_day(t: SystemTime) -> String {
    let (y, m, d) = civil(t);
    let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    if y == civil(SystemTime::now()).0 { format!("{} {d}", months[(m - 1) as usize]) } else { y.to_string() }
}

/// Year, month and day (UTC) from days since 1970 (Howard Hinnant's algorithm).
fn civil(t: SystemTime) -> (i64, i64, i64) {
    let days = t.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

/// Lists `path`'s folders (the drives with none).
pub fn list(path: Option<&str>, recent: &[PathBuf]) -> Listing {
    let places = places(recent);
    let Some(path) = path.filter(|p| !p.is_empty()) else {
        let entries = win::drives()
            .into_iter()
            .map(|d| Entry {
                name: if d.label.is_empty() { d.root.clone() } else { format!("{} {}", d.root.trim_end_matches('\\'), d.label) },
                sub: d.free.map(|f| format!("{} free", gb(f))).unwrap_or_default(),
                path: d.root,
                holds: Holds::Nothing,
            })
            .collect();
        return Listing { path: None, parent: None, places, entries };
    };
    let dir = PathBuf::from(path);
    let parent = dir.parent().map(|p| p.display().to_string()).or(Some(String::new()));
    let mut subs: Vec<PathBuf> = fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .filter(|e| {
                    let n = e.file_name().to_string_lossy().to_string();
                    !n.starts_with('.') && !n.starts_with('$') && n != "System Volume Information"
                })
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    subs.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    let entries = subs.into_iter().map(|p| entry(&p)).collect();
    Listing { path: Some(dir.display().to_string()), parent, places, entries }
}

fn entry(dir: &Path) -> Entry {
    let (mut folders, mut files) = (0, 0);
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => folders += 1,
                Ok(_) => files += 1,
                Err(_) => {}
            }
        }
    }
    let mut parts = Vec::new();
    if folders > 0 {
        parts.push(format!("{folders} folder{}", if folders == 1 { "" } else { "s" }));
    }
    if files > 0 || folders == 0 {
        parts.push(format!("{files} file{}", if files == 1 { "" } else { "s" }));
    }
    if let Ok(t) = fs::metadata(dir).and_then(|m| m.modified()) {
        parts.push(format!("changed {}", short_day(t)));
    }
    Entry {
        name: dir.file_name().map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into_owned()),
        path: dir.display().to_string(),
        sub: parts.join(" · "),
        holds: holds(dir),
    }
}

/// The console folders right inside `dir`: (console id, path, games).
pub fn console_folders(dir: &Path) -> Vec<(String, String, usize)> {
    let mut out: Vec<(String, String, usize)> = fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .filter_map(|e| {
                    let s = systems::by_folder_name(&e.file_name().to_string_lossy())?;
                    Some((s.id.to_string(), e.path().display().to_string(), scan::count(s, &e.path())))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|(id, _, _)| systems::SYSTEMS.iter().position(|s| s.id == id));
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

pub fn holds(dir: &Path) -> Holds {
    if let Some((emu, exe)) = emu::identify(dir) {
        return Holds::Emulator { emu, version: emu::version(emu, &exe) };
    }
    let name = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let console = systems::by_folder_name(&name);
    let folders = console_folders(dir);
    // One console folder inside another console's folder is a game's own folder, not "roms".
    if folders.len() >= 2 || (folders.len() == 1 && console.is_none() && folders[0].2 > 0) {
        return Holds::Roms { folders };
    }
    let files = bios::files_in(dir);
    if !files.is_empty() {
        return Holds::Bios { files };
    }
    if let Some(s) = console {
        return Holds::Games { system: s.id.into(), count: scan::count(s, dir) };
    }
    Holds::Nothing
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Yes,
    Note,
}

#[derive(Debug, Clone, Serialize)]
pub struct Line {
    pub tone: Tone,
    pub text: String,
}

/// What "Use this folder" would do here, and what else the folder is good for.
#[derive(Debug, Clone, Serialize)]
pub struct Check {
    /// "match": usable as asked; "other": something else useful; "none": nothing.
    pub verdict: &'static str,
    pub title: String,
    pub path: String,
    pub lines: Vec<Line>,
    /// A linked emulator whose settings live outside its folder: what they are, so the user can
    /// keep them there or move them in.
    pub settings: Option<SettingsChoice>,
    /// Console folders beside the emulator, offered as game folders too.
    pub suggest: Vec<(String, String, usize)>,
    /// The other thing the folder can be used for ("Use it for PCSX2 instead").
    pub alt: Option<Alt>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SettingsChoice {
    /// "Documents\Dolphin Emulator · 2 controller profiles, 14 memory card saves"
    pub keep: String,
    /// "Copies them and adds portable.txt, so Dolphin reads its settings from …\User"
    pub moving: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Alt {
    Emulator { emu: Emu, label: String },
    Folders { folders: Vec<(String, String, usize)>, label: String },
    Folder { system: String, label: String },
    Bios { label: String },
}

fn yes(t: impl Into<String>) -> Line {
    Line { tone: Tone::Yes, text: t.into() }
}
fn note(t: impl Into<String>) -> Line {
    Line { tone: Tone::Note, text: t.into() }
}

fn system_list(emu: Emu) -> String {
    let names: Vec<&str> = systems::SYSTEMS.iter().filter(|s| s.emu == emu).map(|s| s.short).collect();
    names.join(", ")
}

pub fn check(target: &Target, dir: &Path) -> Check {
    let path = dir.display().to_string();
    let base = |verdict, title: String, lines| Check { verdict, title, path: path.clone(), lines, settings: None, suggest: vec![], alt: None };
    let what = holds(dir);
    let alt_for = |what: &Holds| -> Option<Alt> {
        match what {
            Holds::Emulator { emu, .. } => Some(Alt::Emulator { emu: *emu, label: format!("Use it for {} instead", emu.name()) }),
            Holds::Roms { folders } => Some(Alt::Folders { folders: folders.clone(), label: format!("Set all {} as game folders", folders.len()) }),
            Holds::Games { system, .. } => Some(Alt::Folder {
                system: system.clone(),
                label: format!("Use for {}", systems::get(system).map_or("", |s| s.name)),
            }),
            Holds::Bios { .. } => Some(Alt::Bios { label: "Use for BIOS files".into() }),
            Holds::Nothing => None,
        }
    };
    match target {
        Target::Emulator { emu: want } => match &what {
            Holds::Emulator { emu, version } if emu == want => {
                let exe = emu::find_exe(dir, *emu).unwrap_or_default();
                let mut c = base(
                    "match",
                    format!("{} found", emu.name()),
                    vec![
                        yes(format!(
                            "{}{}",
                            exe.file_name().unwrap_or_default().to_string_lossy(),
                            version.as_ref().map(|v| format!(" · version {v}")).unwrap_or_default()
                        )),
                        yes("Starts games from the command line, so the launcher can open and close it"),
                    ],
                );
                let root = exe.parent().unwrap_or(dir);
                c.settings = settings_choice(*emu, root);
                // Console folders next to the emulator's folder, or inside it.
                let near: Vec<_> = dir
                    .parent()
                    .into_iter()
                    .flat_map(|p| {
                        let mut v = console_folders(p);
                        for name in ["roms", "ROMs", "games", "jogos"] {
                            v.extend(console_folders(&p.join(name)));
                        }
                        v
                    })
                    .chain(console_folders(dir))
                    .filter(|(id, _, n)| *n > 0 && systems::get(id).is_some_and(|s| s.emu == *want))
                    .collect();
                c.suggest = near;
                c
            }
            Holds::Emulator { emu, version } => {
                let mut c = base(
                    "other",
                    format!("This is {}, not {}", emu.name(), want.name()),
                    vec![
                        note(format!(
                            "Found {}{}. It runs {}, so it can’t open {} games.",
                            emu.name(),
                            version.as_ref().map(|v| format!(" {v}")).unwrap_or_default(),
                            system_list(*emu),
                            system_list(*want)
                        )),
                        note(format!("You can link it to {} from here and keep going.", emu.name())),
                    ],
                );
                c.alt = alt_for(&what);
                c
            }
            other => nothing_for(base("none", format!("No {} here", want.name()), vec![]), other, alt_for(other)),
        },
        Target::Folder { system } => {
            let Some(sys) = systems::get(system) else {
                return base("none", "Unknown console".into(), vec![]);
            };
            let count = scan::count(sys, dir);
            if count > 0 {
                let mut c = base(
                    "match",
                    format!("{count} {} game{}", sys.name, if count == 1 { "" } else { "s" }),
                    vec![yes(format!("Files ending in {}", exts(sys))), note("Covers come from libretro’s thumbnails, matched by file name")],
                );
                if let Holds::Roms { .. } = what {
                    c.alt = alt_for(&what);
                }
                return c;
            }
            nothing_for(
                base("none", format!("No {} games here", sys.name), vec![note(format!("{} games end in {}.", sys.name, exts(sys)))]),
                &what,
                alt_for(&what),
            )
        }
        Target::Bios => {
            let files = bios::files_in(dir);
            if !files.is_empty() {
                return base("match", "BIOS files found".into(), files.iter().map(|f| yes(f.clone())).collect());
            }
            nothing_for(base("none", "No BIOS files here".into(), vec![]), &what, alt_for(&what))
        }
    }
}

fn exts(sys: &System) -> String {
    sys.exts.iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(" ")
}

/// A folder that isn't what's asked: say what it is instead.
fn nothing_for(mut c: Check, what: &Holds, alt: Option<Alt>) -> Check {
    match what {
        Holds::Roms { folders } => {
            let names: Vec<&str> = folders.iter().map(|f| f.1.rsplit(['\\', '/']).next().unwrap_or("")).collect();
            let total: usize = folders.iter().map(|f| f.2).sum();
            c.verdict = "other";
            c.lines.push(note(format!("It looks like your game folders: {}, {total} games in total.", names.join(", "))));
        }
        Holds::Bios { files } => {
            c.verdict = "other";
            c.lines.push(note(format!("Found {}.", files.join(", "))));
        }
        Holds::Games { system, count } => {
            c.verdict = "other";
            let name = systems::get(system).map_or("", |s| s.name);
            c.lines.push(note(format!("{count} {name} games are in this folder.")));
        }
        Holds::Emulator { emu, .. } => {
            c.verdict = "other";
            c.lines.push(note(format!("{} is in this folder.", emu.name())));
        }
        Holds::Nothing => {
            if c.lines.is_empty() {
                c.lines.push(note("No emulator, games or BIOS files. Press RB to look inside."));
            }
        }
    }
    c.alt = alt;
    c
}

/// Dolphin and PCSX2 keep their settings in Documents unless they're in portable mode: offer to
/// keep them there or move them into the folder.
fn settings_choice(emu: Emu, root: &Path) -> Option<SettingsChoice> {
    let (outside, marker, into) = match emu {
        Emu::Dolphin => (emu::dolphin_user(root), "portable.txt", root.join("User")),
        Emu::Pcsx2 => (emu::pcsx2_data(root), "portable.ini", root.to_path_buf()),
        Emu::Retroarch => return None,
    };
    if outside.starts_with(root) || !outside.is_dir() {
        return None;
    }
    let count = |sub: &[&str], exts: &[&str]| -> usize {
        let mut dir = outside.clone();
        for s in sub {
            dir.push(s);
        }
        count_files(&dir, exts, 3)
    };
    let detail = match emu {
        Emu::Dolphin => {
            let profiles = count(&["Config", "Profiles", "GCPad"], &["ini"]);
            let cards = count(&["GC"], &["raw", "gci"]);
            format!("{profiles} controller profile{}, {cards} memory card save{}", plural(profiles), plural(cards))
        }
        _ => {
            let cards = count(&["memcards"], &["ps2"]);
            let states = count(&["sstates"], &["p2s"]);
            format!("{cards} memory card{}, {states} save state{}", plural(cards), plural(states))
        }
    };
    let shown = win::documents()
        .and_then(|d| d.parent().map(Path::to_path_buf))
        .and_then(|home| outside.strip_prefix(home).ok().map(|p| p.display().to_string()))
        .unwrap_or_else(|| outside.display().to_string());
    Some(SettingsChoice {
        keep: format!("{shown} · {detail}"),
        moving: format!("Copies them and adds {marker}, so {} reads its settings from {}", emu.name(), into.display()),
    })
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn count_files(dir: &Path, exts: &[&str], depth: usize) -> usize {
    let Ok(rd) = fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() && depth > 0 => count_files(&e.path(), exts, depth - 1),
            Ok(t) if t.is_file() => {
                let p = e.path();
                usize::from(p.extension().is_some_and(|x| exts.iter().any(|w| x.eq_ignore_ascii_case(w))))
            }
            _ => 0,
        })
        .sum()
}

/// Copies a linked emulator's settings into its folder and switches it to portable mode.
pub fn move_settings(emu: Emu, root: &Path) -> std::io::Result<()> {
    let (outside, marker, into) = match emu {
        Emu::Dolphin => (emu::dolphin_user(root), "portable.txt", root.join("User")),
        Emu::Pcsx2 => (emu::pcsx2_data(root), "portable.ini", root.to_path_buf()),
        Emu::Retroarch => return Ok(()),
    };
    if outside.starts_with(root) {
        return Ok(());
    }
    if outside.is_dir() {
        super::install::copy_tree(&outside, &into)?;
    }
    fs::write(root.join(marker), "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spots_roms_and_console_folders() {
        let dir = std::env::temp_dir().join(format!("tvl-browse-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let roms = dir.join("roms");
        for (sub, file) in [("n64", "Super Mario 64 (USA).z64"), ("gc", "Melee.rvz"), ("snes", "Top Gear.sfc")] {
            fs::create_dir_all(roms.join(sub)).unwrap();
            fs::write(roms.join(sub).join(file), b"x").unwrap();
        }
        fs::create_dir_all(dir.join("ferramentas")).unwrap();
        match holds(&roms) {
            Holds::Roms { folders } => assert_eq!(folders.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), ["n64", "gc", "snes"]),
            h => panic!("{h:?}"),
        }
        assert_eq!(holds(&roms.join("gc")), Holds::Games { system: "gc".into(), count: 1 });
        assert_eq!(holds(&dir.join("ferramentas")), Holds::Nothing);

        let c = check(&Target::Folder { system: "gc".into() }, &roms.join("gc"));
        assert_eq!(c.verdict, "match");
        let c = check(&Target::Emulator { emu: Emu::Dolphin }, &roms);
        assert_eq!(c.verdict, "other");
        assert!(matches!(c.alt, Some(Alt::Folders { .. })));
        let l = list(Some(&dir.display().to_string()), &[]);
        assert_eq!(l.entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["ferramentas", "roms"]);
        let _ = fs::remove_dir_all(&dir);
    }
}
