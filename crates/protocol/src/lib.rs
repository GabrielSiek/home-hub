//! What the home server, the TV PCs' agents and the browsers say to each other: every command and
//! who runs it (`commands`), the event topics (`topics`), the frames on an agent's socket (`wire`),
//! and the types they carry.

pub mod clock;
pub mod commands;
pub mod device;
pub mod topics;
pub mod tv;
pub mod wire;

pub use commands::{spec, Access, CommandSpec, Owner, COMMANDS};
pub use device::{Caps, Screen, ScreenKind, ServiceLink};
pub use tv::{Key, Mode, Showing, TvNow};
pub use wire::Frame;

/// Raised whenever a frame or a command's meaning changes; a TV PC on another version is told to
/// update instead of half working.
pub const PROTOCOL_VERSION: u32 = 1;
