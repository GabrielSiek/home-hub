//! What a web page elsewhere, or a phone without the PIN, can't do.

mod common;

use common::*;
use serde_json::json;

#[tokio::test]
async fn requests_naming_another_host_are_refused() {
    let s = server().await;
    let r = reqwest::Client::new()
        .post(format!("http://{}/api/devices_get", s.http))
        .header("Host", "evil.example")
        .header("Content-Type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 421);
    s.stop();
}

#[tokio::test]
async fn pages_elsewhere_cannot_post_or_listen() {
    let s = server().await;
    // A cross-site POST from another site's page.
    let (status, _) = post(s.http, "devices_get", json!({}), &[("Origin", "https://evil.example")]).await;
    assert_eq!(status, 403);
    // The "simple" request a page can send without the browser asking first.
    let r = reqwest::Client::new()
        .post(format!("http://{}/api/smarthome_control", s.http))
        .header("Content-Type", "text/plain")
        .body(r#"{"id":"lamp","command":{"on":true}}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 415);
    // The event socket, opened from another site's page.
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut req = format!("ws://{}/api/events", s.http).into_client_request().unwrap();
    req.headers_mut().insert("Origin", "https://evil.example".parse().unwrap());
    assert!(tokio_tungstenite::connect_async(req).await.is_err());
    // The server's own page is fine.
    let origin = format!("http://{}", s.http);
    let (status, _) = post(s.http, "devices_get", json!({}), &[("Origin", &origin)]).await;
    assert_eq!(status, 200);
    s.stop();
}

#[tokio::test]
async fn admin_needs_the_pin_and_guessing_is_slowed() {
    let s = server().await;
    let (status, msg) = post_as(s.http, "phone", "admin_pin_set", json!({"pin": "1111"})).await;
    assert_eq!((status, msg), (401, json!("Unlock admin with the PIN shown in a TV's Settings")));
    for _ in 0..5 {
        let (status, _) = post_as(s.http, "phone", "admin_unlock", json!({"pin": "wrong"})).await;
        assert_eq!(status, 500);
    }
    let pin = s.ctx.admin.pin();
    let (status, _) = post_as(s.http, "phone", "admin_unlock", json!({"pin": pin})).await;
    assert_eq!(status, 429, "the right PIN waits out the minute too");
    s.stop();
}

#[tokio::test]
async fn pages_reach_what_the_companion_could_and_nothing_more() {
    let s = server().await;
    let (status, _) = post_as(s.http, "phone", "retro_remove", json!({"emu": "pcsx2"})).await;
    assert_eq!(status, 403, "a TV PC's own commands stay on the TV");
    let (status, _) = post_as(s.http, "phone", "save_settings", json!({})).await;
    assert_eq!(status, 403);
    let (status, msg) = post_as(s.http, "phone", "rm_rf", json!({})).await;
    assert_eq!((status, msg), (404, json!("The server doesn't know rm_rf")));
    s.stop();
}

#[tokio::test]
async fn keys_reach_tv_pcs_whole_and_pages_masked() {
    let s = server().await;
    let mut house = s.ctx.settings.get();
    house.tmdb_api_key = "tmdb-secret-123456".into();
    s.ctx.settings.save(house).unwrap();
    let token = paired_tv(&s, "pc", "TV", None);
    let tv = connect(s.http, "pc", &token, ok_handler()).await;
    assert!(eventually(|| tv.events.lock().iter().any(|(n, p)| n == "settings" && p["tmdb_api_key"] == "tmdb-secret-123456")).await);

    let pin = s.ctx.admin.pin();
    let (_, unlocked) = post_as(s.http, "phone", "admin_unlock", json!({"pin": pin})).await;
    let admin = unlocked["token"].as_str().unwrap().to_string();
    let (status, got) = post(s.http, "server_settings_get", json!({}), &[("X-TVL-Admin", &admin)]).await;
    assert_eq!((status, got["tmdb_api_key"].clone()), (200, json!("••••3456")));
    // Saved back as shown: the key stays; a new region goes through.
    let mut edited = got.clone();
    edited["region"] = json!("PT");
    let (status, saved) = post(s.http, "server_settings_save", json!({"settings": edited}), &[("X-TVL-Admin", &admin)]).await;
    assert_eq!(status, 200);
    assert_eq!(saved["region"], "PT");
    assert_eq!(s.ctx.settings.get().tmdb_api_key, "tmdb-secret-123456");
    s.stop();
}
