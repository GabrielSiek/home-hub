//! How the Smart Home dashboard is arranged (smarthome-layout.json): the tiles' order and
//! sizes, which are hidden, names and rooms the user gave devices, the rooms themselves (their
//! order and icons), and groups of devices that act as one. Names, rooms and groups are applied
//! to the device list itself, so the assistant and scenes see them too. Each room's screen shows
//! its tiles in the same order, so moving a tile there orders it among that room's.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use parking_lot::Mutex;

use serde::{Deserialize, Serialize};

use super::{Device, Kind};
use tvl_kit::Result;

const FILE: &str = "smarthome-layout.json";
pub const GROUP_PREFIX: &str = "group:";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    /// Tile ids (devices and "group:<id>") in dashboard order. Tiles not listed come after.
    pub order: Vec<String>,
    pub hidden: Vec<String>,
    /// Tile id → columns it spans (1 or 2); the UI picks when a tile isn't here.
    pub sizes: BTreeMap<String, u8>,
    /// Tile id → the name the user gave it.
    pub names: BTreeMap<String, String>,
    /// Tile id → room.
    pub rooms: BTreeMap<String, String>,
    /// The rooms in order. A room listed here is there even with nothing in it yet; rooms only
    /// named on a device come after, by name.
    pub room_order: Vec<String>,
    /// Room → its icon (sofa, bed, pot, desk, bath, plant…); the UI picks when it isn't here.
    pub room_icons: BTreeMap<String, String>,
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub name: String,
    /// Device ids. A device in a group has no tile of its own.
    pub members: Vec<String>,
}

pub struct Store {
    path: PathBuf,
    layout: Mutex<Layout>,
}

impl Store {
    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join(FILE);
        let layout = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            path,
            layout: Mutex::new(layout),
        }
    }

    pub fn get(&self) -> Layout {
        self.layout.lock().clone()
    }

    pub fn set(&self, layout: Layout) -> Result<()> {
        fs::write(&self.path, serde_json::to_string_pretty(&layout)?)?;
        *self.layout.lock() = layout;
        Ok(())
    }
}

/// The user's names and rooms on the devices, plus one device per group.
pub fn apply(layout: &Layout, mut devices: Vec<Device>) -> Vec<Device> {
    for d in &mut devices {
        if let Some(n) = layout.names.get(&d.id) {
            d.name = n.clone();
        }
        if let Some(r) = layout.rooms.get(&d.id) {
            d.room = Some(r.clone());
        }
    }
    let groups: Vec<Device> = layout
        .groups
        .iter()
        .filter_map(|g| group_device(g, layout.rooms.get(&format!("{GROUP_PREFIX}{}", g.id)), &devices))
        .collect();
    devices.extend(groups);
    devices
}

/// A group as one device: on when any member is, its brightness the average of the lit ones,
/// a capability only when every member has it.
fn group_device(g: &Group, room: Option<&String>, devices: &[Device]) -> Option<Device> {
    let members: Vec<&Device> = g.members.iter().filter_map(|m| devices.iter().find(|d| &d.id == m)).collect();
    let first = members.first()?;
    let kind = if members.iter().all(|d| d.kind == first.kind) {
        first.kind
    } else if members.iter().all(|d| matches!(d.kind, Kind::Light | Kind::Strip)) {
        Kind::Light
    } else {
        Kind::Switch
    };
    let mut d = Device::new(format!("{GROUP_PREFIX}{}", g.id), first.provider, g.name.clone(), kind);
    d.room = room.cloned().or_else(|| first.room.clone());
    let live: Vec<&&Device> = members.iter().filter(|m| m.online).collect();
    d.online = !live.is_empty();
    if members.iter().all(|m| m.on.is_some()) {
        d.on = Some(live.iter().any(|m| m.on == Some(true)));
    }
    let lit: Vec<u8> = live.iter().filter(|m| m.on == Some(true)).filter_map(|m| m.brightness).collect();
    if members.iter().all(|m| m.brightness.is_some()) {
        let all: Vec<u8> = live.iter().filter_map(|m| m.brightness).collect();
        let pool = if lit.is_empty() { &all } else { &lit };
        d.brightness = (!pool.is_empty()).then(|| (pool.iter().map(|&b| u32::from(b)).sum::<u32>() / pool.len() as u32) as u8);
    }
    if members.iter().all(|m| m.color_temp.is_some()) {
        d.color_temp = first.color_temp;
    }
    if members.iter().all(|m| m.color.is_some()) {
        d.color = first.color;
        d.color_mode = first.color_mode;
    }
    d.via = Some(format!("Group · {} devices", members.len()));
    d.members = members.iter().map(|m| m.id.clone()).collect();
    Some(d)
}

/// The group's member ids, for a "group:<id>" device id.
pub fn members(layout: &Layout, id: &str) -> Option<Vec<String>> {
    let gid = id.strip_prefix(GROUP_PREFIX)?;
    layout.groups.iter().find(|g| g.id == gid).map(|g| g.members.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smarthome::Provider;

    fn lamp(id: &str, on: bool, br: u8) -> Device {
        let mut d = Device::new(id.into(), Provider::Tuya, id.into(), Kind::Light);
        d.on = Some(on);
        d.brightness = Some(br);
        d
    }

    #[test]
    fn names_rooms_and_groups() {
        let layout = Layout {
            names: [("a".to_string(), "Desk".to_string())].into(),
            rooms: [("a".to_string(), "Office".to_string()), ("group:g".to_string(), "Living".to_string())].into(),
            groups: vec![Group { id: "g".into(), name: "Lamps".into(), members: vec!["a".into(), "b".into(), "gone".into()] }],
            ..Default::default()
        };
        let out = apply(&layout, vec![lamp("a", true, 40), lamp("b", false, 80)]);
        assert_eq!(out[0].name, "Desk");
        assert_eq!(out[0].room.as_deref(), Some("Office"));
        let g = &out[2];
        assert_eq!(g.id, "group:g");
        assert_eq!(g.room.as_deref(), Some("Living"));
        assert_eq!(g.on, Some(true));
        // Only the lit lamp counts towards the brightness.
        assert_eq!(g.brightness, Some(40));
        assert_eq!(g.members, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(members(&layout, "group:g").unwrap().len(), 3);
    }

    #[test]
    fn mixed_group_is_a_switch() {
        let mut sw = Device::new("s".into(), Provider::Ewelink, "s".into(), Kind::Switch);
        sw.on = Some(false);
        let layout = Layout {
            groups: vec![Group { id: "g".into(), name: "All".into(), members: vec!["a".into(), "s".into()] }],
            ..Default::default()
        };
        let out = apply(&layout, vec![lamp("a", false, 50), sw]);
        let g = out.last().unwrap();
        assert_eq!(g.kind, Kind::Switch);
        assert_eq!(g.brightness, None);
        assert_eq!(g.on, Some(false));
    }

    #[test]
    fn old_file_without_fields() {
        let l: Layout = serde_json::from_str(r#"{"hidden":["x"]}"#).unwrap();
        assert_eq!(l.hidden, vec!["x".to_string()]);
        assert!(l.groups.is_empty());
    }
}
