//! Cards shown over the page of the web app in front: the calendar's reminders (sent by the home
//! server as `calendar.card`) and the Live tab's.

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::cdp::{self, Cmd};
use crate::state::{AppState, Mode};

const CARD_SCRIPT: &str = include_str!("card.js");

/// A reminder's card over the page of the app in front.
pub fn card_over_app(app: &AppHandle, title: &str, start: i64, color: &str) {
    cdp::send(app, Cmd::Eval(format!("{CARD_SCRIPT}({}, {start}, {})", json!(title), json!(color))));
}

/// The server's `calendar.card {event}`: over the app in front when one is, and to the TV UI
/// (`calendar-reminder`), which shows it on the launcher's own screens.
pub fn calendar_card(app: &AppHandle, args: &Value) {
    let event = &args["event"];
    if app.state::<AppState>().mode() == Mode::Streaming {
        let title = event["title"].as_str().unwrap_or_default();
        let color = event["color"].as_str().unwrap_or("#7aa2ff");
        card_over_app(app, title, event["start"].as_i64().unwrap_or_default(), color);
    }
    let _ = app.emit("calendar-reminder", event);
}
