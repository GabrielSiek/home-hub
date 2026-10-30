//! The TV Launcher home server: the household's state and integrations, the companion's pages, and
//! the relay between the house's screens (a phone's remote key goes to the TV PC it controls). It
//! never plays video or audio. See `refactor/docs/ARCHITECTURE.md`.

pub mod admin;
pub mod agents;
pub mod assistant;
pub mod bus;
pub mod calendar;
pub mod clock;
pub mod config;
pub mod ctx;
pub mod devices;
pub mod discovery;
pub mod handlers;
pub mod http;
pub mod hub;
pub mod import;
pub mod library;
pub mod oauth;
pub mod router;
#[cfg(windows)]
pub mod service;
pub mod settings;
pub mod smarthome;
pub mod tls;

use std::net::SocketAddr;
use std::sync::Arc;

use tvl_kit::time::{Clock, SystemClock};
use tvl_kit::Result;

pub use config::Config;
pub use ctx::Ctx;

/// A running server.
pub struct Running {
    pub ctx: Ctx,
    pub http: SocketAddr,
    pub https: Option<SocketAddr>,
}

impl Running {
    /// Stops every listener, socket and loop.
    pub fn stop(&self) {
        self.ctx.shutdown.cancel();
    }
}

/// Starts the server with the system clock.
pub async fn start(config: Config) -> Result<Running> {
    start_with_clock(config, Arc::new(SystemClock)).await
}

/// Starts the server; tests pass a clock they move by hand.
pub async fn start_with_clock(config: Config, clock: Arc<dyn Clock>) -> Result<Running> {
    let ctx = ctx::Server::new(config, clock)?;
    let bound = http::serve(ctx.clone()).await?;
    discovery::start_probe(&ctx, bound.http.port(), bound.https.map(|a| a.port()));
    smarthome::start(&ctx);
    clock::start(&ctx);
    calendar::start(&ctx);
    Ok(Running { ctx, http: bound.http, https: bound.https })
}
