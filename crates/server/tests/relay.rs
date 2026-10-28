//! The server between the house's screens: a phone's command reaches the TV PC it controls, with
//! a fake agent in place of each TV PC.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use common::*;
use serde_json::{json, Value};
use tvl_protocol::{ScreenKind, PROTOCOL_VERSION};

/// Two TV PCs, in Quarto and Sala, and an iPad in Quarto.
async fn house() -> (tvl_server::Running, String, String) {
    let s = server().await;
    let quarto = paired_tv(&s, "pc-quarto", "Quarto TV", Some("Quarto"));
    let sala = paired_tv(&s, "pc-sala", "Sala TV", Some("Sala"));
    s.ctx.devices.hello("ipad", ScreenKind::Tablet, "iPad").unwrap();
    s.ctx.devices.put("ipad", "iPad", Some("Quarto")).unwrap();
    (s, quarto, sala)
}

#[tokio::test]
async fn a_remote_key_reaches_the_tv_pc_of_the_ipads_room() {
    let (s, quarto, sala) = house().await;
    let q = connect(s.http, "pc-quarto", &quarto, ok_handler()).await;
    let l = connect(s.http, "pc-sala", &sala, ok_handler()).await;

    let (status, _) = post_as(s.http, "ipad", "remote_key", json!({"key": "Up"})).await;
    assert_eq!(status, 200);
    assert_eq!(q.calls_of("remote_key"), vec![(json!({"key": "Up"}), Some("ipad".to_string()))]);
    assert!(l.calls_of("remote_key").is_empty());

    // The picker's choice for one call wins.
    let (status, _) = post(s.http, "remote_key", json!({"key": "Down"}), &[("X-TVL-Screen", "ipad"), ("X-TVL-Target", "pc-sala")]).await;
    assert_eq!(status, 200);
    assert_eq!(l.calls_of("remote_key").len(), 1);
    s.stop();
}

#[tokio::test]
async fn a_room_whose_tv_pc_is_off_says_so_instead_of_driving_another_room() {
    let (s, _quarto, sala) = house().await;
    let l = connect(s.http, "pc-sala", &sala, ok_handler()).await;
    let (status, msg) = post_as(s.http, "ipad", "tv_now", json!({})).await;
    assert_eq!((status, msg), (503, json!("Quarto TV is off")));
    assert!(l.calls_of("tv_now").is_empty(), "Sala's TV isn't asked instead");

    // A phone in no room, with two TVs: it has to pick.
    s.ctx.devices.hello("phone", ScreenKind::Phone, "Phone").unwrap();
    let (status, _) = post_as(s.http, "phone", "tv_now", json!({})).await;
    assert_eq!(status, 409);
    s.stop();
}

#[tokio::test]
async fn remote_keys_reach_the_tv_one_at_a_time_and_reads_do_not_wait() {
    let (s, quarto, _) = house().await;
    let (now, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (n, m) = (now.clone(), most.clone());
    let handler: Handler = Arc::new(move |_a, cmd, _args| {
        let (n, m) = (n.clone(), m.clone());
        Box::pin(async move {
            // Only the calls under test count (the server also tells a new TV PC what rings).
            if cmd == "ring.set" {
                return Ok(Value::Null);
            }
            let at_once = n.fetch_add(1, Ordering::SeqCst) + 1;
            m.fetch_max(at_once, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(30)).await;
            n.fetch_sub(1, Ordering::SeqCst);
            Ok(Value::Null)
        })
    });
    let _q = connect(s.http, "pc-quarto", &quarto, handler).await;
    let keys = (0..5).map(|_| post_as(s.http, "ipad", "remote_key", json!({"key": "Right"})));
    for (status, _) in futures_util::future::join_all(keys).await {
        assert_eq!(status, 200);
    }
    assert_eq!(most.load(Ordering::SeqCst), 1, "ordered keys never overlap");

    most.store(0, Ordering::SeqCst);
    let reads = (0..5).map(|_| post_as(s.http, "ipad", "tmdb_search", json!({"query": "x"})));
    futures_util::future::join_all(reads).await;
    assert!(most.load(Ordering::SeqCst) > 1, "reads run side by side");
    s.stop();
}

#[tokio::test]
async fn a_tv_pc_dropping_mid_call_fails_the_call_at_once() {
    let (s, quarto, _) = house().await;
    let handler: Handler = Arc::new(|a, _cmd, _args| {
        Box::pin(async move {
            a.close();
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok(Value::Null)
        })
    });
    let _q = connect(s.http, "pc-quarto", &quarto, handler).await;
    let started = std::time::Instant::now();
    let (status, msg) = post_as(s.http, "ipad", "watch_play", json!({})).await;
    assert_eq!((status, msg), (503, json!("Quarto TV is off")));
    assert!(started.elapsed() < Duration::from_secs(5), "not the minute-long play timeout");
    s.stop();
}

#[tokio::test]
async fn a_newer_socket_from_the_same_tv_pc_takes_over() {
    let (s, quarto, _) = house().await;
    let first = connect(s.http, "pc-quarto", &quarto, ok_handler()).await;
    let second = connect(s.http, "pc-quarto", &quarto, ok_handler()).await;
    assert!(eventually(|| first.closed.is_cancelled()).await, "the older socket is closed");
    let (status, _) = post_as(s.http, "ipad", "tv_off", json!({})).await;
    assert_eq!(status, 200);
    assert_eq!(second.calls_of("tv_off").len(), 1);
    assert!(s.ctx.devices.get("pc-quarto").unwrap().online);
    s.stop();
}

#[tokio::test]
async fn a_tv_pc_can_ask_the_server_while_the_server_waits_on_it() {
    let (s, quarto, _) = house().await;
    // While answering the iPad's call, the TV PC asks the server something first.
    let handler: Handler = Arc::new(|a, _cmd, _args| {
        Box::pin(async move {
            let devices = a.call("devices_get", json!({})).await?;
            Ok(json!({ "screens": devices.as_array().map_or(0, Vec::len) }))
        })
    });
    let _q = connect(s.http, "pc-quarto", &quarto, handler).await;
    let (status, answer) = post_as(s.http, "ipad", "tv_now", json!({})).await;
    assert_eq!(status, 200);
    assert_eq!(answer, json!({"screens": 3}));
    s.stop();
}

#[tokio::test]
async fn pages_hear_their_own_tv_pc_and_the_server_but_never_the_tv_pcs_secrets() {
    let (s, quarto, sala) = house().await;
    let q = connect(s.http, "pc-quarto", &quarto, ok_handler()).await;
    let l = connect(s.http, "pc-sala", &sala, ok_handler()).await;
    let ipad = page(s.http, "ipad").await;
    tokio::time::sleep(Duration::from_millis(150)).await;

    q.send_event("tv", json!({"mode": "streaming", "title": "Quarto show"}));
    l.send_event("tv", json!({"mode": "streaming", "title": "Sala show"}));
    // An event a TV PC may not send is dropped.
    q.send_event("smarthome", json!({"devices": []}));
    s.ctx.bus.emit("clock", &json!({"timers": []}));
    s.ctx.bus.emit("settings", &json!({"tmdb_api_key": "SECRET"}));

    assert!(eventually(|| !ipad.heard("tv").is_empty() && !ipad.heard("clock").is_empty()).await);
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(ipad.heard("tv"), vec![json!({"mode": "streaming", "title": "Quarto show"})]);
    assert!(ipad.heard("settings").is_empty(), "the TV PCs' feed never reaches a page");
    assert!(!ipad.heard("smarthome").contains(&json!({"devices": []})), "a TV PC can't speak for the smart home");
    // TV PCs hear the server's events, the secrets feed included, but not each other's.
    assert!(eventually(|| q.events.lock().iter().any(|(n, _)| n == "settings")).await);
    assert!(!q.events.lock().iter().any(|(n, _)| n == "tv"));
    s.stop();
}

#[tokio::test]
async fn moving_the_ipad_to_another_room_switches_its_tv_and_tells_the_page() {
    let (s, quarto, sala) = house().await;
    let _q = connect(s.http, "pc-quarto", &quarto, ok_handler()).await;
    let l = connect(s.http, "pc-sala", &sala, ok_handler()).await;
    let ipad = page(s.http, "ipad").await;
    let (status, _) = post_as(s.http, "ipad", "smarthome_put_screen", json!({"id": "ipad", "name": "iPad", "room": "Sala"})).await;
    assert_eq!(status, 200);
    assert!(eventually(|| ipad.heard("follow").iter().any(|f| f["device"] == "pc-sala")).await);
    post_as(s.http, "ipad", "remote_key", json!({"key": "Select"})).await;
    assert_eq!(l.calls_of("remote_key").len(), 1);
    s.stop();
}

#[tokio::test]
async fn pairing_waits_for_the_pin_holder_and_the_token_then_works() {
    let s = server().await;
    let (code, mut socket) = ask_to_pair(s.http, "pc-new").await;
    // A page without admin can't approve; with the PIN it can.
    let (status, _) = post_as(s.http, "phone", "pair_approve", json!({"code": code})).await;
    assert_eq!(status, 401);
    let pin = s.ctx.admin.pin();
    let (status, unlocked) = post_as(s.http, "phone", "admin_unlock", json!({"pin": pin})).await;
    assert_eq!(status, 200);
    let session = unlocked["token"].as_str().unwrap().to_string();
    let (status, listed) = post(s.http, "pair_list", json!({}), &[("X-TVL-Admin", &session)]).await;
    assert_eq!((status, listed[0]["device"].clone()), (200, json!("pc-new")));
    let (status, _) = post(s.http, "pair_approve", json!({"code": code}), &[("X-TVL-Admin", &session)]).await;
    assert_eq!(status, 200);

    use futures_util::StreamExt;
    let token = loop {
        if let tokio_tungstenite::tungstenite::Message::Text(t) = socket.next().await.unwrap().unwrap() {
            if let Ok(tvl_protocol::Frame::Paired { token, ca_pem }) = serde_json::from_str(t.as_str()) {
                assert!(ca_pem.contains("BEGIN CERTIFICATE") || s.ctx.ca.is_none());
                break token;
            }
        }
    };
    let agent = connect(s.http, "pc-new", &token, ok_handler()).await;
    assert!(s.ctx.devices.get("pc-new").unwrap().online);
    drop(agent);
    s.stop();
}

#[tokio::test]
async fn unknown_revoked_and_outdated_tv_pcs_are_turned_away() {
    let s = server().await;
    let token = paired_tv(&s, "pc", "TV", None);
    let refused = |f: tvl_protocol::Frame| matches!(f, tvl_protocol::Frame::Refused { .. });

    assert!(refused(answer_to(s.http, Some("made-up"), hello("pc", PROTOCOL_VERSION)).await));
    let old = answer_to(s.http, Some(&token), hello("pc", PROTOCOL_VERSION + 1)).await;
    assert!(matches!(&old, tvl_protocol::Frame::Refused { reason } if reason.contains("Update this TV PC")), "{old:?}");
    assert!(refused(answer_to(s.http, Some(&token), hello("someone-else", PROTOCOL_VERSION)).await));

    // Forgetting the TV PC (admin) revokes its token.
    let (status, _) = post(s.http, "smarthome_forget_screen", json!({"id": "pc"}), &[]).await;
    assert_eq!(status, 401);
    s.ctx.admin.revoke("pc").unwrap();
    assert!(refused(answer_to(s.http, Some(&token), hello("pc", PROTOCOL_VERSION)).await));
    s.stop();
}
