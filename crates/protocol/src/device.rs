//! The house's screens: TV PCs running the desktop app, and the phones, tablets and laptops that
//! open the companion. Same shape as the old launcher's `rooms.json` screens and the Smart Home
//! snapshot's `screens`, so both UIs read them as before; the new fields are optional.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScreenKind {
    /// A TV PC with the desktop app (the old launcher's one fixed "tv").
    Tv,
    Tablet,
    Phone,
    Computer,
}

/// What a TV PC can do for the rest of the house.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Caps {
    /// Takes remote keys and text.
    #[serde(default)]
    pub remote: bool,
    /// Has speakers whose volume can be set.
    #[serde(default)]
    pub volume: bool,
    /// Opens apps and plays titles.
    #[serde(default)]
    pub apps: bool,
    /// Has a microphone for the assistant.
    #[serde(default)]
    pub mic: bool,
    /// Speaks the assistant's replies.
    #[serde(default)]
    pub voice: bool,
    /// Rings timers and alarms.
    #[serde(default)]
    pub ring: bool,
    /// The name of its Spotify Connect device, when the Spotify app runs there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spotify_device: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Screen {
    pub id: String,
    pub name: String,
    pub kind: ScreenKind,
    /// The room it's in; None for one that moves around (a phone) or isn't placed yet.
    #[serde(default)]
    pub room: Option<String>,
    /// When it was last connected, unix ms.
    #[serde(default)]
    pub last_seen: i64,
    /// Connected right now. Worked out each time, never read back from a file.
    #[serde(skip_deserializing, default)]
    pub online: bool,
    /// TV PCs: what they can do (None until one has connected).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caps: Option<Caps>,
    /// Phones, tablets, laptops: the TV PC this screen chose to control. None: its room's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picks: Option<String>,
    /// True on the entry of the TV PC whose app is showing the list (set by that PC's agent, so
    /// its UI can say "this TV"). The server never sets it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub this: bool,
}

impl Screen {
    pub fn is_tv(&self) -> bool {
        self.kind == ScreenKind::Tv
    }
}

/// How an integration is doing, for status lines and Settings: the old launcher's
/// `smarthome::Link`, which calendar, YouTube, Spotify, Twitch and photos also used.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ServiceLink {
    /// Keys are set.
    pub configured: bool,
    pub signed_in: bool,
    /// The last fetch worked.
    pub connected: bool,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_rooms_json_screens_still_read() {
        let s: Screen =
            serde_json::from_str(r#"{"id":"tv","name":"TV","kind":"tv","room":"Quarto","last_seen":0,"online":true}"#).unwrap();
        assert!(s.is_tv());
        assert_eq!(s.room.as_deref(), Some("Quarto"));
        // Online is worked out each time, never read back.
        assert!(!s.online);
        // Nothing new is written for an old-style screen.
        let back = serde_json::to_value(&s).unwrap();
        assert!(back.get("caps").is_none() && back.get("picks").is_none() && back.get("this").is_none());
    }
}
