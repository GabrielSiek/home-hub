//! Plumbing shared by the home server (`tvl-server`) and the TV app's agent: one error type, JSON
//! files that survive a crash, HTTP clients, OAuth, time, text matching, logging and supervised
//! background tasks.

pub mod error;
pub mod http;
pub mod log;
pub mod oauth;
pub mod store;
pub mod task;
pub mod testing;
pub mod text;
pub mod time;

pub use error::{Error, Result};
