//! The house's screens: TV PCs (the desktop app) and the phones, tablets and laptops that open the
//! companion. The old launcher kept them in rooms.json beside the tasks, with one fixed "tv"; here
//! every TV PC is a screen of its own, kept in devices.json.
//!
//! The rule that matters most lives here: which TV PC a phone or tablet controls (`target`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tvl_kit::store::JsonStore;
use tvl_kit::time::{Clock, DAY};
use tvl_kit::{Error, Result};
use tvl_protocol::{Caps, Screen, ScreenKind};

use crate::bus::Bus;

pub const FILE: &str = "devices.json";
/// A phone or tablet not seen for this long drops off the list (TV PCs stay until forgotten).
const FORGET_SCREEN_MS: i64 = 60 * DAY;
/// More screens than any house has: a page making up new ids can't fill the file.
const MAX_SCREENS: usize = 64;

/// The old launcher's one TV, waiting for the first TV PC that pairs to take its place (its name
/// and room).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Unclaimed {
    pub name: String,
    pub room: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    screens: Vec<Screen>,
    #[serde(default)]
    unclaimed_tv: Option<Unclaimed>,
}

/// Why a call from a phone or tablet has no TV PC to go to.
#[derive(Debug, Clone, PartialEq)]
pub enum NoTarget {
    /// No TV PC is set up yet, or several and this screen has neither a room nor a pick.
    Pick,
    /// The TV PC it controls is off (named).
    Off(String),
}

pub struct Devices {
    store: JsonStore<Saved>,
    /// Companion pages open now, by screen id (one tablet may have the page open twice).
    sockets: Mutex<HashMap<String, usize>>,
    /// TV PCs connected now.
    agents: Mutex<HashSet<String>>,
    /// The TV PC each open screen was last told it controls, to tell it again only on a change.
    told: Mutex<HashMap<String, Option<String>>>,
    clock: Arc<dyn Clock>,
    bus: Bus,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn tidy_name(name: &str) -> String {
    name.trim().chars().take(40).collect()
}

impl Devices {
    pub fn open(dir: &std::path::Path, clock: Arc<dyn Clock>, bus: Bus) -> Self {
        let store = JsonStore::open(dir.join(FILE));
        let me = Self {
            store,
            sockets: Mutex::default(),
            agents: Mutex::default(),
            told: Mutex::default(),
            clock,
            bus,
        };
        let now = me.clock.now_ms();
        let _ = me.store.change(|s| {
            s.screens.retain(|x| x.is_tv() || now - x.last_seen < FORGET_SCREEN_MS);
            Ok(())
        });
        me
    }

    /// Every screen, online worked out now.
    pub fn list(&self) -> Vec<Screen> {
        let sockets = self.sockets.lock();
        let agents = self.agents.lock();
        let mut list = self.store.read(|s| s.screens.clone());
        for s in &mut list {
            s.online = if s.is_tv() { agents.contains(&s.id) } else { sockets.get(&s.id).is_some_and(|n| *n > 0) };
        }
        list
    }

    pub fn get(&self, id: &str) -> Option<Screen> {
        self.list().into_iter().find(|s| s.id == id)
    }

    pub fn tvs(&self) -> Vec<Screen> {
        self.list().into_iter().filter(Screen::is_tv).collect()
    }

    /// A companion page is open somewhere (the smart home polls often then).
    pub fn any_page_open(&self) -> bool {
        self.sockets.lock().values().any(|n| *n > 0)
    }

    pub fn unclaimed_tv(&self) -> Option<Unclaimed> {
        self.store.read(|s| s.unclaimed_tv.clone())
    }

    /// A phone, tablet or laptop opened the companion: listed (under the name it gives the first
    /// time), and seen now.
    pub fn hello(&self, id: &str, kind: ScreenKind, name: &str) -> Result<Screen> {
        if !valid_id(id) || kind == ScreenKind::Tv {
            return Err(Error::msg("Not a screen id"));
        }
        let now = self.clock.now_ms();
        let screen = self.store.change(|s| {
            if let Some(x) = s.screens.iter_mut().find(|x| x.id == id) {
                if x.is_tv() {
                    return Err(Error::msg("Not a screen id"));
                }
                x.last_seen = now;
                return Ok(x.clone());
            }
            if s.screens.iter().filter(|x| !x.is_tv()).count() >= MAX_SCREENS {
                return Err(Error::msg("Too many screens: forget some in Settings"));
            }
            let name = tidy_name(name);
            let screen = Screen {
                id: id.to_string(),
                name: if name.is_empty() { "Screen".into() } else { name },
                kind,
                room: None,
                last_seen: now,
                online: false,
                caps: None,
                picks: None,
                this: false,
            };
            s.screens.push(screen.clone());
            Ok(screen)
        })?;
        self.changed();
        Ok(screen)
    }

    /// A TV PC joined the house (its pairing was approved). The first one takes the old
    /// launcher's TV's name and room.
    pub fn add_tv(&self, id: &str, name: &str) -> Result<Screen> {
        if !valid_id(id) {
            return Err(Error::msg("Not a TV PC id"));
        }
        let now = self.clock.now_ms();
        let screen = self.store.change(|s| {
            if let Some(x) = s.screens.iter().find(|x| x.id == id) {
                return Ok(x.clone());
            }
            let claimed = s.unclaimed_tv.take();
            let name = tidy_name(name);
            let screen = Screen {
                id: id.to_string(),
                name: claimed.as_ref().map(|c| c.name.clone()).filter(|n| !n.is_empty()).unwrap_or(if name.is_empty() {
                    "TV".into()
                } else {
                    name
                }),
                kind: ScreenKind::Tv,
                room: claimed.and_then(|c| c.room),
                last_seen: now,
                online: false,
                caps: None,
                picks: None,
                this: false,
            };
            s.screens.push(screen.clone());
            Ok(screen)
        })?;
        self.changed();
        Ok(screen)
    }

    /// A TV PC connected (with what it can do) or went away.
    pub fn tv_online(&self, id: &str, online: bool, caps: Option<Caps>) {
        if online {
            self.agents.lock().insert(id.to_string());
        } else {
            self.agents.lock().remove(id);
        }
        let now = self.clock.now_ms();
        let _ = self.store.change(|s| {
            if let Some(x) = s.screens.iter_mut().find(|x| x.id == id) {
                x.last_seen = now;
                if caps.is_some() {
                    x.caps = caps;
                }
            }
            Ok(())
        });
        self.changed();
    }

    /// A companion page opened (true) or closed its event socket.
    pub fn socket(&self, id: &str, open: bool) {
        {
            let mut c = self.sockets.lock();
            let n = c.entry(id.to_string()).or_default();
            *n = if open { *n + 1 } else { n.saturating_sub(1) };
        }
        if !open {
            let now = self.clock.now_ms();
            let _ = self.store.change(|s| {
                if let Some(x) = s.screens.iter_mut().find(|x| x.id == id) {
                    x.last_seen = now;
                }
                Ok(())
            });
        }
        self.changed();
    }

    /// Renames a screen or moves it to another room (None: not in one).
    pub fn put(&self, id: &str, name: &str, room: Option<&str>) -> Result<()> {
        let name = tidy_name(name);
        if name.is_empty() {
            return Err(Error::msg("Give the screen a name"));
        }
        self.store.change(|s| {
            let x = s.screens.iter_mut().find(|x| x.id == id).ok_or_else(|| Error::msg("That screen is gone"))?;
            x.name = name;
            x.room = room.map(str::to_string);
            Ok(())
        })?;
        self.changed();
        Ok(())
    }

    /// Takes a screen off the list (a phone or tablet is back the next time it opens the
    /// companion). Returns it, so a TV PC's pairing can be revoked too.
    pub fn forget(&self, id: &str) -> Result<Screen> {
        let gone = self.store.change(|s| {
            let at = s.screens.iter().position(|x| x.id == id).ok_or_else(|| Error::msg("That screen is gone"))?;
            let gone = s.screens.remove(at);
            // Screens that picked it control their room's again.
            for x in &mut s.screens {
                if x.picks.as_deref() == Some(id) {
                    x.picks = None;
                }
            }
            Ok(gone)
        })?;
        self.changed();
        Ok(gone)
    }

    /// The TV PC `screen` chose to control; None goes back to its room's.
    pub fn pick(&self, screen: &str, device: Option<&str>) -> Result<()> {
        if let Some(d) = device {
            if !self.store.read(|s| s.screens.iter().any(|x| x.id == d && x.is_tv())) {
                return Err(Error::msg("That TV PC is gone"));
            }
        }
        self.store.change(|s| {
            let x = s.screens.iter_mut().find(|x| x.id == screen).ok_or_else(|| Error::msg("Open the companion again"))?;
            x.picks = device.map(str::to_string);
            Ok(())
        })?;
        self.changed();
        Ok(())
    }

    /// A room renamed (Some) or deleted (None): its screens follow.
    pub fn move_room(&self, from: &str, to: Option<&str>) -> Result<()> {
        self.store.change(|s| {
            for x in s.screens.iter_mut().filter(|x| x.room.as_deref() == Some(from)) {
                x.room = to.map(str::to_string);
            }
            if let Some(u) = s.unclaimed_tv.as_mut().filter(|u| u.room.as_deref() == Some(from)) {
                u.room = to.map(str::to_string);
            }
            Ok(())
        })?;
        self.changed();
        Ok(())
    }

    /// The TV PC `screen` controls, online or not: its pick, else its room's TV PC, else, for a
    /// screen in no room, the house's only TV PC. Never another room's PC by surprise.
    pub fn followed(&self, screen: Option<&str>) -> Option<String> {
        let list = self.store.read(|s| s.screens.clone());
        let me = screen.and_then(|id| list.iter().find(|x| x.id == id));
        let tvs: Vec<&Screen> = list.iter().filter(|x| x.is_tv()).collect();
        if let Some(me) = me {
            // A TV PC's own UI: itself.
            if me.is_tv() {
                return Some(me.id.clone());
            }
            if let Some(p) = me.picks.as_deref().filter(|p| tvs.iter().any(|t| t.id == *p)) {
                return Some(p.to_string());
            }
            if let Some(room) = me.room.as_deref() {
                return tvs.iter().find(|t| t.room.as_deref() == Some(room)).map(|t| t.id.clone());
            }
        }
        (tvs.len() == 1).then(|| tvs[0].id.clone())
    }

    /// Where a call from `screen` goes: `explicit` when the page named a TV PC for this call,
    /// else the one it follows. It must be online.
    pub fn target(&self, screen: Option<&str>, explicit: Option<&str>) -> std::result::Result<String, NoTarget> {
        let id = match explicit {
            Some(e) => Some(e.to_string()),
            None => self.followed(screen),
        };
        let Some(id) = id else { return Err(NoTarget::Pick) };
        match self.get(&id) {
            Some(tv) if tv.is_tv() && tv.online => Ok(id),
            Some(tv) if tv.is_tv() => Err(NoTarget::Off(tv.name)),
            _ => Err(NoTarget::Pick),
        }
    }

    /// Leaves the old launcher's TV for the first TV PC that pairs (import).
    pub fn set_unclaimed_tv(&self, tv: Option<Unclaimed>) -> Result<()> {
        self.store.change(|s| {
            s.unclaimed_tv = tv;
            Ok(())
        })
    }

    /// Tells every screen what changed, and each open companion page whose TV PC changed.
    fn changed(&self) {
        let list = self.list();
        self.bus.emit("devices", &list);
        let open: Vec<String> = self.sockets.lock().iter().filter(|(_, n)| **n > 0).map(|(id, _)| id.clone()).collect();
        for id in open {
            let now = self.followed(Some(&id));
            let mut told = self.told.lock();
            if told.get(&id) != Some(&now) {
                told.insert(id.clone(), now.clone());
                drop(told);
                let tv = now.as_deref().and_then(|d| list.iter().find(|s| s.id == d));
                self.bus.emit_to(
                    &id,
                    "follow",
                    &serde_json::json!({ "device": now, "name": tv.map(|t| t.name.clone()), "online": tv.is_some_and(|t| t.online) }),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tvl_kit::time::FakeClock;

    fn devices() -> (Devices, std::path::PathBuf) {
        let dir = tvl_kit::testing::temp_dir("devices");
        (Devices::open(&dir, Arc::new(FakeClock::new(100 * DAY)), Bus::new()), dir)
    }

    #[test]
    fn a_screen_controls_its_rooms_tv_and_never_another_rooms() {
        let (d, dir) = devices();
        d.add_tv("pc-quarto", "Quarto TV").unwrap();
        d.add_tv("pc-sala", "Sala TV").unwrap();
        d.put("pc-quarto", "Quarto TV", Some("Quarto")).unwrap();
        d.put("pc-sala", "Sala TV", Some("Sala")).unwrap();
        d.hello("ipad", ScreenKind::Tablet, "iPad").unwrap();
        d.put("ipad", "iPad", Some("Quarto")).unwrap();
        d.tv_online("pc-sala", true, None);

        // Its room's TV is off: say so, don't drive the living room's.
        assert_eq!(d.followed(Some("ipad")).as_deref(), Some("pc-quarto"));
        assert_eq!(d.target(Some("ipad"), None), Err(NoTarget::Off("Quarto TV".into())));
        d.tv_online("pc-quarto", true, None);
        assert_eq!(d.target(Some("ipad"), None).as_deref(), Ok("pc-quarto"));
        // A pick wins; an explicit target for one call wins over both.
        d.pick("ipad", Some("pc-sala")).unwrap();
        assert_eq!(d.target(Some("ipad"), None).as_deref(), Ok("pc-sala"));
        assert_eq!(d.target(Some("ipad"), Some("pc-quarto")).as_deref(), Ok("pc-quarto"));
        // A phone in no room, with two TVs: it has to choose.
        d.hello("phone", ScreenKind::Phone, "Phone").unwrap();
        assert_eq!(d.target(Some("phone"), None), Err(NoTarget::Pick));
        // A TV PC's own UI controls itself.
        assert_eq!(d.followed(Some("pc-sala")).as_deref(), Some("pc-sala"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn with_one_tv_a_screen_in_no_room_controls_it() {
        let (d, dir) = devices();
        assert_eq!(d.target(Some("phone"), None), Err(NoTarget::Pick));
        d.add_tv("pc-quarto", "TV").unwrap();
        d.tv_online("pc-quarto", true, None);
        d.hello("phone", ScreenKind::Phone, "Phone").unwrap();
        assert_eq!(d.target(Some("phone"), None).as_deref(), Ok("pc-quarto"));
        // Even a page that hasn't said hello yet.
        assert_eq!(d.target(None, None).as_deref(), Ok("pc-quarto"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_first_tv_pc_takes_the_old_tvs_place() {
        let (d, dir) = devices();
        d.set_unclaimed_tv(Some(Unclaimed { name: "TV".into(), room: Some("Quarto".into()) })).unwrap();
        let first = d.add_tv("pc-a", "DESKTOP-A").unwrap();
        assert_eq!((first.name.as_str(), first.room.as_deref()), ("TV", Some("Quarto")));
        let second = d.add_tv("pc-b", "DESKTOP-B").unwrap();
        assert_eq!((second.name.as_str(), second.room), ("DESKTOP-B", None));
        assert!(d.unclaimed_tv().is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn screen_ids_are_checked_and_capped() {
        let (d, dir) = devices();
        assert!(d.hello("", ScreenKind::Phone, "x").is_err());
        assert!(d.hello("../etc", ScreenKind::Phone, "x").is_err());
        assert!(d.hello("tvlike", ScreenKind::Tv, "x").is_err(), "a page can't claim to be a TV PC");
        for i in 0..MAX_SCREENS {
            d.hello(&format!("s{i}"), ScreenKind::Phone, "x").unwrap();
        }
        assert!(d.hello("one-more", ScreenKind::Phone, "x").is_err());
        // One already listed still says hello.
        assert!(d.hello("s3", ScreenKind::Phone, "x").is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn forgetting_a_tv_frees_the_screens_that_picked_it_and_rooms_move() {
        let (d, dir) = devices();
        d.add_tv("pc", "TV").unwrap();
        d.hello("ipad", ScreenKind::Tablet, "iPad").unwrap();
        d.pick("ipad", Some("pc")).unwrap();
        d.put("ipad", "iPad", Some("Quarto")).unwrap();
        d.move_room("Quarto", Some("Bedroom")).unwrap();
        assert_eq!(d.get("ipad").unwrap().room.as_deref(), Some("Bedroom"));
        d.forget("pc").unwrap();
        assert_eq!(d.get("ipad").unwrap().picks, None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
