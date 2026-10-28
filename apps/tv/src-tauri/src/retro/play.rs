//! Starting a retro game and getting back from it.
//!
//! The controller: while the emulator runs the launcher is in game mode, so it acts on nothing
//! but Home (input.rs) and the emulator has the pad to itself. Home doesn't leave the emulator
//! running behind the launcher (it would keep reading the pad there): it saves and quits it, the
//! way each one does that by itself, and the launcher comes back when the process ends.

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use tauri::{AppHandle, Emitter, Manager};

use super::emu::{self, Emu, DOLPHIN_SLOT};
use super::systems::{self, Bios};
use super::{bios, states, win, Source};
use crate::error::{Error, Result};
use crate::focus_win;
use crate::state::{set_mode, AppState, Mode};

/// RetroArch's network command port, for QUIT.
pub const RETROARCH_PORT: u16 = 55355;
/// RetroArch gets this long to quit on its QUIT before its window is closed instead.
const RETROARCH_QUIT_WAIT: Duration = Duration::from_secs(4);
/// Dolphin's save to slot 8 is written in the background; it gets this long before closing.
const DOLPHIN_SAVE_WAIT: Duration = Duration::from_millis(1500);
/// An emulator still running this long after being asked to quit is stopped.
const QUIT_GRACE: Duration = Duration::from_secs(15);

pub struct Session {
    pub game: String,
    pub emu: Emu,
    pub pid: u32,
    started: Instant,
    since: SystemTime,
    states_dir: PathBuf,
    quitting: bool,
}

/// The retro game running now, if any.
pub fn running(app: &AppHandle) -> Option<String> {
    app.state::<AppState>().retro.session.lock().unwrap().as_ref().map(|s| s.game.clone())
}

/// The launcher's monitor when there's more than one: its place among them (1-based, the order
/// RetroArch counts them in) and a point inside it.
fn tv_monitor(app: &AppHandle) -> Option<(u32, i32, i32)> {
    let w = app.get_webview_window("main")?;
    let cur = w.current_monitor().ok()??;
    let all = w.available_monitors().ok()?;
    if all.len() < 2 {
        return None;
    }
    let i = all.iter().position(|m| m.position() == cur.position())? as u32 + 1;
    Some((i, cur.position().x + 64, cur.position().y + 64))
}

/// Plays `id`; `fresh` starts it over instead of from where it was left.
pub fn launch(app: &AppHandle, id: &str, fresh: bool) -> Result<()> {
    let st = app.state::<AppState>();
    if let Some(s) = st.retro.session.lock().unwrap().as_ref() {
        if s.game == id && !s.quitting {
            win::focus(s.pid);
            set_mode(app, Mode::Game);
            return Ok(());
        }
        return Err(Error::msg("Another game is still closing. Try again in a moment."));
    }
    let rom = super::rom(app, id).ok_or_else(|| Error::msg("That game isn't in its folder anymore"))?;
    let sys = systems::get(rom.system).ok_or_else(|| Error::msg("Unknown console"))?;
    let linked = st
        .retro
        .store
        .lock()
        .unwrap()
        .emulators
        .get(&sys.emu)
        .cloned()
        .ok_or_else(|| Error::msg(format!("Install {} or pick its folder first", sys.emu.name())))?;
    let exe = emu::find_exe(&linked.path, sys.emu)
        .ok_or_else(|| Error::msg(format!("{} isn't in {} anymore", sys.emu.exe(), linked.path.display())))?;
    let root = exe.parent().map(PathBuf::from).unwrap_or_else(|| linked.path.clone());
    let dirs = emu::dirs(sys.emu, &root);

    // A BIOS left in the BIOS folder goes where the emulator looks first.
    let bios_folder = st.retro.store.lock().unwrap().bios_folder.clone();
    if let Some(from) = &bios_folder {
        let pcsx2 = (sys.bios == Bios::Ps2).then_some(dirs.bios.as_deref()).flatten();
        let ra = (sys.bios == Bios::Ps1).then_some(dirs.bios.as_deref()).flatten();
        bios::copy_missing(from, ra, pcsx2);
    }
    match sys.bios {
        Bios::Ps1 if dirs.bios.as_deref().and_then(bios::ps1_in).is_none() => {
            return Err(Error::msg("PlayStation games need a BIOS dumped from your own console, in RetroArch's system folder"));
        }
        Bios::Ps2 if dirs.bios.as_deref().and_then(bios::ps2_in).is_none() => {
            return Err(Error::msg("PlayStation 2 games need a BIOS dumped from your own console, in PCSX2's bios folder"));
        }
        _ => {}
    }

    let monitor = tv_monitor(app);
    let home = st.config.read().unwrap().settings.home_button;
    let played = st.retro.store.lock().unwrap().games.get(id).cloned().unwrap_or_default();
    let mut cmd = Command::new(&exe);
    cmd.current_dir(&root);
    match sys.emu {
        Emu::Retroarch => {
            let core = super::core_path(&st, &dirs, sys.core.unwrap_or_default())
                .ok_or_else(|| Error::msg(format!("RetroArch's {} core isn't installed", sys.core.unwrap_or_default())))?;
            let cfg = st.data_dir.join("retro").join("retroarch-tv.cfg");
            let text = emu::retroarch_append(home, !fresh, linked.source == Source::Linked, monitor.map(|m| m.0));
            super::install::write_if_changed(&cfg, &text)?;
            cmd.arg("-L").arg(core).arg("-f").arg("--appendconfig").arg(&cfg).arg(&rom.path);
        }
        Emu::Pcsx2 => {
            emu::pcsx2_tv(&root)?;
            cmd.args(["-nogui", "-fullscreen"]);
            let resume = played.prefix.as_ref().map(|p| dirs.states.join(format!("{p}.resume.p2s")));
            if let Some(state) = resume.filter(|p| !fresh && p.is_file()) {
                cmd.arg("-statefile").arg(state);
            }
            cmd.arg("--").arg(&rom.path);
        }
        Emu::Dolphin => {
            cmd.arg("-b");
            for c in emu::dolphin_config(monitor.map(|m| (m.1, m.2))) {
                cmd.arg("-C").arg(c);
            }
            let auto = played.prefix.as_ref().map(|p| dirs.states.join(format!("{p}.s{DOLPHIN_SLOT:02}")));
            if let Some(state) = auto.filter(|p| !fresh && p.is_file()) {
                cmd.arg("-s").arg(state);
            }
            cmd.arg("-e").arg(&rom.path);
        }
    }

    // The emulator's windows and states folders hold the save-state pictures the UI shows.
    let _ = app.asset_protocol_scope().allow_directory(&dirs.states, true);
    crate::watch::end(app);
    focus_win::cover(app);
    let mut child = cmd.spawn().inspect_err(|_| focus_win::uncover(app))?;
    let pid = child.id();
    *st.retro.session.lock().unwrap() = Some(Session {
        game: id.to_string(),
        emu: sys.emu,
        pid,
        started: Instant::now(),
        since: SystemTime::now(),
        states_dir: dirs.states,
        quitting: false,
    });
    set_mode(app, Mode::Game);
    let _ = app.emit("retro-started", id);
    focus_win::uncover_when_game_fullscreen(app, pid, move |app| {
        app.state::<AppState>().retro.session.lock().unwrap().as_ref().is_some_and(|s| s.pid == pid)
    });

    let app = app.clone();
    std::thread::spawn(move || {
        let _ = child.wait();
        ended(&app, pid);
    });
    Ok(())
}

/// The emulator closed: play time and save states noted, the launcher back in front.
fn ended(app: &AppHandle, pid: u32) {
    let st = app.state::<AppState>();
    let session = {
        let mut slot = st.retro.session.lock().unwrap();
        if !slot.as_ref().is_some_and(|s| s.pid == pid) {
            return;
        }
        slot.take().unwrap()
    };
    let minutes = ((session.started.elapsed().as_secs() + 30) / 60) as u32;
    let learned = states::learn(session.emu, &states::files(&session.states_dir), session.since);
    {
        let mut store = st.retro.store.lock().unwrap();
        let g = store.games.entry(session.game.clone()).or_default();
        g.minutes += minutes;
        g.last = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        if learned.is_some() {
            g.prefix = learned;
        }
    }
    super::save(&st);
    if st.mode() == Mode::Game {
        set_mode(app, Mode::Launcher);
        focus_win::bring_to_front(app);
    } else {
        set_mode(app, st.mode());
    }
    super::changed(app);
    let _ = app.emit("games-updated", ());
}

/// Home during a retro game: saves and quits it. False when no retro game is running.
pub fn quit(app: &AppHandle) -> bool {
    let st = app.state::<AppState>();
    let (emu, pid) = {
        let mut slot = st.retro.session.lock().unwrap();
        let Some(s) = slot.as_mut() else { return false };
        if s.quitting {
            return true;
        }
        s.quitting = true;
        (s.emu, s.pid)
    };
    let app = app.clone();
    std::thread::spawn(move || {
        let alive = || app.state::<AppState>().retro.session.lock().unwrap().as_ref().is_some_and(|s| s.pid == pid);
        let asked = Instant::now();
        match emu {
            Emu::Retroarch => {
                let sent = std::net::UdpSocket::bind("127.0.0.1:0")
                    .and_then(|s| s.send_to(b"QUIT", ("127.0.0.1", RETROARCH_PORT)))
                    .is_ok();
                let until = Instant::now() + RETROARCH_QUIT_WAIT;
                while sent && alive() && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(100));
                }
                if alive() {
                    win::close_windows(pid);
                }
            }
            // ConfirmShutdown is off and SaveStateOnShutdown on (emu::PCSX2_TV_KEYS): closing
            // the window saves the resume state and, in -nogui mode, exits.
            Emu::Pcsx2 => {
                win::close_windows(pid);
            }
            // Dolphin has no save-on-exit: its own Save State Slot 8 key first (Shift + F8),
            // then closing the window stops the game, and -b mode exits.
            Emu::Dolphin => {
                win::focus(pid);
                std::thread::sleep(Duration::from_millis(150));
                save_slot_key();
                std::thread::sleep(DOLPHIN_SAVE_WAIT);
                win::close_windows(pid);
            }
        }
        while alive() && asked.elapsed() < QUIT_GRACE {
            std::thread::sleep(Duration::from_millis(200));
        }
        if alive() {
            eprintln!("{} didn't quit; stopping it", emu.name());
            crate::launcher::kill_tree(pid);
        }
    });
    true
}

fn save_slot_key() {
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    let Ok(mut enigo) = Enigo::new(&Settings::default()) else { return };
    let _ = enigo.key(Key::Shift, Direction::Press);
    let _ = enigo.key(Key::F8, Direction::Click);
    let _ = enigo.key(Key::Shift, Direction::Release);
}
