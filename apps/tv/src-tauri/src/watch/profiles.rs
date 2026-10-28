//! Which of a service's profiles the launcher plays on. Learned rather than typed: when a
//! profile is picked on the service's "Who's watching?" (by watch.js, by its apps.toml name, or
//! by the user with the pad), the service's own id for it is kept, and Play opens that profile
//! directly next time, without the picker.
//!
//! Kept per launcher profile, for the launcher's own profiles to come: until then there is one,
//! `DEFAULT`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;

pub const DEFAULT: &str = "default";

/// A service profile, by the service's id for it and the name its picker shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Links {
    /// Launcher profile → service id → its profile there.
    #[serde(default)]
    by_profile: HashMap<String, HashMap<String, Link>>,
}

impl Links {
    pub fn load(dir: &Path) -> Self {
        fs::read_to_string(dir.join("stream-profiles.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        fs::write(dir.join("stream-profiles.json"), serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// The service profile to open for `profile`. An apps.toml name that doesn't fit the learned
    /// one means the user changed it: the picker is used again, and learns the new one.
    pub fn get(&self, profile: &str, service: &str, wanted: Option<&str>) -> Option<&Link> {
        let link = self.by_profile.get(profile)?.get(service)?;
        match wanted.map(str::trim).filter(|w| !w.is_empty()) {
            Some(w) if !fits(w, &link.name) => None,
            _ => Some(link),
        }
    }

    /// True when it changed anything.
    pub fn set(&mut self, profile: &str, service: &str, link: Link) -> bool {
        let services = self.by_profile.entry(profile.to_string()).or_default();
        if services.get(service) == Some(&link) {
            return false;
        }
        services.insert(service.to_string(), link);
        true
    }
}

/// The name as the picker shows it, or its start (as watch.js matches an apps.toml name).
fn fits(wanted: &str, name: &str) -> bool {
    name.trim().to_lowercase().starts_with(&wanted.trim().to_lowercase())
}

/// The address that makes `id` the service's current profile, on the services that have one.
/// Netflix's lands on its home page.
pub fn switch_url(service: &str, id: &str) -> Option<String> {
    let id_ok = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric());
    match service {
        "netflix" if id_ok => Some(format!("https://www.netflix.com/SwitchProfile?tkn={id}")),
        _ => None,
    }
}

/// The service's "Who's watching?", where the header's profile button sends the user to pick
/// (watch.js mirrors the picker paths in its SITES).
pub fn picker_url(service: &str) -> Option<&'static str> {
    match service {
        "netflix" => Some("https://www.netflix.com/ProfilesGate"),
        "hbomax" => Some("https://play.hbomax.com/profile-picker"),
        "disney" => Some("https://www.disneyplus.com/select-profile"),
        "prime" => Some("https://www.primevideo.com/profiles"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(id: &str, name: &str) -> Link {
        Link { id: id.into(), name: name.into() }
    }

    #[test]
    fn learned_link_unless_apps_toml_names_another() {
        let mut l = Links::default();
        assert!(l.set(DEFAULT, "netflix", link("ABC", "Gabriel")));
        assert!(!l.set(DEFAULT, "netflix", link("ABC", "Gabriel")));
        assert_eq!(l.get(DEFAULT, "netflix", None).unwrap().id, "ABC");
        assert_eq!(l.get(DEFAULT, "netflix", Some(" gabriel ")).unwrap().id, "ABC");
        assert_eq!(l.get(DEFAULT, "netflix", Some("Gab")).unwrap().id, "ABC");
        assert!(l.get(DEFAULT, "netflix", Some("Kids")).is_none());
        assert!(l.get(DEFAULT, "prime", None).is_none());
        assert!(l.get("other", "netflix", None).is_none());
    }

    #[test]
    fn netflix_switch_address() {
        assert_eq!(
            switch_url("netflix", "3JVPBY5WLBCAJOJ3T6UOIEAY7M").as_deref(),
            Some("https://www.netflix.com/SwitchProfile?tkn=3JVPBY5WLBCAJOJ3T6UOIEAY7M")
        );
        assert!(switch_url("netflix", "").is_none());
        assert!(switch_url("netflix", "a&b=c").is_none());
        assert!(switch_url("prime", "ABC").is_none());
    }

    #[test]
    fn pickers_only_where_there_are_profiles() {
        assert!(picker_url("netflix").is_some());
        assert!(picker_url("prime").is_some());
        assert!(picker_url("globoplay").is_none());
    }
}
