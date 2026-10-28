//! Timers ringing across the house: on the TV PC of the room they were set in, else wherever a
//! TV PC is on, stopped from any screen.

mod common;

use common::*;
use serde_json::{json, Value};
use tvl_protocol::ScreenKind;

/// The rings in the last `ring.set` TV PC `a` got.
fn ringing_on(a: &Agent) -> Option<Vec<Value>> {
    a.calls_of("ring.set").last().map(|(args, _)| args["rings"].as_array().cloned().unwrap_or_default())
}

async fn house() -> (tvl_server::Running, String, String) {
    let s = server().await;
    let quarto = paired_tv(&s, "pc-quarto", "Quarto TV", Some("Quarto"));
    let sala = paired_tv(&s, "pc-sala", "Sala TV", Some("Sala"));
    s.ctx.devices.hello("ipad", ScreenKind::Tablet, "iPad").unwrap();
    s.ctx.devices.put("ipad", "iPad", Some("Quarto")).unwrap();
    s.ctx.devices.hello("phone", ScreenKind::Phone, "Phone").unwrap();
    (s, quarto, sala)
}

#[tokio::test]
async fn a_timer_set_on_the_ipad_rings_in_its_room_and_any_screen_stops_it() {
    let (s, quarto, sala) = house().await;
    let q = connect(s.http, "pc-quarto", &quarto, ok_handler()).await;
    let l = connect(s.http, "pc-sala", &sala, ok_handler()).await;

    let (status, clock) = post_as(s.http, "ipad", "clock_timer_start", json!({"ms": 1000, "label": "Pizza"})).await;
    assert_eq!(status, 200);
    assert_eq!(clock["timers"][0]["target"], json!({"type": "room", "id": "Quarto"}));
    let id = clock["timers"][0]["id"].as_str().unwrap().to_string();

    assert!(eventually_long(|| ringing_on(&q).is_some_and(|r| r.len() == 1)).await, "Quarto's TV rings");
    let ring = ringing_on(&q).unwrap()[0].clone();
    assert_eq!((ring["label"].as_str(), ring["kind"].as_str()), (Some("Pizza"), Some("timer")));
    assert!(ringing_on(&l).is_none_or(|r| r.is_empty()), "Sala's TV stays quiet");

    // The phone answers; Quarto's TV is told nothing rings any more.
    let (status, clock) = post_as(s.http, "phone", "clock_respond", json!({"id": id, "answer": {"type": "stop"}})).await;
    assert_eq!(status, 200);
    assert!(clock["ringing"].as_array().unwrap().is_empty());
    assert!(eventually(|| ringing_on(&q).is_some_and(|r| r.is_empty())).await);
    // A second answer does nothing.
    let (status, _) = post_as(s.http, "ipad", "clock_respond", json!({"id": id, "answer": {"type": "stop"}})).await;
    assert_eq!(status, 200);
    s.stop();
}

#[tokio::test]
async fn with_the_rooms_tv_off_another_tv_rings_and_says_why() {
    let (s, _quarto, sala) = house().await;
    let l = connect(s.http, "pc-sala", &sala, ok_handler()).await;
    let (status, _) = post_as(s.http, "ipad", "clock_timer_start", json!({"ms": 1000})).await;
    assert_eq!(status, 200);
    assert!(eventually_long(|| ringing_on(&l).is_some_and(|r| r.len() == 1)).await);
    let ring = ringing_on(&l).unwrap()[0].clone();
    assert_eq!(ring["note"], "No TV is on in Quarto: ringing on Sala TV");
    assert_eq!(ring["targets"], json!(["pc-sala"]));
    s.stop();
}

#[tokio::test]
async fn a_tv_pc_that_reconnects_hears_what_is_ringing() {
    let (s, quarto, _) = house().await;
    let first = connect(s.http, "pc-quarto", &quarto, ok_handler()).await;
    post_as(s.http, "ipad", "clock_timer_start", json!({"ms": 1000})).await;
    assert!(eventually_long(|| ringing_on(&first).is_some_and(|r| r.len() == 1)).await);
    let again = connect(s.http, "pc-quarto", &quarto, ok_handler()).await;
    assert!(eventually(|| ringing_on(&again).is_some_and(|r| r.len() == 1)).await, "the whole state again");
    s.stop();
}

#[tokio::test]
async fn shadow_mode_never_rings() {
    let dir = tvl_kit::testing::temp_dir("server-shadow");
    let mut config = tvl_server::Config::for_tests(&dir);
    config.shadow = true;
    let s = tvl_server::start(config).await.unwrap();
    let token = paired_tv(&s, "pc", "TV", Some("Quarto"));
    let tv = connect(s.http, "pc", &token, ok_handler()).await;
    post_as(s.http, "pc", "clock_timer_start", json!({"ms": 1000})).await;
    tokio::time::sleep(std::time::Duration::from_millis(1800)).await;
    assert!(tv.calls_of("ring.set").is_empty(), "the old launcher rings its own copy");
    s.stop();
}
