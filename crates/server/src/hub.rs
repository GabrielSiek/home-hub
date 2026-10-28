//! The companion Hub's devices: the six used most this week, or the user's picks. Every change to a
//! device counts, from a TV, a phone, a tablet or an automation (smarthome::control). Kept in
//! companion.json, so every phone and tablet shows the same Hub. From the old launcher's
//! `companion/hub.rs`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tvl_kit::store::JsonStore;
use tvl_kit::time::now_ms;
use tvl_kit::Result;

use crate::ctx::Ctx;

const FILE: &str = "companion.json";
const WEEK_MS: i64 = 7 * 24 * 3600 * 1000;
/// A brightness drag sends many commands: changes this close together are one use.
const SAME_USE_MS: i64 = 60 * 1000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HubMode {
    /// The six most used this week.
    #[default]
    Auto,
    /// The picks first, topped up with the most used.
    Picks,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HubPrefs {
    pub mode: HubMode,
    pub picks: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    hub: HubPrefs,
    /// When each device was used, unix ms, the last 7 days.
    #[serde(default)]
    uses: HashMap<String, Vec<i64>>,
}

/// What the companion's Hub reads.
#[derive(Debug, Clone, Serialize)]
pub struct Companion {
    pub hub: HubPrefs,
    /// Uses in the last 7 days, by device id.
    pub usage: HashMap<String, usize>,
}

pub struct Hub {
    saved: JsonStore<Saved>,
}

impl Hub {
    pub fn open(data_dir: &std::path::Path) -> Self {
        // Written on every device change: one line on disk.
        Self { saved: JsonStore::open_compact(data_dir.join(FILE)) }
    }
}

fn view(saved: &Saved, now: i64) -> Companion {
    let usage = saved
        .uses
        .iter()
        .map(|(id, at)| (id.clone(), at.iter().filter(|&&t| now - t < WEEK_MS).count()))
        .filter(|(_, n)| *n > 0)
        .collect();
    Companion { hub: saved.hub.clone(), usage }
}

pub fn snapshot(app: &Ctx) -> Companion {
    app.hub.saved.read(|s| view(s, now_ms()))
}

pub fn set_hub(app: &Ctx, mut hub: HubPrefs) -> Result<Companion> {
    hub.picks.dedup();
    hub.picks.truncate(6);
    let view = app.hub.saved.change(|saved| {
        saved.hub = hub;
        Ok(view(saved, now_ms()))
    })?;
    app.bus.emit("companion", &view);
    Ok(view)
}

/// Counts a change to device `id` towards this week's most used.
pub fn record_use(app: &Ctx, id: &str) {
    let now = now_ms();
    // A drag's later commands change nothing: not even a write.
    let same_use = app.hub.saved.read(|s| s.uses.get(id).and_then(|at| at.last()).is_some_and(|&t| now - t < SAME_USE_MS));
    if same_use {
        return;
    }
    let counted = app.hub.saved.change(|saved| Ok(add_use(&mut saved.uses, id, now).then(|| view(saved, now))));
    match counted {
        Ok(Some(view)) => app.bus.emit("companion", &view),
        Ok(None) => {}
        Err(e) => tracing::warn!("can't count the Hub's use of {id}: {e}"),
    }
}

/// False when it's the same use as the last one.
fn add_use(uses: &mut HashMap<String, Vec<i64>>, id: &str, now: i64) -> bool {
    for at in uses.values_mut() {
        at.retain(|&t| now - t < WEEK_MS);
    }
    uses.retain(|_, at| !at.is_empty());
    let at = uses.entry(id.to_string()).or_default();
    if at.last().is_some_and(|&t| now - t < SAME_USE_MS) {
        return false;
    }
    at.push(now);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_is_one_use_and_old_uses_drop_out() {
        let mut uses = HashMap::new();
        let t0 = 1_000_000_000_000;
        assert!(add_use(&mut uses, "lamp", t0));
        assert!(!add_use(&mut uses, "lamp", t0 + 5_000));
        assert!(add_use(&mut uses, "lamp", t0 + SAME_USE_MS + 1));
        assert!(add_use(&mut uses, "ac", t0 + 10));
        assert_eq!(uses["lamp"].len(), 2);
        assert!(add_use(&mut uses, "switch", t0 + WEEK_MS + 20));
        assert!(!uses.contains_key("ac"));
        assert_eq!(uses["lamp"].len(), 1);
    }
}
