//! The TV UI calls `invoke(...)` as it always did. A command the home server owns
//! (`tvl_protocol::commands`) goes there, with this TV PC as the caller; the rest run here. A read
//! the server can't answer right now gets the last answer it gave, so the screens still show the
//! house while it's away.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde_json::Value;
use tauri::ipc::{Invoke, InvokeBody, InvokeError};
use tauri::{AppHandle, Manager};

/// Last answers kept for when the server is away; more than the reads the screens repeat.
const MOST_KEPT: usize = 64;

fn kept() -> &'static Mutex<HashMap<String, Value>> {
    static KEPT: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();
    KEPT.get_or_init(Mutex::default)
}

/// The app's invoke handler: the server's commands go there, the rest to `local` (Tauri's own).
pub fn route(invoke: Invoke, local: &(dyn Fn(Invoke) -> bool + Send + Sync)) -> bool {
    let cmd = invoke.message.command();
    if !tvl_protocol::spec(cmd).is_some_and(|s| s.runs_on_server()) {
        return local(invoke);
    }
    let cmd = cmd.to_string();
    let app = invoke.message.webview().app_handle().clone();
    let args = match invoke.message.payload() {
        InvokeBody::Json(v) => v.clone(),
        InvokeBody::Raw(_) => Value::Null,
    };
    invoke.resolver.respond_async(async move { forward(&app, &cmd, args).await.map_err(InvokeError::from) });
    true
}

/// Runs server-owned `cmd` there, and shapes the answer for this TV.
pub async fn forward(app: &AppHandle, cmd: &str, args: Value) -> Result<Value, String> {
    let rings = cmd.starts_with("clock_");
    // Answered on this TV: the bell stops now, not when the server's word comes back.
    if cmd == "clock_respond" {
        if let Some(id) = args["id"].as_str() {
            crate::ring::stop_here(app, id);
        }
    }
    let read = tvl_protocol::spec(cmd).is_some_and(|s| s.idempotent);
    let key = format!("{cmd} {args}");
    match super::server_call(app, cmd, args).await {
        Ok(v) => {
            // Every clock command answers with the clock: kept, and with only this TV's rings.
            let v = if rings && v.get("ringing").is_some() {
                crate::ring::clock(app, v.clone());
                crate::ring::snapshot(app).unwrap_or(v)
            } else {
                super::for_this_tv(app, cmd, v)
            };
            if read {
                let mut kept = kept().lock().unwrap();
                if kept.len() >= MOST_KEPT {
                    kept.clear();
                }
                kept.insert(key, v.clone());
            }
            Ok(v)
        }
        Err(e) if e == tvl_agent_link::OFFLINE => {
            if rings {
                if let Some(clock) = crate::ring::snapshot(app) {
                    return Ok(clock);
                }
            }
            if read {
                if let Some(v) = kept().lock().unwrap().get(&key) {
                    return Ok(v.clone());
                }
            }
            Err(e)
        }
        Err(e) => Err(e),
    }
}
