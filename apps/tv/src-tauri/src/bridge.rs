//! Turns the pad into keyboard and mouse input while a streaming site is in front:
//! the services' web UIs don't understand gamepads.

use enigo::{Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};

use crate::input::{Action, Extra, Volume};

pub struct Bridge {
    enigo: Option<Enigo>,
}

impl Bridge {
    pub fn new() -> Self {
        let enigo = Enigo::new(&Settings::default())
            .map_err(|e| eprintln!("keyboard bridge disabled: {e}"))
            .ok();
        Self { enigo }
    }

    pub fn action(&mut self, a: Action) {
        let key = match a {
            Action::Up => Key::UpArrow,
            Action::Down => Key::DownArrow,
            Action::Left => Key::LeftArrow,
            Action::Right => Key::RightArrow,
            Action::Select => Key::Return,
            Action::Back => Key::Escape,
            Action::Home
            | Action::TabPrev
            | Action::TabNext
            | Action::TriggerPrev
            | Action::TriggerNext
            | Action::Options
            | Action::Assistant => return,
        };
        self.tap(key);
    }

    /// Media volume keys: Windows applies the step and shows its own volume overlay.
    pub fn volume(&mut self, v: Volume) {
        self.tap(match v {
            Volume::Up => Key::VolumeUp,
            Volume::Down => Key::VolumeDown,
        });
    }

    pub fn extra(&mut self, e: Extra) {
        let Some(enigo) = self.enigo.as_mut() else {
            return;
        };
        let _ = match e {
            Extra::X => enigo.key(Key::Space, Direction::Click),
            // Virtual-key code for F: players toggle fullscreen on it.
            Extra::Y => enigo.key(letter_f(), Direction::Click),
            Extra::Rb => enigo.key(Key::Tab, Direction::Click),
            Extra::Lb => enigo
                .key(Key::Shift, Direction::Press)
                .and_then(|_| enigo.key(Key::Tab, Direction::Click))
                .and_then(|_| enigo.key(Key::Shift, Direction::Release)),
            Extra::Rt => enigo.button(Button::Left, Direction::Click),
            Extra::Lt => Ok(()),
        };
    }

    pub fn mouse(&mut self, dx: i32, dy: i32) {
        if let Some(enigo) = self.enigo.as_mut() {
            let _ = enigo.move_mouse(dx, dy, Coordinate::Rel);
        }
    }

    fn tap(&mut self, key: Key) {
        if let Some(enigo) = self.enigo.as_mut() {
            let _ = enigo.key(key, Direction::Click);
        }
    }
}

#[cfg(windows)]
fn letter_f() -> Key {
    Key::Other(0x46)
}

#[cfg(not(windows))]
fn letter_f() -> Key {
    Key::Unicode('f')
}
