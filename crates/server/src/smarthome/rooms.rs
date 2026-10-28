//! Rooms beyond the devices in them (layout.rs keeps which room each device is in and the rooms'
//! order and icons): each room's tasks, and what the assistant heard last. Tasks are kept in
//! rooms.json. The old launcher kept the house's screens here too; they're in devices.rs now,
//! one per TV PC instead of one fixed "tv".
//!
//! Voice follows a screen's room: "turn on the lights" said to the bedroom iPad means the
//! bedroom's lights (assistant).

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tvl_kit::store::JsonStore;
use tvl_kit::time::now_ms;
use tvl_kit::{Error, Result};

use super::{emit, Device, Kind, Layout};
use crate::ctx::Ctx;

const FILE: &str = "rooms.json";
/// A task ticked off stays on the list, struck through, this long.
const DONE_SHOWN_MS: i64 = 12 * 3600 * 1000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    /// None: for the whole house.
    #[serde(default)]
    pub room: Option<String>,
    pub title: String,
    pub created_at: i64,
    #[serde(default)]
    pub done_at: Option<i64>,
}

/// The last thing said or typed to the assistant, for the Smart Home's "heard" line.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Heard {
    pub said: String,
    /// The screen it was said to.
    pub by: String,
    /// The room a request naming none was about: the screen's.
    pub room: Option<String>,
    pub reply: String,
    pub at: i64,
}

/// rooms.json. The old launcher's file also has `screens`, which the import moves to
/// devices.json; it's ignored here.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    tasks: Vec<Task>,
}

pub struct Store {
    saved: JsonStore<Saved>,
    heard: Mutex<Option<Heard>>,
}

/// Old ticked-off tasks dropped.
fn tidy(saved: &mut Saved, now: i64) {
    saved.tasks.retain(|t| t.done_at.is_none_or(|d| now - d < DONE_SHOWN_MS));
}

impl Store {
    pub fn load(data_dir: &std::path::Path) -> Self {
        let saved = JsonStore::open(data_dir.join(FILE));
        let _ = saved.change(|s| {
            tidy(s, now_ms());
            Ok(())
        });
        Self { saved, heard: Mutex::new(None) }
    }

    /// Changes what's kept and saves it.
    fn change<T>(&self, f: impl FnOnce(&mut Saved) -> Result<T>) -> Result<T> {
        self.saved.change(|saved| {
            let out = f(saved)?;
            tidy(saved, now_ms());
            Ok(out)
        })
    }

    pub fn tasks(&self) -> Vec<Task> {
        let now = now_ms();
        self.saved.read(|s| s.tasks.iter().filter(|t| t.done_at.is_none_or(|d| now - d < DONE_SHOWN_MS)).cloned().collect())
    }

    pub fn heard(&self) -> Option<Heard> {
        self.heard.lock().clone()
    }
}

fn store(app: &Ctx) -> &Store {
    &app.smarthome.rooms
}

// ---------- Which devices are in a room ----------

/// Bulbs, strips and wall switches: what a room's All lights switch drives.
pub fn is_light(d: &Device) -> bool {
    matches!(d.kind, Kind::Light | Kind::Strip | Kind::Switch)
}

/// The devices in `room`, a group's members counted when the group is the one placed there. The
/// group devices themselves aren't listed: their members are.
pub fn devices_in<'a>(devices: &'a [Device], room: &str) -> Vec<&'a Device> {
    let placed_groups: Vec<&Device> = devices.iter().filter(|d| !d.members.is_empty() && d.room.as_deref() == Some(room)).collect();
    devices
        .iter()
        .filter(|d| d.members.is_empty())
        .filter(|d| d.room.as_deref() == Some(room) || placed_groups.iter().any(|g| g.members.contains(&d.id)))
        .collect()
}

/// The lights in `room`.
pub fn lights_in<'a>(devices: &'a [Device], room: &str) -> Vec<&'a Device> {
    devices_in(devices, room).into_iter().filter(|d| is_light(d)).collect()
}

/// Every room there is: the layout's in order, then those only named on a device, by name. A
/// room with nothing in it is one when it was made as one (it has an icon), not a name left over
/// from devices that moved out before rooms were kept (src/lib/smarthome.ts, roomList).
pub fn names(layout: &Layout, devices: &[Device]) -> Vec<String> {
    let used = |r: &String| devices.iter().any(|d| d.room.as_ref() == Some(r));
    let mut out: Vec<String> = layout.room_order.iter().filter(|r| used(r) || layout.room_icons.contains_key(*r)).cloned().collect();
    let mut extra: Vec<String> = devices.iter().filter_map(|d| d.room.clone()).filter(|r| !layout.room_order.contains(r)).collect();
    extra.sort();
    extra.dedup();
    out.extend(extra);
    out
}

// ---------- Rooms ----------

/// A room's name tidied, or why it can't be one.
fn room_name(name: &str) -> Result<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(Error::msg("Give the room a name"));
    }
    if name.chars().count() > 40 {
        return Err(Error::msg("That room name is too long"));
    }
    Ok(name)
}

/// A new empty room at the end (or the one that's there by that name), with an icon. An empty
/// icon is kept too (the UI then picks one from the name): it marks the room as made.
pub fn add_room(app: &Ctx, name: &str, icon: &str) -> Result<String> {
    let name = room_name(name)?;
    let sh = &app.smarthome;
    let mut layout = sh.layout.get();
    if !layout.room_order.contains(&name) {
        let devices = super::snapshot(app).devices;
        // Rooms only named on devices so far keep their place before the new one.
        layout.room_order = names(&layout, &devices);
        layout.room_order.retain(|r| r != &name);
        layout.room_order.push(name.clone());
    }
    if !icon.is_empty() || !layout.room_icons.contains_key(&name) {
        layout.room_icons.insert(name.clone(), icon.to_string());
    }
    sh.layout.set(layout)?;
    emit(app);
    Ok(name)
}

/// Renames a room everywhere: its devices, place, icon, routines, tasks and screens.
pub fn rename_room(app: &Ctx, from: &str, to: &str) -> Result<()> {
    let to = room_name(to)?;
    if to == from {
        return Ok(());
    }
    let sh = &app.smarthome;
    let mut layout = sh.layout.get();
    let devices = super::snapshot(app).devices;
    if names(&layout, &devices).contains(&to) {
        return Err(Error::msg(format!("There's already a room called {to}")));
    }
    // Rooms only named on devices so far keep their place too.
    layout.room_order = names(&layout, &devices).into_iter().map(|r| if r == from { to.clone() } else { r }).collect();
    for r in layout.rooms.values_mut().filter(|r| r.as_str() == from) {
        *r = to.clone();
    }
    if let Some(icon) = layout.room_icons.remove(from) {
        layout.room_icons.insert(to.clone(), icon);
    }
    sh.layout.set(layout)?;
    sh.scenes.move_room(from, Some(&to))?;
    move_room(store(app), from, Some(&to))?;
    app.devices.move_room(from, Some(&to))?;
    crate::clock::move_room(app, from, Some(&to))?;
    emit(app);
    Ok(())
}

/// Removes a room: its devices, routines and tasks are left with no room (the routines and
/// tasks become the whole house's), its screens aren't placed anywhere.
pub fn delete_room(app: &Ctx, name: &str) -> Result<()> {
    let sh = &app.smarthome;
    let mut layout = sh.layout.get();
    let devices = super::snapshot(app).devices;
    layout.room_order = names(&layout, &devices).into_iter().filter(|r| r != name).collect();
    layout.rooms.retain(|_, r| r != name);
    layout.room_icons.remove(name);
    sh.layout.set(layout)?;
    sh.scenes.move_room(name, None)?;
    move_room(store(app), name, None)?;
    app.devices.move_room(name, None)?;
    crate::clock::move_room(app, name, None)?;
    emit(app);
    Ok(())
}

fn move_room(st: &Store, from: &str, to: Option<&str>) -> Result<()> {
    st.change(|saved| {
        for t in saved.tasks.iter_mut().filter(|t| t.room.as_deref() == Some(from)) {
            t.room = to.map(str::to_string);
        }
        Ok(())
    })
}

// ---------- Tasks ----------

/// Adds a task, or retitles (and moves) one when `id` is given. Returns its id.
pub fn put_task(app: &Ctx, id: Option<&str>, room: Option<&str>, title: &str) -> Result<String> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        return Err(Error::msg("Write what needs doing"));
    }
    let room = room.map(str::to_string);
    let id = store(app).change(|saved| {
        if let Some(t) = id.and_then(|id| saved.tasks.iter_mut().find(|t| t.id == id)) {
            t.title = title;
            t.room = room;
            return Ok(t.id.clone());
        }
        let now = now_ms();
        let id = format!("task-{now}");
        saved.tasks.push(Task { id: id.clone(), room, title, created_at: now, done_at: None });
        Ok(id)
    })?;
    emit(app);
    Ok(id)
}

/// Ticks a task off, or back on.
pub fn task_done(app: &Ctx, id: &str, done: bool) -> Result<()> {
    store(app).change(|saved| {
        let t = saved.tasks.iter_mut().find(|t| t.id == id).ok_or_else(|| Error::msg("That task is gone"))?;
        t.done_at = done.then(now_ms);
        Ok(())
    })?;
    emit(app);
    Ok(())
}

pub fn delete_task(app: &Ctx, id: &str) -> Result<()> {
    store(app).change(|saved| {
        saved.tasks.retain(|t| t.id != id);
        Ok(())
    })?;
    emit(app);
    Ok(())
}

/// What the assistant was just asked, by which screen, about which room, and what it said.
pub fn heard(app: &Ctx, heard: Heard) {
    *store(app).heard.lock() = Some(heard);
    emit(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smarthome::Provider;

    fn dev(id: &str, room: Option<&str>, kind: Kind) -> Device {
        let mut d = Device::new(id.into(), Provider::Tuya, id.into(), kind);
        d.room = room.map(str::to_string);
        d
    }

    #[test]
    fn a_room_holds_its_devices_and_its_groups_members() {
        let mut group = dev("group:g", Some("Bedroom"), Kind::Light);
        group.members = vec!["b".into(), "c".into()];
        let devices = vec![
            dev("a", Some("Bedroom"), Kind::Light),
            dev("b", None, Kind::Light),
            dev("c", Some("Office"), Kind::Strip),
            dev("ac", Some("Bedroom"), Kind::Ac),
            dev("d", Some("Kitchen"), Kind::Switch),
            group,
        ];
        let ids = |v: Vec<&Device>| v.into_iter().map(|d| d.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(devices_in(&devices, "Bedroom")), ["a", "b", "c", "ac"]);
        assert_eq!(ids(lights_in(&devices, "Bedroom")), ["a", "b", "c"]);
        assert_eq!(ids(lights_in(&devices, "Kitchen")), ["d"]);
    }

    #[test]
    fn rooms_in_order_then_by_name() {
        let layout = Layout {
            room_order: vec!["Office".into(), "Empty".into(), "Left over".into()],
            room_icons: [("Empty".to_string(), "bath".to_string())].into(),
            ..Default::default()
        };
        let devices = vec![dev("a", Some("Bedroom"), Kind::Light), dev("b", Some("Office"), Kind::Light), dev("c", Some("Attic"), Kind::Light)];
        // "Empty" was made as a room; "Left over" is a name no device has any more.
        assert_eq!(names(&layout, &devices), ["Office", "Empty", "Attic", "Bedroom"]);
    }

    #[test]
    fn done_tasks_and_gone_screens_drop_off() {
        let now = 100 * 24 * 3600 * 1000;
        let task = |id: &str, done_at: Option<i64>| Task { id: id.into(), room: None, title: id.into(), created_at: 0, done_at };
        let mut saved = Saved { tasks: vec![task("open", None), task("just-done", Some(now - 1000)), task("old", Some(now - DONE_SHOWN_MS - 1))] };
        tidy(&mut saved, now);
        assert_eq!(saved.tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["open", "just-done"]);
        // (Screens left the file: devices.rs drops phones and tablets not seen for 60 days.)
    }

    #[test]
    fn rooms_json_round_trip() {
        // The old launcher's file, with its screens: they're ignored here (the import moved them).
        let saved: Saved = serde_json::from_str(
            r#"{"tasks":[{"id":"t","room":"Quarto","title":"Water plants","created_at":1}],"screens":[{"id":"tv","name":"TV","kind":"tv","room":"Quarto"}]}"#,
        )
        .unwrap();
        assert_eq!(saved.tasks[0].room.as_deref(), Some("Quarto"));
        assert!(!serde_json::to_string(&saved).unwrap().contains("screens"));
    }
}
