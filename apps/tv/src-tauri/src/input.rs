//! Reads the controller on its own thread and routes it by mode: to the launcher UI (the player's
//! controls are part of it), to the browser (DevTools, else the keyboard/mouse bridge) while a web
//! app is open, or nowhere during a game. A timer or alarm ringing over a web app takes it first.

use std::sync::{mpsc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use gilrs::{Axis, Button, EventType, Gilrs};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::bridge::Bridge;
use crate::cdp::{self, Cmd};
use crate::config::HomeButton;
use crate::state::{AppState, Mode};
use crate::{games, guide_guard, launcher, retro, ring};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Select,
    Back,
    Home,
    /// Previous / next section tab (LB / RB).
    TabPrev,
    TabNext,
    /// LT / RT: section tabs like LB / RB, unless the screen uses them
    /// (the search keyboard: caps / jump to results).
    TriggerPrev,
    TriggerNext,
    /// More controls for the focused item (Y), e.g. a light's brightness and color.
    /// In the assistant: talk.
    Options,
    /// Opens the assistant (X).
    Assistant,
}

/// Buttons outside the D-pad/A/B set: section tabs in the launcher, keys and clicks in the bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extra {
    X,
    Y,
    Lb,
    Rb,
    Lt,
    Rt,
}

/// System volume step, from the − (Select) and + (Start) buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Volume {
    Down,
    Up,
}

pub fn volume_for(b: Button) -> Option<Volume> {
    match b {
        Button::Select => Some(Volume::Down),
        Button::Start => Some(Volume::Up),
        _ => None,
    }
}

/// What X, Y, a shoulder or a trigger does while the launcher is in front.
pub fn launcher_action(e: Extra) -> Option<Action> {
    match e {
        Extra::Lb => Some(Action::TabPrev),
        Extra::Rb => Some(Action::TabNext),
        Extra::Lt => Some(Action::TriggerPrev),
        Extra::Rt => Some(Action::TriggerNext),
        Extra::Y => Some(Action::Options),
        Extra::X => Some(Action::Assistant),
    }
}

const TICK: Duration = Duration::from_millis(16);
const REPEAT_DELAY: Duration = Duration::from_millis(400);
const REPEAT_RATE: Duration = Duration::from_millis(120);
const STICK_ON: f32 = 0.6;
const STICK_OFF: f32 = 0.4;
const MOUSE_DEADZONE: f32 = 0.15;
const MOUSE_MAX_SPEED: f32 = 28.0;

/// Fires a held input once, then again after a delay, then at a steady rate.
pub struct Repeater<T> {
    held: Option<T>,
    next: Option<Instant>,
}

impl<T> Default for Repeater<T> {
    fn default() -> Self {
        Self { held: None, next: None }
    }
}

impl<T: Copy + PartialEq> Repeater<T> {
    pub fn update(&mut self, dir: Option<T>, now: Instant) -> Option<T> {
        if dir != self.held {
            self.held = dir;
            self.next = dir.map(|_| now + REPEAT_DELAY);
            return dir;
        }
        match (self.held, self.next) {
            (Some(d), Some(next)) if now >= next => {
                self.next = Some(now + REPEAT_RATE);
                Some(d)
            }
            _ => None,
        }
    }
}

/// Left stick to a direction, with hysteresis so it doesn't flicker near the threshold.
/// `y` is positive up, as gilrs reports it.
pub fn stick_direction(x: f32, y: f32, prev: Option<Action>) -> Option<Action> {
    if let Some(p) = prev {
        let along = match p {
            Action::Up => y,
            Action::Down => -y,
            Action::Left => -x,
            Action::Right => x,
            _ => 0.0,
        };
        if along > STICK_OFF {
            return Some(p);
        }
    }
    if x.abs() >= y.abs() {
        if x > STICK_ON {
            Some(Action::Right)
        } else if x < -STICK_ON {
            Some(Action::Left)
        } else {
            None
        }
    } else if y > STICK_ON {
        Some(Action::Up)
    } else if y < -STICK_ON {
        Some(Action::Down)
    } else {
        None
    }
}

/// D-pad state; the most recently pressed direction wins while several are held.
#[derive(Default)]
pub struct Dpad {
    held: Vec<Action>,
}

impl Dpad {
    pub fn set(&mut self, dir: Action, pressed: bool) {
        self.held.retain(|d| *d != dir);
        if pressed {
            self.held.push(dir);
        }
    }

    pub fn current(&self) -> Option<Action> {
        self.held.last().copied()
    }
}

/// The gilrs button for a configured Home button; the Guide button is read separately.
pub fn home_trigger(h: HomeButton) -> Option<Button> {
    match h {
        HomeButton::Guide => None,
        HomeButton::Select => Some(Button::Select),
        HomeButton::Start => Some(Button::Start),
        HomeButton::L3 => Some(Button::LeftThumb),
        HomeButton::R3 => Some(Button::RightThumb),
    }
}

/// The Guide button (the 8BitDo's Home button in X-input mode). gilrs doesn't report it,
/// so it is read through XInput's undocumented XInputGetStateEx.
struct Guide {
    #[cfg(windows)]
    xinput: Option<rusty_xinput::XInputHandle>,
    down: bool,
}

impl Guide {
    fn new() -> Self {
        Self {
            #[cfg(windows)]
            xinput: rusty_xinput::XInputHandle::load_default().ok(),
            down: false,
        }
    }

    /// True once per press.
    fn pressed(&mut self) -> bool {
        let down = self.any_down();
        let edge = down && !self.down;
        self.down = down;
        edge
    }

    #[cfg(windows)]
    fn any_down(&self) -> bool {
        self.xinput
            .as_ref()
            .is_some_and(|x| (0..4).any(|i| x.get_state_ex(i).is_ok_and(|s| s.guide_button())))
    }

    #[cfg(not(windows))]
    fn any_down(&self) -> bool {
        false
    }
}

/// Select + Start held together means Home, whatever the configured Home button is.
#[derive(Default)]
pub struct Chord {
    select: bool,
    start: bool,
    fired: bool,
}

impl Chord {
    /// Returns true once each time the chord completes.
    pub fn set(&mut self, button: Button, pressed: bool) -> bool {
        match button {
            Button::Select => self.select = pressed,
            Button::Start => self.start = pressed,
            _ => return false,
        }
        if !(self.select && self.start) {
            self.fired = false;
            return false;
        }
        !std::mem::replace(&mut self.fired, true)
    }
}

/// Right-stick deflection to a mouse step in pixels per tick (quadratic curve).
pub fn mouse_step(v: f32) -> i32 {
    let a = v.abs();
    if a < MOUSE_DEADZONE {
        return 0;
    }
    let n = ((a - MOUSE_DEADZONE) / (1.0 - MOUSE_DEADZONE)).min(1.0);
    (v.signum() * n * n * MOUSE_MAX_SPEED).round() as i32
}

/// A key from the companion's remote (companion/tv.rs), routed like the pad's own buttons.
#[derive(Debug, Clone, Copy)]
pub enum Remote {
    Action(Action),
    Extra(Extra),
}

static REMOTE: OnceLock<Mutex<mpsc::Sender<Remote>>> = OnceLock::new();

/// Sends a remote key down the pad's path; false when the input thread isn't running.
pub fn remote(r: Remote) -> bool {
    REMOTE.get().is_some_and(|tx| tx.lock().unwrap().send(r).is_ok())
}

pub fn spawn(app: AppHandle) {
    let (tx, rx) = mpsc::channel();
    let _ = REMOTE.set(Mutex::new(tx));
    let _ = std::thread::Builder::new()
        .name("gamepad".into())
        .spawn(move || run(app, rx));
}

fn run(app: AppHandle, remote: mpsc::Receiver<Remote>) {
    let mut router = Router {
        app,
        dpad: Dpad::default(),
        stick: None,
        repeater: Repeater::default(),
        volume_held: None,
        volume: Repeater::default(),
        chord: Chord::default(),
        guide: Guide::new(),
        bridge: Bridge::new(),
    };
    let mut gilrs = match Gilrs::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("gamepad input disabled: {e}");
            // The companion's remote still works.
            for r in remote {
                router.remote(r);
            }
            return;
        }
    };
    router.publish_pad(&gilrs);

    loop {
        let mut next = gilrs.next_event_blocking(Some(TICK));
        while let Some(ev) = next {
            match ev.event {
                EventType::ButtonPressed(b, _) => router.button(b, true),
                EventType::ButtonReleased(b, _) => router.button(b, false),
                EventType::AxisChanged(Axis::LeftStickX | Axis::LeftStickY, _, _) => {
                    let pad = gilrs.gamepad(ev.id);
                    router.stick = stick_direction(
                        pad.value(Axis::LeftStickX),
                        pad.value(Axis::LeftStickY),
                        router.stick,
                    );
                }
                EventType::Connected | EventType::Disconnected => router.publish_pad(&gilrs),
                _ => {}
            }
            next = gilrs.next_event();
        }
        while let Ok(r) = remote.try_recv() {
            router.remote(r);
        }
        router.tick(&gilrs, Instant::now());
    }
}

struct Router {
    app: AppHandle,
    dpad: Dpad,
    stick: Option<Action>,
    repeater: Repeater<Action>,
    volume_held: Option<Volume>,
    volume: Repeater<Volume>,
    chord: Chord,
    guide: Guide,
    bridge: Bridge,
}

impl Router {
    fn mode(&self) -> Mode {
        self.app.state::<AppState>().mode()
    }

    fn home_button(&self) -> HomeButton {
        self.app.state::<AppState>().config.read().unwrap().settings.home_button
    }

    fn button(&mut self, b: Button, pressed: bool) {
        if self.chord.set(b, pressed) {
            self.volume_held = None;
            self.dispatch(Action::Home);
            return;
        }
        if home_trigger(self.home_button()) == Some(b) {
            if pressed {
                self.dispatch(Action::Home);
            }
            return;
        }
        // − / + change the volume, held to repeat; games keep those buttons for themselves.
        if let Some(v) = volume_for(b) {
            if pressed && self.mode() != Mode::Game {
                self.volume_held = Some(v);
            } else if !pressed && self.volume_held == Some(v) {
                self.volume_held = None;
            }
            return;
        }
        let dir = match b {
            Button::DPadUp => Some(Action::Up),
            Button::DPadDown => Some(Action::Down),
            Button::DPadLeft => Some(Action::Left),
            Button::DPadRight => Some(Action::Right),
            _ => None,
        };
        if let Some(d) = dir {
            self.dpad.set(d, pressed);
            return;
        }
        if !pressed {
            // Y released: ends push-to-talk in the assistant.
            if b == Button::North && matches!(self.mode(), Mode::Launcher | Mode::Player) {
                let _ = self.app.emit("talk-release", ());
            }
            // A released: ends a hold (e.g. removing from Continue watching), or makes it a press.
            // Sent whatever the mode, so a hold never outlives the button.
            if b == Button::South {
                let _ = self.app.emit("select-release", ());
            }
            return;
        }
        match b {
            Button::South => self.dispatch(Action::Select),
            Button::East => self.dispatch(Action::Back),
            Button::West => self.extra(Extra::X),
            Button::North => self.extra(Extra::Y),
            Button::LeftTrigger => self.extra(Extra::Lb),
            Button::RightTrigger => self.extra(Extra::Rb),
            Button::LeftTrigger2 => self.extra(Extra::Lt),
            Button::RightTrigger2 => self.extra(Extra::Rt),
            _ => {}
        }
    }

    fn tick(&mut self, gilrs: &Gilrs, now: Instant) {
        if self.guide.pressed() {
            let home = self.home_button() == HomeButton::Guide;
            // Steam reacts to Guide too; keep it off the screen whenever the launcher is
            // in front or this press brings it there. In a game, Guide stays Steam's.
            if home || self.mode() != Mode::Game {
                guide_guard::after_guide(&self.app);
            }
            if home {
                self.dispatch(Action::Home);
            }
        }
        let from_stick = self.dpad.current().is_none() && self.stick.is_some();
        let dir = self.dpad.current().or(self.stick);
        if let Some(a) = self.repeater.update(dir, now) {
            if from_stick {
                self.dispatch_stick(a);
            } else {
                self.dispatch(a);
            }
        }
        if let Some(v) = self.volume.update(self.volume_held, now) {
            self.bridge.volume(v);
        }
        if self.mode() == Mode::Streaming {
            for (_, pad) in gilrs.gamepads() {
                let dx = mouse_step(pad.value(Axis::RightStickX));
                let dy = -mouse_step(pad.value(Axis::RightStickY));
                if dx != 0 || dy != 0 {
                    self.bridge.mouse(dx, dy);
                    break;
                }
            }
        }
    }

    /// While a timer or alarm rings over a web app, its overlay has the pad: A, B, X and Y (`b`)
    /// answer it, the rest does nothing. False when nothing rings there.
    fn answers_ring(&self, b: Option<ring::Button>) -> bool {
        if self.mode() != Mode::Streaming {
            return false;
        }
        match b {
            Some(b) => ring::press(&self.app, b),
            None => ring::ringing(&self.app),
        }
    }

    fn remote(&mut self, r: Remote) {
        match r {
            Remote::Action(a) => self.dispatch(a),
            Remote::Extra(e) => self.extra(e),
        }
    }

    fn dispatch(&mut self, a: Action) {
        let button = match a {
            Action::Select => Some(ring::Button::A),
            Action::Back => Some(ring::Button::B),
            _ => None,
        };
        if a != Action::Home && self.answers_ring(button) {
            return;
        }
        match self.mode() {
            // Home lands on Home, as from a streaming service's player.
            Mode::Player if a == Action::Home => {
                launcher::close(&self.app);
                let _ = self.app.emit("input", a);
            }
            Mode::Launcher | Mode::Player => {
                let _ = self.app.emit("input", a);
            }
            // Playing from the launcher's service screens, Home lands on Home rather than
            // on the details screen the player was opened from.
            Mode::Streaming if a == Action::Home => {
                let watching = crate::watch::active(&self.app);
                launcher::close(&self.app);
                if watching {
                    let _ = self.app.emit("input", a);
                }
            }
            Mode::Streaming => {
                if !cdp::send(&self.app, Cmd::Action(a)) {
                    self.bridge.action(a);
                }
            }
            // A retro game is saved and quit; a PC game keeps running behind the launcher.
            Mode::Game if a == Action::Home => {
                if !retro::quit(&self.app) {
                    games::to_launcher(&self.app);
                }
            }
            Mode::Game => {}
        }
    }

    /// The left stick moves like the D-pad, except that its up and down arrive told apart
    /// (players turn them into volume): as their own press in a browser's TV mode, and as
    /// "StickUp" / "StickDown" in the launcher.
    fn dispatch_stick(&mut self, a: Action) {
        if self.answers_ring(None) {
            return;
        }
        let up = match a {
            Action::Up => true,
            Action::Down => false,
            _ => return self.dispatch(a),
        };
        match self.mode() {
            Mode::Streaming => {
                if !cdp::send(&self.app, Cmd::Stick(a)) {
                    self.bridge.action(a);
                }
            }
            Mode::Launcher | Mode::Player => {
                let _ = self.app.emit("input", if up { "StickUp" } else { "StickDown" });
            }
            Mode::Game => {}
        }
    }

    fn extra(&mut self, e: Extra) {
        let button = match e {
            Extra::X => Some(ring::Button::X),
            Extra::Y => Some(ring::Button::Y),
            _ => None,
        };
        if self.answers_ring(button) {
            return;
        }
        match self.mode() {
            Mode::Launcher | Mode::Player => {
                if let Some(a) = launcher_action(e) {
                    self.dispatch(a);
                }
            }
            // RT stays a mouse click, for the cursor on the right stick.
            Mode::Streaming => {
                if matches!(e, Extra::Lt | Extra::Rt) || !cdp::send(&self.app, Cmd::Extra(e)) {
                    self.bridge.extra(e);
                }
            }
            Mode::Game => {}
        }
    }

    fn publish_pad(&self, gilrs: &Gilrs) {
        let name = gilrs
            .gamepads()
            .find(|(_, g)| g.is_connected())
            .map(|(_, g)| g.name().to_string());
        *self.app.state::<AppState>().pad.lock().unwrap() = name.clone();
        let _ = self.app.emit("pad", name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeater_fires_on_press_then_after_delay_then_at_rate() {
        let t0 = Instant::now();
        let mut r = Repeater::default();
        assert_eq!(r.update(Some(Action::Down), t0), Some(Action::Down));
        assert_eq!(r.update(Some(Action::Down), t0 + Duration::from_millis(100)), None);
        assert_eq!(r.update(Some(Action::Down), t0 + Duration::from_millis(399)), None);
        let t1 = t0 + Duration::from_millis(400);
        assert_eq!(r.update(Some(Action::Down), t1), Some(Action::Down));
        assert_eq!(r.update(Some(Action::Down), t1 + Duration::from_millis(100)), None);
        assert_eq!(r.update(Some(Action::Down), t1 + Duration::from_millis(120)), Some(Action::Down));
    }

    #[test]
    fn repeater_release_and_direction_change() {
        let t0 = Instant::now();
        let mut r = Repeater::default();
        r.update(Some(Action::Left), t0);
        assert_eq!(r.update(None, t0), None);
        assert_eq!(r.update(None, t0 + Duration::from_secs(1)), None);
        r.update(Some(Action::Left), t0);
        assert_eq!(r.update(Some(Action::Up), t0), Some(Action::Up));
    }

    #[test]
    fn stick_thresholds_and_hysteresis() {
        assert_eq!(stick_direction(0.5, 0.0, None), None);
        assert_eq!(stick_direction(0.7, 0.1, None), Some(Action::Right));
        assert_eq!(stick_direction(0.1, -0.9, None), Some(Action::Down));
        assert_eq!(stick_direction(0.1, 0.9, None), Some(Action::Up));
        // Below the press threshold but above the release one: keep going right.
        assert_eq!(stick_direction(0.5, 0.0, Some(Action::Right)), Some(Action::Right));
        assert_eq!(stick_direction(0.3, 0.0, Some(Action::Right)), None);
    }

    #[test]
    fn dpad_latest_press_wins() {
        let mut d = Dpad::default();
        d.set(Action::Up, true);
        d.set(Action::Right, true);
        assert_eq!(d.current(), Some(Action::Right));
        d.set(Action::Right, false);
        assert_eq!(d.current(), Some(Action::Up));
        d.set(Action::Up, false);
        assert_eq!(d.current(), None);
    }

    #[test]
    fn chord_fires_once_per_hold() {
        let mut c = Chord::default();
        assert!(!c.set(Button::Select, true));
        assert!(c.set(Button::Start, true));
        assert!(!c.set(Button::Start, true));
        assert!(!c.set(Button::South, true));
        assert!(!c.set(Button::Start, false));
        assert!(c.set(Button::Start, true));
    }

    #[test]
    fn center_buttons_are_volume_shoulders_switch_tabs_y_is_options_x_is_assistant() {
        assert_eq!(volume_for(Button::Select), Some(Volume::Down));
        assert_eq!(volume_for(Button::Start), Some(Volume::Up));
        assert_eq!(volume_for(Button::South), None);
        assert_eq!(launcher_action(Extra::Lb), Some(Action::TabPrev));
        assert_eq!(launcher_action(Extra::Lt), Some(Action::TriggerPrev));
        assert_eq!(launcher_action(Extra::Rb), Some(Action::TabNext));
        assert_eq!(launcher_action(Extra::Rt), Some(Action::TriggerNext));
        assert_eq!(launcher_action(Extra::Y), Some(Action::Options));
        assert_eq!(launcher_action(Extra::X), Some(Action::Assistant));
    }

    #[test]
    fn volume_repeats_while_held() {
        let t0 = Instant::now();
        let mut r = Repeater::default();
        assert_eq!(r.update(Some(Volume::Up), t0), Some(Volume::Up));
        assert_eq!(r.update(Some(Volume::Up), t0 + Duration::from_millis(200)), None);
        assert_eq!(r.update(Some(Volume::Up), t0 + Duration::from_millis(400)), Some(Volume::Up));
        assert_eq!(r.update(None, t0 + Duration::from_millis(450)), None);
    }

    #[test]
    fn home_trigger_mapping() {
        assert_eq!(home_trigger(HomeButton::Guide), None);
        assert_eq!(home_trigger(HomeButton::Select), Some(Button::Select));
        assert_eq!(home_trigger(HomeButton::L3), Some(Button::LeftThumb));
        assert_eq!(home_trigger(HomeButton::R3), Some(Button::RightThumb));
    }

    #[test]
    fn mouse_curve() {
        assert_eq!(mouse_step(0.1), 0);
        assert_eq!(mouse_step(1.0), MOUSE_MAX_SPEED as i32);
        assert_eq!(mouse_step(-1.0), -(MOUSE_MAX_SPEED as i32));
        assert!(mouse_step(0.5) < mouse_step(0.9));
    }
}
