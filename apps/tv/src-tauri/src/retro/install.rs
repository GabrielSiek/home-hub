//! "Install for me": the newest release from each emulator's own site, downloaded with progress
//! for the card, unpacked into C:\TVLauncher\emulators\<name> and set up for the TV in portable
//! mode. Also RetroArch's cores, and the update check.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::emu::{self, Emu};
use super::{systems, win, Linked, Source};
use crate::error::{Error, Result};
use crate::state::AppState;

const RA_STABLE: &str = "https://buildbot.libretro.com/stable/";
const RA_CORES: &str = "https://buildbot.libretro.com/nightly/windows/x86_64/latest/";
const RA_INFO: &str = "https://buildbot.libretro.com/assets/frontend/info.zip";
const PCSX2_LATEST: &str = "https://api.github.com/repos/PCSX2/pcsx2/releases/latest";
/// Dolphin's "beta" update track is its release builds (2407, 2409…); "dev" is every commit.
const DOLPHIN_LATEST: &str = "https://dolphin-emu.org/update/latest/beta/";
const UA: &str = "tv-launcher";
/// Progress reaches the screen at most this often.
const TICK: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub file: String,
    /// "Sep 4", when the site says.
    pub date: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Checking,
    Downloading,
    Extracting,
    Configuring,
}

/// An install in progress, as its card shows it.
#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub stage: Stage,
    pub version: Option<String>,
    /// Bytes (downloading, extracting) or cores (configuring) done, of `total`.
    pub done: u64,
    pub total: u64,
    /// Download speed, bytes per second.
    pub speed: u64,
    /// The four steps: release found, download, extract, set up.
    pub steps: [String; 4],
}

/// Downloads run for minutes: no overall timeout, only for connecting and between bytes.
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(UA)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()
        .unwrap_or_default()
}

fn short_date(iso: &str) -> Option<String> {
    let (_, m, d) = (iso.get(0..4)?, iso.get(5..7)?.parse::<usize>().ok()?, iso.get(8..10)?.parse::<u32>().ok()?);
    let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    Some(format!("{} {d}", months.get(m.checked_sub(1)?)?))
}

/// The newest version on buildbot's stable folder: "1.22.2".
fn newest_stable(html: &str) -> Option<String> {
    html.split("href=\"/stable/")
        .skip(1)
        .filter_map(|s| s.split('/').next())
        .filter_map(|v| {
            let parts: Option<Vec<u32>> = v.split('.').map(|n| n.parse().ok()).collect();
            parts.map(|p| (p, v.to_string()))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, v)| v)
}

pub async fn latest(http: &reqwest::Client, emu: Emu) -> Result<Release> {
    match emu {
        Emu::Retroarch => {
            let html = http.get(RA_STABLE).send().await?.error_for_status()?.text().await?;
            let version = newest_stable(&html).ok_or_else(|| Error::msg("No RetroArch release on buildbot"))?;
            Ok(Release {
                url: format!("{RA_STABLE}{version}/windows/x86_64/RetroArch.7z"),
                file: "RetroArch.7z".into(),
                version,
                date: None,
            })
        }
        Emu::Pcsx2 => {
            #[derive(Deserialize)]
            struct Gh {
                tag_name: String,
                published_at: Option<String>,
                assets: Vec<Asset>,
            }
            #[derive(Deserialize)]
            struct Asset {
                name: String,
                browser_download_url: String,
            }
            let r: Gh = http
                .get(PCSX2_LATEST)
                .header("Accept", "application/vnd.github+json")
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            let a = r
                .assets
                .into_iter()
                .find(|a| a.name.ends_with("windows-x64-Qt.7z"))
                .ok_or_else(|| Error::msg("PCSX2's latest release has no Windows download"))?;
            Ok(Release {
                version: r.tag_name.trim_start_matches('v').to_string(),
                url: a.browser_download_url,
                file: a.name,
                date: r.published_at.as_deref().and_then(short_date),
            })
        }
        Emu::Dolphin => {
            #[derive(Deserialize)]
            struct Latest {
                shortrev: String,
                date: Option<String>,
                artifacts: Vec<Artifact>,
            }
            #[derive(Deserialize)]
            struct Artifact {
                system: String,
                url: String,
            }
            let r: Latest = http.get(DOLPHIN_LATEST).send().await?.error_for_status()?.json().await?;
            let a = r
                .artifacts
                .into_iter()
                .find(|a| a.system == "Windows x64")
                .ok_or_else(|| Error::msg("Dolphin's latest release has no Windows x64 download"))?;
            Ok(Release {
                file: a.url.rsplit('/').next().unwrap_or("dolphin.7z").to_string(),
                url: a.url,
                version: r.shortrev,
                date: r.date.as_deref().and_then(short_date),
            })
        }
    }
}

fn steps(emu: Emu, rel: Option<&Release>, target: &Path) -> [String; 4] {
    let found = match rel {
        None => format!("Looking for the latest {}", emu.name()),
        Some(r) => {
            let at = match emu {
                Emu::Retroarch => "on buildbot",
                Emu::Pcsx2 => "in GitHub releases",
                Emu::Dolphin => "on dolphin-emu.org",
            };
            match &r.date {
                Some(d) => format!("Found {} {at} ({d})", r.version),
                None => format!("Found {} {at}", r.version),
            }
        }
    };
    let setup = match emu {
        Emu::Retroarch => "Add the cores, fullscreen on the TV, hotkeys",
        Emu::Pcsx2 => "Add portable.ini, fullscreen on the TV, hotkeys",
        Emu::Dolphin => "Add portable.txt, fullscreen, hotkeys",
    };
    [
        found,
        format!("Download {}", rel.map_or("the release", |r| r.file.as_str())),
        format!("Extract to {}", target.display()),
        setup.to_string(),
    ]
}

/// The card's progress, rate-limited to `TICK` unless `force`.
struct Reporter<'a> {
    app: &'a AppHandle,
    emu: Emu,
    last: Instant,
}

impl Reporter<'_> {
    fn set(&mut self, p: Progress, force: bool) {
        if !force && self.last.elapsed() < TICK {
            return;
        }
        self.last = Instant::now();
        self.app.state::<AppState>().retro.installs.lock().unwrap().insert(self.emu, p);
        super::changed(self.app);
    }
}

fn cancelled(app: &AppHandle, emu: Emu) -> bool {
    app.state::<AppState>().retro.cancel.lock().unwrap().contains(&emu)
}

/// Installs (or updates) `emu`; the outcome is on its card.
pub async fn install(app: AppHandle, emu: Emu) {
    let st = app.state::<AppState>();
    st.retro.cancel.lock().unwrap().remove(&emu);
    st.retro.errors.lock().unwrap().remove(&emu);
    let result = run(&app, emu).await;
    st.retro.installs.lock().unwrap().remove(&emu);
    let was_cancelled = st.retro.cancel.lock().unwrap().remove(&emu);
    if let Err(e) = result {
        if !was_cancelled {
            eprintln!("installing {} failed: {e}", emu.name());
            st.retro.errors.lock().unwrap().insert(emu, e.to_string());
        }
    }
    super::changed(&app);
}

async fn run(app: &AppHandle, emu: Emu) -> Result<()> {
    let st = app.state::<AppState>();
    let root = win::install_root();
    let target = root.join(emu.name());
    let mut rep = Reporter { app, emu, last: Instant::now() - TICK };
    let blank = |stage, rel: Option<&Release>| Progress {
        stage,
        version: rel.map(|r| r.version.clone()),
        done: 0,
        total: 0,
        speed: 0,
        steps: steps(emu, rel, &target),
    };
    rep.set(blank(Stage::Checking, None), true);

    let http = client();
    let rel = latest(&http, emu).await?;
    st.retro.latest.lock().unwrap().insert(emu, rel.version.clone());

    // Download next to where it goes (same drive, so moving it in is a rename).
    fs::create_dir_all(&root)?;
    let archive = root.join(format!(".{}.download", rel.file));
    let mut p = blank(Stage::Downloading, Some(&rel));
    rep.set(p.clone(), true);
    let started = Instant::now();
    let fetched = fetch(&http, &rel.url, &archive, |done, total| {
        let secs = started.elapsed().as_secs_f64().max(0.5);
        p.done = done;
        p.total = total;
        p.speed = (done as f64 / secs) as u64;
        rep.set(p.clone(), false);
        !cancelled(app, emu)
    })
    .await;
    if let Err(e) = fetched {
        let _ = fs::remove_file(&archive);
        return Err(e);
    }

    let mut p = blank(Stage::Extracting, Some(&rel));
    rep.set(p.clone(), true);
    let unpacked = root.join(format!(".{}-new", emu.name()));
    let _ = fs::remove_dir_all(&unpacked);
    let (a, u) = (archive.clone(), unpacked.clone());
    let app2 = app.clone();
    let extracted = tauri::async_runtime::spawn_blocking(move || {
        let mut rep = Reporter { app: &app2, emu, last: Instant::now() };
        extract_7z(&a, &u, |done, total| {
            p.done = done;
            p.total = total;
            rep.set(p.clone(), false);
            !cancelled(&app2, emu)
        })
    })
    .await
    .map_err(|e| Error::msg(e.to_string()))
    .and_then(|r| r);
    let _ = fs::remove_file(&archive);
    let placed = extracted.and_then(|_| {
        let exe = emu::find_exe(&unpacked, emu)
            .ok_or_else(|| Error::msg(format!("The download has no {}", emu.exe())))?;
        let program = exe.parent().unwrap_or(&unpacked).to_path_buf();
        move_into(&program, &target)?;
        Ok(())
    });
    let _ = fs::remove_dir_all(&unpacked);
    placed?;

    let mut p = blank(Stage::Configuring, Some(&rel));
    rep.set(p.clone(), true);
    match emu {
        Emu::Retroarch => {
            let cores = target.join("cores");
            let wanted: Vec<&str> = systems::SYSTEMS.iter().filter_map(|s| s.core).collect();
            p.total = wanted.len() as u64 + 1;
            for core in wanted {
                if cancelled(app, emu) {
                    return Err(Error::msg("Cancelled"));
                }
                install_core(&http, core, &cores).await?;
                p.done += 1;
                rep.set(p.clone(), true);
            }
            // Core information (names, BIOS lists) for RetroArch's own menus.
            let info = root.join(".info.zip");
            fetch(&http, RA_INFO, &info, |_, _| true).await?;
            let unzipped = unzip(&info, &target.join("info"));
            let _ = fs::remove_file(&info);
            unzipped?;
        }
        Emu::Pcsx2 => {
            fs::write(target.join("portable.ini"), "")?;
            fs::create_dir_all(target.join("bios"))?;
            let ini = target.join("inis").join("PCSX2.ini");
            if ini.exists() {
                emu::pcsx2_tv(&target)?;
            } else {
                fs::create_dir_all(target.join("inis"))?;
                fs::write(ini, emu::pcsx2_ini())?;
            }
        }
        Emu::Dolphin => {
            fs::write(target.join("portable.txt"), "")?;
            let config = target.join("User").join("Config");
            fs::create_dir_all(&config)?;
            let pad = config.join("GCPadNew.ini");
            if !pad.exists() {
                fs::write(pad, emu::DOLPHIN_GCPAD)?;
            }
        }
    }

    let size = folder_size(&target);
    super::set_emulator(
        app,
        emu,
        Some(Linked { source: Source::Installed, path: target, version: Some(rel.version), size: Some(size) }),
    )?;
    Ok(())
}

/// Downloads `url` to `to`. `on(done, total)` hears each chunk and returns false to stop.
async fn fetch(http: &reqwest::Client, url: &str, to: &Path, mut on: impl FnMut(u64, u64) -> bool) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut resp = http.get(url).send().await?.error_for_status()?;
    let total = resp.content_length().unwrap_or(0);
    let mut file = tokio::fs::File::create(to).await?;
    let mut done = 0u64;
    while let Some(chunk) = resp.chunk().await? {
        file.write_all(&chunk).await?;
        done += chunk.len() as u64;
        if !on(done, total) {
            return Err(Error::msg("Cancelled"));
        }
    }
    file.flush().await?;
    Ok(())
}

/// Unpacks a .7z, reporting bytes written of the total; `on` returns false to stop.
fn extract_7z(archive: &Path, dest: &Path, mut on: impl FnMut(u64, u64) -> bool) -> Result<()> {
    let seven = |e: sevenz_rust2::Error| Error::msg(format!("Couldn't unpack the download: {e}"));
    let total: u64 = sevenz_rust2::Archive::open(archive).map_err(seven)?.files.iter().map(|f| f.size()).sum();
    let mut done = 0u64;
    let mut stopped = false;
    let result = sevenz_rust2::decompress_file_with_extract_fn(archive, dest, |entry, reader, path| {
        let ok = sevenz_rust2::default_entry_extract_fn(entry, reader, path)?;
        done += entry.size();
        if !on(done, total) {
            stopped = true;
            return Ok(false);
        }
        Ok(ok)
    });
    if stopped {
        return Err(Error::msg("Cancelled"));
    }
    match result {
        Ok(()) => Ok(()),
        // A filter the crate doesn't know: Windows' own tar (libarchive) reads every 7z.
        Err(e) => {
            eprintln!("7z unpack failed ({e}), trying tar");
            let _ = fs::remove_dir_all(dest);
            fs::create_dir_all(dest)?;
            let mut cmd = std::process::Command::new("tar");
            cmd.arg("-xf").arg(archive).arg("-C").arg(dest);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000);
            }
            let status = cmd.status()?;
            if status.success() {
                on(total, total);
                Ok(())
            } else {
                Err(seven(e))
            }
        }
    }
}

fn unzip(archive: &Path, dest: &Path) -> Result<()> {
    let zipped = |e: zip::result::ZipError| Error::msg(format!("Couldn't unpack {}: {e}", archive.display()));
    let mut zip = zip::ZipArchive::new(fs::File::open(archive)?).map_err(zipped)?;
    fs::create_dir_all(dest)?;
    zip.extract(dest).map_err(zipped)
}

/// Downloads a RetroArch core into `cores` ("mgba" → cores\mgba_libretro.dll).
pub async fn install_core(http: &reqwest::Client, core: &str, cores: &Path) -> Result<()> {
    fs::create_dir_all(cores)?;
    let zip = cores.join(format!(".{core}.zip"));
    fetch(http, &format!("{RA_CORES}{core}_libretro.dll.zip"), &zip, |_, _| true).await?;
    let unzipped = unzip(&zip, cores);
    let _ = fs::remove_file(&zip);
    unzipped
}

/// The cores the RetroArch consoles need that `cores` lacks, downloaded there.
pub async fn add_cores(app: AppHandle, cores: PathBuf) {
    let st = app.state::<AppState>();
    let http = client();
    let missing: Vec<&str> = systems::SYSTEMS
        .iter()
        .filter_map(|s| s.core)
        .filter(|c| !cores.join(format!("{c}_libretro.dll")).exists())
        .collect();
    let mut rep = Reporter { app: &app, emu: Emu::Retroarch, last: Instant::now() - TICK };
    let mut p = Progress {
        stage: Stage::Configuring,
        version: None,
        done: 0,
        total: missing.len() as u64,
        speed: 0,
        steps: [
            "Look up the cores the consoles need".into(),
            format!("Download {}", missing.join(", ")),
            format!("Unpack into {}", cores.display()),
            "Ready to play".into(),
        ],
    };
    rep.set(p.clone(), true);
    for core in missing {
        if let Err(e) = install_core(&http, core, &cores).await {
            st.retro.errors.lock().unwrap().insert(Emu::Retroarch, format!("Couldn't add {core}: {e}"));
            break;
        }
        p.done += 1;
        rep.set(p.clone(), true);
    }
    st.retro.installs.lock().unwrap().remove(&Emu::Retroarch);
    super::changed(&app);
}

/// Moves everything in `from` into `to`, replacing what's there by the same name (an update
/// keeps saves, states and settings: the archives don't carry those).
fn move_into(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for e in fs::read_dir(from)?.flatten() {
        let src = e.path();
        let dst = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            if dst.is_dir() {
                move_into(&src, &dst)?;
                continue;
            }
        } else if dst.exists() {
            fs::remove_file(&dst)?;
        }
        if fs::rename(&src, &dst).is_err() {
            copy_tree(&src, &dst)?;
        }
    }
    Ok(())
}

pub fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_file() {
        fs::copy(from, to)?;
        return Ok(());
    }
    fs::create_dir_all(to)?;
    for e in fs::read_dir(from)?.flatten() {
        copy_tree(&e.path(), &to.join(e.file_name()))?;
    }
    Ok(())
}

pub fn folder_size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => folder_size(&e.path()),
            _ => e.metadata().map_or(0, |m| m.len()),
        })
        .sum()
}

/// Looks up the newest version of each emulator the launcher installed.
pub async fn check_updates(app: AppHandle) {
    let st = app.state::<AppState>();
    let http = client();
    let installed: Vec<Emu> = st
        .retro
        .store
        .lock()
        .unwrap()
        .emulators
        .iter()
        .filter(|(_, l)| l.source == Source::Installed)
        .map(|(e, _)| *e)
        .collect();
    for emu in installed {
        match latest(&http, emu).await {
            Ok(r) => {
                st.retro.latest.lock().unwrap().insert(emu, r.version);
            }
            Err(e) => eprintln!("{} update check failed: {e}", emu.name()),
        }
    }
    st.retro.checked.store(true, Ordering::SeqCst);
    super::changed(&app);
}

/// Writes a file only if it differs, so an emulator's settings aren't touched for nothing.
pub fn write_if_changed(path: &Path, text: &str) -> std::io::Result<()> {
    if fs::read_to_string(path).ok().as_deref() == Some(text) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut f = fs::File::create(path)?;
    f.write_all(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_versions() {
        let html = r#"<a href="/stable/1.9.9/">1.9.9</a><a href="/stable/1.22.2/">x</a><a href="/stable/1.22.10/">y</a><a href="/stable/altstore.json">"#;
        assert_eq!(newest_stable(html).as_deref(), Some("1.22.10"));
    }

    #[test]
    fn dates() {
        assert_eq!(short_date("2026-09-04T23:04:48Z").as_deref(), Some("Sep 4"));
        assert_eq!(short_date("bad"), None);
    }

    /// Asks the real sites for the newest releases: `cargo test real_releases -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_releases() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let http = client();
            for emu in Emu::ALL {
                let r = latest(&http, emu).await.unwrap();
                println!("{}: {} {} {:?}", emu.name(), r.version, r.url, r.date);
            }
        });
    }

    /// Downloads and unpacks the real PCSX2 and Dolphin releases into the temp folder:
    /// `cargo test real_unpack -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_unpack() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let http = client();
            let dir = std::env::temp_dir().join("tvl-unpack");
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            for emu in [Emu::Pcsx2, Emu::Dolphin, Emu::Retroarch] {
                let r = latest(&http, emu).await.unwrap();
                let archive = dir.join(&r.file);
                fetch(&http, &r.url, &archive, |_, _| true).await.unwrap();
                let out = dir.join(emu.name());
                let t = Instant::now();
                extract_7z(&archive, &out, |_, _| true).unwrap();
                let exe = emu::find_exe(&out, emu).expect("program in the archive");
                println!("{}: {} in {:?}, version {:?}", emu.name(), exe.display(), t.elapsed(), emu::version(emu, &exe));
            }
            install_core(&http, "mgba", &dir.join("cores")).await.unwrap();
            assert!(dir.join("cores").join("mgba_libretro.dll").exists());
        });
    }
}
