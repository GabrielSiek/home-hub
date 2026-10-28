use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;

const MAX_OPENED: usize = 20;
const MAX_SEARCHES: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Opened {
    pub app_id: String,
    pub title: Option<String>,
    pub backdrop: Option<String>,
    /// Unix time in milliseconds.
    pub at: i64,
}

/// What was opened from the launcher. The streaming services expose no watch-progress API,
/// so this local list stands in for "continue watching".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    #[serde(default)]
    pub opened: Vec<Opened>,
    #[serde(default)]
    pub searches: Vec<String>,
}

impl History {
    pub fn load(dir: &Path) -> Self {
        fs::read_to_string(dir.join("history.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        fs::write(dir.join("history.json"), serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn record_open(&mut self, item: Opened) {
        self.opened
            .retain(|o| !(o.app_id == item.app_id && o.title == item.title));
        self.opened.insert(0, item);
        self.opened.truncate(MAX_OPENED);
    }

    pub fn forget_opened(&mut self, app_id: &str, title: Option<&str>) {
        self.opened
            .retain(|o| !(o.app_id == app_id && o.title.as_deref() == title));
    }

    pub fn record_search(&mut self, query: &str) {
        let q = query.trim();
        if q.is_empty() {
            return;
        }
        self.searches.retain(|s| !s.eq_ignore_ascii_case(q));
        self.searches.insert(0, q.to_string());
        self.searches.truncate(MAX_SEARCHES);
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened(app: &str, title: Option<&str>, at: i64) -> Opened {
        Opened {
            app_id: app.into(),
            title: title.map(Into::into),
            backdrop: None,
            at,
        }
    }

    #[test]
    fn reopening_moves_item_to_front() {
        let mut h = History::default();
        h.record_open(opened("netflix", Some("Dark"), 1));
        h.record_open(opened("hbomax", None, 2));
        h.record_open(opened("netflix", Some("Dark"), 3));
        assert_eq!(h.opened.len(), 2);
        assert_eq!(h.opened[0], opened("netflix", Some("Dark"), 3));
    }

    #[test]
    fn same_app_different_titles_are_separate() {
        let mut h = History::default();
        h.record_open(opened("netflix", Some("Dark"), 1));
        h.record_open(opened("netflix", None, 2));
        assert_eq!(h.opened.len(), 2);
    }

    #[test]
    fn opened_is_capped() {
        let mut h = History::default();
        for i in 0..30 {
            h.record_open(opened("netflix", Some(&i.to_string()), i));
        }
        assert_eq!(h.opened.len(), MAX_OPENED);
        assert_eq!(h.opened[0].at, 29);
    }

    #[test]
    fn searches_dedupe_case_insensitively() {
        let mut h = History::default();
        h.record_search("Dune");
        h.record_search("  ");
        h.record_search("arcane");
        h.record_search("dune");
        assert_eq!(h.searches, vec!["dune", "arcane"]);
    }
}
