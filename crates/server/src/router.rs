//! Every command passes here, from a browser or a TV PC: is it known, may this caller run it, and
//! where does it run (`tvl_protocol::commands`). A TV PC's command goes to the TV PC the caller
//! controls, its arguments untouched: the TV PC reads them.

use std::fmt;

use serde_json::Value;
use tvl_protocol::{spec, Access, Owner};

use crate::agents::CallError;
use crate::ctx::Ctx;
use crate::devices::NoTarget;
use crate::handlers;

/// Who's asking.
#[derive(Debug, Clone, Default)]
pub struct Caller {
    /// The screen that asked: a companion page, or a TV PC (its own UI).
    pub screen: Option<String>,
    /// Set when a paired TV PC asked over its socket.
    pub agent: Option<String>,
    /// Holds an admin session, or asked from the server itself.
    pub admin: bool,
    /// The TV PC a page named for this one call (the "Controlling" picker's choice).
    pub target: Option<String>,
}

impl Caller {
    pub fn agent(device: &str) -> Self {
        Caller { screen: Some(device.to_string()), agent: Some(device.to_string()), admin: false, target: None }
    }

    /// Paired TV PCs are trusted like the admin: they show the PIN themselves.
    pub fn is_admin(&self) -> bool {
        self.admin || self.agent.is_some()
    }
}

/// Why a command didn't run, and the HTTP status a browser gets for it.
#[derive(Debug, PartialEq)]
pub enum Failure {
    Unknown(String),
    TvOnly,
    NeedsAdmin,
    /// Several TV PCs and nothing says which (no room, no pick).
    PickTv,
    /// The TV PC it controls is off (named).
    TvOff(String),
    TimedOut,
    TooManyTries,
    Failed(String),
}

impl Failure {
    pub fn status(&self) -> u16 {
        match self {
            Failure::Unknown(_) => 404,
            Failure::TvOnly => 403,
            Failure::NeedsAdmin => 401,
            Failure::PickTv => 409,
            Failure::TvOff(_) => 503,
            Failure::TimedOut => 504,
            Failure::TooManyTries => 429,
            Failure::Failed(_) => 500,
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Unknown(cmd) => write!(f, "The server doesn't know {cmd}"),
            Failure::TvOnly => write!(f, "Only a TV's own screen can do that"),
            Failure::NeedsAdmin => write!(f, "Unlock admin with the PIN shown in a TV's Settings"),
            Failure::PickTv => write!(f, "Choose which TV to control"),
            Failure::TvOff(name) => write!(f, "{name} is off"),
            Failure::TimedOut => write!(f, "The TV didn't answer in time"),
            Failure::TooManyTries => write!(f, "Too many wrong PINs: wait a minute"),
            Failure::Failed(why) => write!(f, "{why}"),
        }
    }
}

impl From<tvl_kit::Error> for Failure {
    fn from(e: tvl_kit::Error) -> Self {
        Failure::Failed(e.to_string())
    }
}

impl From<serde_json::Error> for Failure {
    fn from(e: serde_json::Error) -> Self {
        Failure::Failed(e.to_string())
    }
}

pub async fn dispatch(ctx: &Ctx, caller: &Caller, cmd: &str, args: Value) -> Result<Value, Failure> {
    let Some(spec) = spec(cmd) else { return Err(Failure::Unknown(cmd.to_string())) };
    match spec.access {
        Access::Public => {}
        Access::Admin if caller.is_admin() => {}
        Access::Admin => return Err(Failure::NeedsAdmin),
        Access::TvOnly if caller.agent.is_some() => {}
        Access::TvOnly => return Err(Failure::TvOnly),
    }
    if !spec.idempotent {
        tracing::info!(cmd, by = caller.screen.as_deref().unwrap_or("?"), "{}", describe(cmd, &args));
    }
    match spec.owner {
        Owner::Server | Owner::Composite => handlers::run(ctx, caller, cmd, args).await,
        Owner::Device => {
            // A TV PC runs its own commands itself; it never sends them here.
            if caller.agent.is_some() {
                return Err(Failure::TvOnly);
            }
            let device = ctx.devices.target(caller.screen.as_deref(), caller.target.as_deref()).map_err(|e| match e {
                NoTarget::Pick => Failure::PickTv,
                NoTarget::Off(name) => Failure::TvOff(name),
            })?;
            let from = caller.screen.as_deref();
            ctx.agents.call(&device, cmd, args, from, spec.timeout_ms, spec.ordered).await.map_err(|e| match e {
                CallError::Offline => Failure::TvOff(ctx.devices.get(&device).map_or_else(|| "The TV".into(), |s| s.name)),
                CallError::Timeout => Failure::TimedOut,
                CallError::Failed(why) => Failure::Failed(why),
            })
        }
    }
}

/// What the log says a command did, without what was typed or any secret.
fn describe(cmd: &str, args: &Value) -> String {
    match cmd {
        "remote_text" => "(text)".into(),
        "assistant_transcribe" => "(recording)".into(),
        "admin_unlock" | "admin_pin_set" | "server_settings_save" | "oauth_paste" => "(private)".into(),
        _ => tvl_kit::log::clip(&args.to_string(), 200),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn what_was_typed_never_reaches_the_log() {
        assert_eq!(describe("remote_text", &json!({"text": "my password"})), "(text)");
        assert_eq!(describe("admin_unlock", &json!({"pin": "1234"})), "(private)");
        assert!(describe("smarthome_control", &json!({"id": "lamp"})).contains("lamp"));
    }

    #[test]
    fn failures_map_to_statuses_and_plain_messages() {
        assert_eq!(Failure::TvOff("Quarto TV".into()).to_string(), "Quarto TV is off");
        assert_eq!(Failure::TvOff("x".into()).status(), 503);
        assert_eq!(Failure::PickTv.status(), 409);
        assert_eq!(Failure::NeedsAdmin.status(), 401);
    }
}
