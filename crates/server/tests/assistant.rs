//! The assistant on the server, without a model: what it can do by itself, and where it acts.

mod common;

use common::*;
use serde_json::json;
use tvl_protocol::ScreenKind;

#[tokio::test]
async fn a_timer_said_to_the_ipad_needs_no_model_and_rings_in_its_room() {
    let s = server().await;
    s.ctx.devices.hello("ipad", ScreenKind::Tablet, "iPad do quarto").unwrap();
    s.ctx.devices.put("ipad", "iPad do quarto", Some("Quarto")).unwrap();
    let (status, reply) = post_as(
        s.http,
        "ipad",
        "assistant_ask",
        json!({"text": "timer de 12 minutos pra pizza", "now": "2026-10-27 19:00", "screen": "ipad", "room": null}),
    )
    .await;
    assert_eq!(status, 200, "{reply}");
    assert_eq!(reply["text"], "Timer de 12 minutos para pizza começou.");
    assert_eq!(reply["cards"][0]["clock"].as_str().map(|c| c.starts_with("timer:")), Some(true));

    let (_, clock) = post_as(s.http, "ipad", "clock_get", json!({})).await;
    assert_eq!(clock["timers"][0]["label"], "Pizza");
    assert_eq!(clock["timers"][0]["target"], json!({"type": "room", "id": "Quarto"}));

    // The Smart Home's "heard" line says who asked, and where.
    let (_, home) = post_as(s.http, "ipad", "smarthome_get", json!({})).await;
    assert_eq!(home["heard"]["by"], "iPad do quarto");
    assert_eq!(home["heard"]["room"], "Quarto");
    s.stop();
}

#[tokio::test]
async fn anything_else_needs_the_deepinfra_key() {
    let s = server().await;
    let (status, msg) = post_as(s.http, "phone", "assistant_ask", json!({"text": "qual a capital da França?", "now": "x"})).await;
    assert_eq!((status, msg), (500, json!("Add a DeepInfra API key in Settings to use the assistant")));
    s.stop();
}
