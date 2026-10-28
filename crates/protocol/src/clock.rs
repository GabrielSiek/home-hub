//! Timers and alarms going off, as the server tells the house and as each TV PC rings them. The
//! server decides what rings where; a TV PC makes the sound, quiets what's playing and shows the
//! ring over the app in front.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RingKind {
    Timer,
    Alarm,
}

/// One of today's things, on the alarm's screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chip {
    pub color: String,
    pub text: String,
}

/// What an alarm plays.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Sound {
    #[default]
    Chime,
    /// A playlist (or album) on Spotify, getting louder, on the TV PC that rings.
    Spotify { uri: String, name: String },
}

/// Where a timer or alarm rings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum RingTarget {
    /// The TV PCs in this room.
    Room(String),
    /// This TV PC.
    Device(String),
    /// Every TV PC that's on.
    Everywhere,
}

/// A timer or alarm going off.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ring {
    /// The timer's or the alarm's id.
    pub id: String,
    pub kind: RingKind,
    pub label: String,
    pub emoji: String,
    pub color: String,
    /// When it went off.
    pub since: i64,
    /// Timers: how long it was.
    pub total_ms: i64,
    /// Alarms: what else is on today.
    pub chips: Vec<Chip>,
    /// What else is going on: "Nothing is on in Quarto: ringing on Sala TV". A TV PC shows its own
    /// ("Netflix is paused until you stop the timer").
    pub note: String,
    #[serde(default)]
    pub sound: Sound,
    /// The TV PCs ringing it.
    #[serde(default)]
    pub targets: Vec<String>,
}

/// Everything ringing on one TV PC, sent whole on every change and on every reconnect, so a lost
/// "stop" can never leave a bell going. A newer generation replaces an older one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RingSet {
    pub generation: u64,
    pub rings: Vec<Ring>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_and_sounds_read_as_json() {
        let t: RingTarget = serde_json::from_str(r#"{"type":"room","id":"Quarto"}"#).unwrap();
        assert_eq!(t, RingTarget::Room("Quarto".into()));
        assert_eq!(serde_json::to_string(&RingTarget::Everywhere).unwrap(), r#"{"type":"everywhere"}"#);
        let s: Sound = serde_json::from_str(r#"{"type":"spotify","uri":"spotify:playlist:1","name":"Bom dia"}"#).unwrap();
        assert_eq!(s, Sound::Spotify { uri: "spotify:playlist:1".into(), name: "Bom dia".into() });
    }
}
