//! A TV PC as the companion sees it: what's on, its speakers, the remote's keys. From the old
//! launcher's `companion/tv.rs` and `state.rs`, with the TV PC named, since a house can have more
//! than one.

use serde::{Deserialize, Serialize};

/// Who owns the controller on a TV PC.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// The launcher UI is in front and receives every action.
    #[default]
    Launcher,
    /// A launched app (browser) is in front; the pad is bridged to keyboard/mouse.
    Streaming,
    /// A game is running; only Home is acted on.
    Game,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TvNow {
    /// The TV PC this is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    pub mode: Mode,
    /// The app in front, from apps.toml.
    pub app: Option<String>,
    pub app_name: Option<String>,
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub image: Option<String>,
    pub live: bool,
    /// The speakers, 0–100; None when Windows won't say.
    pub volume: Option<u8>,
    pub muted: bool,
}

/// What a companion played on the TV, shown until the TV moves on to another app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Showing {
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub live: bool,
}

/// The remote's keys, as the companion names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Select,
    Back,
    Home,
    /// Y: the focused item's options.
    Menu,
    PlayPause,
    ChannelUp,
    ChannelDown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tv_now_reads_as_the_companion_expects() {
        let now = TvNow { mode: Mode::Streaming, app: Some("netflix".into()), volume: Some(40), ..Default::default() };
        let v = serde_json::to_value(&now).unwrap();
        assert_eq!(v["mode"], "streaming");
        assert_eq!(v["volume"], 40);
        // The old launcher sent no device; a TV PC that says none still reads.
        assert!(v.get("device").is_none());
        let key: Key = serde_json::from_str(r#""PlayPause""#).unwrap();
        assert_eq!(key, Key::PlayPause);
    }
}
