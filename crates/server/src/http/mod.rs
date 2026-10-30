//! The server on the home network: the companion's files, the API (`POST /api/<command>`), the
//! event socket for pages, and the TV PCs' sockets. The same router answers on HTTP and on HTTPS.

pub mod events;
pub mod files;
pub mod security;

use std::net::SocketAddr;

use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path as UrlPath, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::serve::ListenerExt;
use axum::{Json, Router};
use serde_json::Value;
use tokio::net::TcpListener;
use tower_http::services::{ServeDir, ServeFile};

use crate::ctx::Ctx;
use crate::router::{self, Caller};
use crate::tls;

pub struct Bound {
    pub http: SocketAddr,
    pub https: Option<SocketAddr>,
}

pub async fn serve(ctx: Ctx) -> std::io::Result<Bound> {
    let dir = ctx.config.companion();
    if !dir.join("index.html").is_file() {
        tracing::warn!("no companion in {} (build it with `npm run companion:build`)", dir.display());
    }
    let router = router(ctx.clone());
    let http = TcpListener::bind((ctx.config.bind, ctx.config.http_port)).await?;
    let http_addr = http.local_addr()?;
    let app = router.clone().into_make_service_with_connect_info::<SocketAddr>();
    let stop = ctx.shutdown.clone();
    tokio::spawn(async move {
        if let Err(e) = axum::serve(http, app).with_graceful_shutdown(stop.cancelled_owned()).await {
            tracing::error!("HTTP stopped: {e}");
        }
    });

    let mut https_addr = None;
    if let (Some(ca), Some(port)) = (ctx.ca.clone(), ctx.config.https_port) {
        match tls::listen(SocketAddr::new(ctx.config.bind, port), ca).await {
            Ok(listener) => {
                https_addr = Some(listener.addr());
                let _ = ctx.https_port.set(listener.addr().port());
                let app = router.into_make_service_with_connect_info::<SocketAddr>();
                let stop = ctx.shutdown.clone();
                tokio::spawn(async move {
                    // Tapped, as axum hands the peer's address (home_network_only) only to a
                    // TcpListener's connections or a tapped listener's.
                    if let Err(e) = axum::serve(listener.tap_io(|_| {}), app).with_graceful_shutdown(stop.cancelled_owned()).await {
                        tracing::error!("HTTPS stopped: {e}");
                    }
                });
            }
            Err(e) => tracing::error!("can't serve HTTPS on port {port}: {e}"),
        }
    }
    Ok(Bound { http: http_addr, https: https_addr })
}

fn router(ctx: Ctx) -> Router {
    let dir = ctx.config.companion();
    // Addresses like /watch/remote are the app's own: they get the page. A built file that's gone
    // (an old page asking for it) gets a 404, not the page.
    let files = ServeDir::new(&dir).fallback(ServeFile::new(dir.join("index.html")));
    let assets = ServeDir::new(dir.join("assets"));
    Router::new()
        .route("/api/events", get(events::socket))
        .route("/api/agent", get(crate::agents::socket))
        .route("/api/file", get(files::file))
        .route("/api/media/{token}", get(crate::library::stream::serve))
        .route("/api/assistant_transcribe", post(transcribe).layer(DefaultBodyLimit::max(10 * 1024 * 1024)))
        .route("/api/{cmd}", post(call))
        .route("/ca.crt", get(ca_certificate))
        .nest_service("/assets", assets)
        .fallback_service(files)
        .layer(middleware::from_fn(cache_rules))
        .layer(middleware::from_fn_with_state(ctx.names.clone(), security::check))
        .layer(middleware::from_fn(security::home_network_only))
        .with_state(ctx)
}

/// Built files have their hash in the name and never change; the page itself must be asked for
/// again each time, or Safari keeps showing an old build.
async fn cache_rules(req: Request, next: Next) -> Response {
    let built = req.uri().path().starts_with("/assets/");
    let mut res = next.run(req).await;
    let rule = if built && res.status().is_success() { "public, max-age=31536000, immutable" } else { "no-cache" };
    if !res.headers().contains_key(header::CACHE_CONTROL) {
        res.headers_mut().insert(header::CACHE_CONTROL, header::HeaderValue::from_static(rule));
    }
    res
}

/// A screen id or TV PC id from a header: short and plain, or ignored.
fn id_header(headers: &HeaderMap, name: &str) -> Option<String> {
    let v = headers.get(name)?.to_str().ok()?.trim();
    let ok = !v.is_empty() && v.len() <= 64 && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then(|| v.to_string())
}

/// Who a page's call comes from: its screen (X-TVL-Screen), the TV PC it chose for this call
/// (X-TVL-Target), and whether it holds an admin session (X-TVL-Admin) or is the server itself.
pub fn browser_caller(ctx: &Ctx, peer: SocketAddr, headers: &HeaderMap) -> Caller {
    let session = headers.get("x-tvl-admin").and_then(|v| v.to_str().ok()).is_some_and(|t| ctx.admin.session_ok(t.trim()));
    let local = ctx.config.trust_loopback && peer.ip().is_loopback();
    Caller {
        screen: id_header(headers, "x-tvl-screen"),
        agent: None,
        admin: session || local,
        target: id_header(headers, "x-tvl-target"),
    }
}

/// POST /api/<command> with the arguments as JSON, named as the TV's invoke() names them.
/// Answers the result as JSON, or the error as a JSON string (as Tauri sent it).
async fn call(
    State(ctx): State<Ctx>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    UrlPath(cmd): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let args: Value = if body.is_empty() {
        Value::Null
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => return (StatusCode::BAD_REQUEST, Json(format!("Bad arguments: {e}"))).into_response(),
        }
    };
    let caller = browser_caller(&ctx, peer, &headers);
    match router::dispatch(&ctx, &caller, &cmd, args).await {
        Ok(v) => Json(v).into_response(),
        Err(f) => {
            let status = StatusCode::from_u16(f.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (status, Json(f.to_string())).into_response()
        }
    }
}

/// The home certificate authority, for a phone or tablet to trust before it opens the HTTPS
/// address; Safari offers it as a profile to install.
async fn ca_certificate(State(ctx): State<Ctx>) -> Response {
    match &ctx.ca {
        Some(ca) => ([(header::CONTENT_TYPE, "application/x-x509-ca-cert")], ca.certificate()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// POST /api/assistant_transcribe with what a device's own microphone recorded (16-bit WAV): the
/// words, or null when nothing was said.
async fn transcribe(State(ctx): State<Ctx>, wav: Bytes) -> Response {
    tracing::info!("transcribing {} KB", wav.len() / 1024);
    match crate::assistant::transcribe::transcribe_wav(&ctx, wav.to_vec()).await {
        Ok(text) => Json(text).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e.to_string())).into_response(),
    }
}
