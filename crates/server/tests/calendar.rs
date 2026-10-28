//! The calendar on the server: Grêmio's matches sent by a TV PC, and sign-ins that go through a
//! TV PC's browser.

mod common;

use std::sync::Arc;

use common::*;
use serde_json::{json, Value};

fn gremio_game(start: i64) -> Value {
    json!({
        "id": "401", "league": "bra.1", "league_name": "Brasileirão", "sport": "soccer", "start": start,
        "phase": "pre", "status": "", "progress": 0.0,
        "home": {"id": "6273", "name": "Grêmio", "abbr": "GRE", "color": "#0d80bf", "winner": false},
        "away": {"id": "2026", "name": "Internacional", "abbr": "INT", "color": "#e5050f", "winner": false},
        "venue": "Arena do Grêmio", "channels": [{"name": "Premiere", "app": "globoplay"}], "mine": "gremio"
    })
}

#[tokio::test]
async fn the_next_match_comes_from_a_tv_pcs_sports_tab_and_stays_while_it_is_off() {
    let s = server().await;
    let token = paired_tv(&s, "pc", "TV", Some("Quarto"));
    let tv = connect(s.http, "pc", &token, ok_handler()).await;
    let kickoff = tvl_kit::time::now_ms() + 3 * 3_600_000;
    tv.send_event("feed.sports", json!({"games": [gremio_game(kickoff)], "reminders": ["401"]}));
    assert!(eventually(|| !s.ctx.calendar.feed_is_empty()).await);
    tv.close();

    let (status, next) = post_as(s.http, "phone", "calendar_next", json!({})).await;
    assert_eq!(status, 200);
    assert_eq!(next["title"], "Grêmio × Internacional");
    assert_eq!(next["calendar"], "gremio");
    assert_eq!(next["remind"], true, "the Sports tab's reminder carries over");
    assert_eq!(next["game"]["venue"], "Arena do Grêmio", "the match center gets the whole game");
    s.stop();
}

#[tokio::test]
async fn a_sign_in_answered_for_another_request_is_refused() {
    let s = server().await;
    let mut house = s.ctx.settings.get();
    house.google_client_id = "id.apps.googleusercontent.com".into();
    house.google_client_secret = "secret".into();
    s.ctx.settings.save(house).unwrap();
    let token = paired_tv(&s, "pc", "TV", None);
    let handler: Handler = Arc::new(|_a, cmd, args| {
        Box::pin(async move {
            if cmd != "oauth.loopback" {
                return Ok(Value::Null);
            }
            // The page it opens must be Google's, on the registered loopback port.
            assert!(args["url"].as_str().unwrap().starts_with("https://accounts.google.com/"));
            assert_eq!(args["port"], 47952);
            Ok(json!({"state": "someone-else", "code": "abc"}))
        })
    });
    let tv = connect(s.http, "pc", &token, handler).await;
    let reply = tv.call("calendar_sign_in", json!({})).await;
    assert_eq!(reply, Err("The sign-in answered for another request: try again".into()));
    assert!(!s.ctx.calendar.signed_in());
    s.stop();
}

#[tokio::test]
async fn a_phone_needs_a_tv_pc_that_is_on_to_sign_in() {
    let s = server().await;
    let mut house = s.ctx.settings.get();
    house.google_client_id = "id".into();
    house.google_client_secret = "secret".into();
    s.ctx.settings.save(house).unwrap();
    paired_tv(&s, "pc", "TV", None);
    let pin = s.ctx.admin.pin();
    let (_, unlocked) = post_as(s.http, "phone", "admin_unlock", json!({"pin": pin})).await;
    let session = unlocked["token"].as_str().unwrap().to_string();
    let (status, msg) = post(s.http, "calendar_sign_in", json!({}), &[("X-TVL-Screen", "phone"), ("X-TVL-Admin", &session)]).await;
    assert_eq!((status, msg), (500, json!("Sign in from a TV PC that's on: its browser opens Google's page")));
    s.stop();
}
