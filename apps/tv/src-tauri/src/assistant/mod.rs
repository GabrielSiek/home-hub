//! The assistant's parts on this TV PC: its ears (the microphone, voice.rs), its voice (Kokoro,
//! speech.rs), the PC's volume and media keys (system.rs), and the tools that act on this TV
//! (tools.rs). The assistant itself (the model, the house's tools, the conversation) runs on the
//! home server: the TV UI's `assistant_ask` goes there, and the server sends this TV the calls of
//! its tools.

pub mod speech;
pub mod system;
pub mod tools;
pub mod voice;

use serde::Serialize;

/// Something the assistant did, shown under its reply.
#[derive(Debug, Clone, Serialize)]
pub struct Card {
    /// An icon name the UI knows.
    pub icon: String,
    pub title: String,
    pub subtitle: String,
    /// Drawn in the accent color: something started or turned on.
    pub accent: bool,
    /// A timer or alarm the card follows live, and opens the Clock screen on: "timer:<id>",
    /// "alarm:<id>".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock: Option<String>,
}

impl Card {
    pub fn new(icon: &str, title: &str, subtitle: &str) -> Self {
        Self {
            icon: icon.into(),
            title: title.into(),
            subtitle: subtitle.into(),
            accent: false,
            clock: None,
        }
    }
}

/// A movie, series or game the reply is about: a poster the user can pick to watch or play it.
#[derive(Debug, Clone, Serialize)]
pub struct Media {
    /// "movie:<tmdb id>", "tv:<tmdb id>", or a game id ("steam:<appid>", "epic:<name>").
    pub id: String,
    pub kind: MediaKind,
    pub title: String,
    /// Year and type, or store and play time.
    pub subtitle: String,
    /// Poster candidates, best first: URLs or file paths.
    pub images: Vec<String>,
    pub backdrop: Option<String>,
    /// Movies and series: the user's apps that stream it, best first.
    pub apps: Vec<String>,
    /// Games: ready to play; otherwise picking it offers to install it.
    pub installed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Movie,
    Tv,
    Game,
}

/// A step for the UI after the reply.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Ui {
    /// Leave the assistant for a screen: search with `query`, with the title `pick`
    /// ("movie:<id>", "tv:<id>") selected.
    Navigate {
        screen: String,
        query: Option<String>,
        pick: Option<String>,
    },
    /// Close the assistant: an app took over the screen.
    Close,
}

#[derive(Default)]
pub struct AssistantState {
    pub recorder: voice::Recorder,
    pub speaker: speech::Speaker,
}
