//! The library, against stand-ins for Radarr, Sonarr, qBittorrent and TMDB speaking their real paths.

mod common;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, post, put};
use axum::{Form, Json, Router};
use common::*;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tvl_server::settings::House;
use tvl_server::{Config, Running};

const KEY: &str = "arr-key";

#[derive(Default)]
struct Fake {
    /// Requests that changed something: "POST movie", with their bodies.
    log: Mutex<Vec<(String, Value)>>,
    movies: Mutex<Vec<Value>>,
    series: Mutex<Vec<Value>>,
    episodes: Mutex<Vec<Value>>,
    files: Mutex<Vec<Value>>,
    queue: Mutex<Vec<Value>>,
    root: Mutex<String>,
}

type Shared = Arc<Fake>;

impl Fake {
    fn did(&self, what: &str) -> Vec<Value> {
        self.log.lock().iter().filter(|(w, _)| w == what).map(|(_, b)| b.clone()).collect()
    }
}

fn keyed(h: &HeaderMap) -> Result<(), StatusCode> {
    (h.get("x-api-key").and_then(|k| k.to_str().ok()) == Some(KEY)).then_some(()).ok_or(StatusCode::UNAUTHORIZED)
}

async fn listen(app: Router) -> SocketAddr {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    addr
}

/// The routes Radarr and Sonarr have in common.
fn shared(app: Router<Shared>) -> Router<Shared> {
    app.route("/api/v3/rootfolder", get(|State(f): State<Shared>| async move { Json(json!([{ "path": *f.root.lock(), "freeSpace": 5000 }])) }))
        .route(
            "/api/v3/qualityprofile",
            get(|| async { Json(json!([{ "id": 1, "name": "Any" }, { "id": 4, "name": "HD-1080p" }])) }),
        )
        .route(
            "/api/v3/queue",
            get(|State(f): State<Shared>, h: HeaderMap| async move {
                keyed(&h)?;
                Ok::<_, StatusCode>(Json(json!({ "records": *f.queue.lock() })))
            }),
        )
        .route(
            "/api/v3/command",
            post(|State(f): State<Shared>, Json(b): Json<Value>| async move {
                f.log.lock().push(("command".into(), b));
                Json(json!({ "id": 1 }))
            }),
        )
}

async fn radarr(f: Shared) -> SocketAddr {
    let app = shared(Router::new())
        .route(
            "/api/v3/movie",
            get(|State(f): State<Shared>, Query(q): Query<std::collections::HashMap<String, String>>, h: HeaderMap| async move {
                keyed(&h)?;
                let all = f.movies.lock().clone();
                let wanted = q.get("tmdbId").and_then(|t| t.parse::<u64>().ok());
                Ok::<_, StatusCode>(Json(all.into_iter().filter(|m| wanted.is_none_or(|w| m["tmdbId"] == w)).collect::<Vec<_>>()))
            })
            .post(|State(f): State<Shared>, Json(b): Json<Value>| async move {
                f.log.lock().push(("add movie".into(), b.clone()));
                f.movies.lock().push(b.clone());
                Json(b)
            }),
        )
        // Deleting a movie deletes its history.
        .route(
            "/api/v3/history/movie",
            get(|State(f): State<Shared>| async move {
                let there = !f.movies.lock().is_empty();
                Json(if there { json!([{ "downloadId": "MOVIE1" }, { "eventType": "grabbed" }, { "downloadId": "MOVIE1" }]) } else { json!([]) })
            }),
        )
        .route(
            "/api/v3/movie/lookup/tmdb",
            get(|Query(q): Query<std::collections::HashMap<String, String>>| async move {
                let id: u64 = q["tmdbId"].parse().unwrap();
                Json(json!({ "id": 0, "tmdbId": id, "title": "Looked up", "titleSlug": "looked-up", "images": [] }))
            }),
        )
        .route(
            "/api/v3/movie/{id}",
            delete(|State(f): State<Shared>, Query(q): Query<std::collections::HashMap<String, String>>| async move {
                f.log.lock().push(("delete movie".into(), json!(q)));
                f.movies.lock().clear();
            }),
        )
        .with_state(f);
    listen(app).await
}

async fn sonarr(f: Shared) -> SocketAddr {
    let app = shared(Router::new())
        .route(
            "/api/v3/series",
            get(|State(f): State<Shared>, h: HeaderMap| async move {
                keyed(&h)?;
                Ok::<_, StatusCode>(Json(f.series.lock().clone()))
            })
            .post(|State(f): State<Shared>, Json(mut b): Json<Value>| async move {
                f.log.lock().push(("add series".into(), b.clone()));
                b["id"] = json!(7);
                b["tmdbId"] = json!(66732);
                f.series.lock().push(b.clone());
                Json(b)
            }),
        )
        .route(
            "/api/v3/history/series",
            get(|State(f): State<Shared>| async move {
                let there = !f.series.lock().is_empty();
                Json(if there { json!([{ "downloadId": "SERIES1" }, { "downloadId": "SERIES2" }]) } else { json!([]) })
            }),
        )
        .route(
            "/api/v3/series/{id}",
            delete(|State(f): State<Shared>, Query(q): Query<std::collections::HashMap<String, String>>| async move {
                f.log.lock().push(("delete series".into(), json!(q)));
                f.series.lock().clear();
            }),
        )
        .route(
            "/api/v3/series/lookup",
            get(|Query(q): Query<std::collections::HashMap<String, String>>| async move {
                assert_eq!(q["term"], "tvdb:305288");
                Json(json!([{ "tvdbId": 305288, "title": "Stranger Things", "seasons": [] }]))
            }),
        )
        .route("/api/v3/episode", get(|State(f): State<Shared>| async move { Json(f.episodes.lock().clone()) }))
        .route("/api/v3/episodefile", get(|State(f): State<Shared>| async move { Json(f.files.lock().clone()) }))
        .route(
            "/api/v3/episode/monitor",
            put(|State(f): State<Shared>, Json(b): Json<Value>| async move {
                f.log.lock().push(("monitor".into(), b));
                Json(json!([]))
            }),
        )
        .with_state(f);
    listen(app).await
}

async fn qbittorrent(f: Shared) -> SocketAddr {
    let app = Router::new()
        .route(
            "/api/v2/auth/login",
            post(|Form(q): Form<std::collections::HashMap<String, String>>| async move {
                if q["password"] == "secret" {
                    Ok(([("set-cookie", "SID=abc; HttpOnly; path=/")], "Ok."))
                } else {
                    Err(StatusCode::FORBIDDEN)
                }
            }),
        )
        .route(
            "/api/v2/torrents/info",
            get(|h: HeaderMap| async move {
                if h.get("cookie").and_then(|c| c.to_str().ok()) != Some("SID=abc") {
                    return Err(StatusCode::FORBIDDEN);
                }
                Ok(Json(json!([{ "hash": "abc123", "name": "x", "state": "downloading", "progress": 0.5, "dlspeed": 2048, "eta": 90 }])))
            }),
        )
        .route(
            "/api/v2/torrents/delete",
            post(|State(f): State<Shared>, Form(q): Form<std::collections::HashMap<String, String>>| async move {
                f.log.lock().push(("delete torrents".into(), json!(q)));
            }),
        )
        // Version 4: no torrents/stop.
        .route(
            "/api/v2/torrents/pause",
            post(|State(f): State<Shared>, Form(q): Form<std::collections::HashMap<String, String>>| async move {
                f.log.lock().push(("pause".into(), json!(q)));
            }),
        )
        .with_state(f);
    listen(app).await
}

async fn tmdb() -> SocketAddr {
    listen(Router::new().route("/tv/66732/external_ids", get(|| async { Json(json!({ "tvdb_id": 305288 })) }))).await
}

struct Bench {
    s: Running,
    fake: Shared,
    dir: std::path::PathBuf,
}

/// A server with all of it set up, and a media folder with one file for it to hand out.
async fn bench() -> Bench {
    let fake = Shared::default();
    let dir = tvl_kit::testing::temp_dir("library");
    let media = dir.join("media");
    std::fs::create_dir_all(&media).unwrap();
    *fake.root.lock() = media.to_string_lossy().into_owned();
    let (r, so, q, t) = (radarr(fake.clone()).await, sonarr(fake.clone()).await, qbittorrent(fake.clone()).await, tmdb().await);
    let mut config = Config::for_tests(&dir);
    config.tmdb_url = format!("http://{t}");
    let s = tvl_server::start(config).await.unwrap();
    s.ctx
        .settings
        .save(House {
            tmdb_api_key: "tmdb".into(),
            radarr_url: format!("http://{r}"),
            radarr_api_key: KEY.into(),
            sonarr_url: format!("http://{so}/"),
            sonarr_api_key: KEY.into(),
            qbittorrent_url: format!("http://{q}"),
            qbittorrent_username: "admin".into(),
            qbittorrent_password: "secret".into(),
            ..Default::default()
        })
        .unwrap();
    Bench { s, fake, dir }
}

/// A paired TV PC that answers nothing.
async fn fake_agent(addr: SocketAddr, device: &str, token: &str) -> Arc<Agent> {
    connect(addr, device, token, Arc::new(|_, _, _| Box::pin(async { Ok(Value::Null) }))).await
}

/// The body of a call that must work.
async fn ok(s: &Running, cmd: &str, args: Value) -> Value {
    let (status, v) = post_as(s.http, "tablet", cmd, args).await;
    assert_eq!(status, 200, "{cmd}: {v}");
    v
}

fn movie_with_file(b: &Bench, name: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(&*b.fake.root.lock()).join(name);
    std::fs::write(&path, (0..=255u8).cycle().take(4096).collect::<Vec<u8>>()).unwrap();
    b.fake.movies.lock().push(json!({
        "id": 3, "tmdbId": 603, "title": "The Matrix", "year": 1999, "hasFile": true, "added": "2026-10-01T00:00:00Z",
        "images": [{ "coverType": "poster", "remoteUrl": "https://img/p.jpg" }, { "coverType": "fanart", "remoteUrl": "https://img/f.jpg" }],
        "movieFile": { "id": 1, "path": path.to_string_lossy(), "size": 4096 },
    }));
    path
}

#[tokio::test]
async fn without_keys_the_library_says_so_and_nothing_else_breaks() {
    let s = server().await;
    let v = ok(&s, "library_get", json!({})).await;
    assert_eq!((v["configured"].as_bool(), v["titles"].as_array().map(Vec::len)), (Some(false), Some(0)));
    let t = ok(&s, "library_title", json!({ "mediaType": "movie", "id": 1 })).await;
    assert_eq!((&t["configured"], &t["state"]), (&json!(false), &json!("absent")));
    let (status, said) = post_as(s.http, "tablet", "library_request", json!({ "mediaType": "movie", "id": 1 })).await;
    assert_eq!(status, 500);
    assert!(said.as_str().unwrap().contains("isn't set up"), "{said}");
    s.stop();
}

#[tokio::test]
async fn the_list_is_what_can_be_played_with_the_space_left() {
    let b = bench().await;
    movie_with_file(&b, "matrix.mkv");
    b.fake.movies.lock().push(json!({ "id": 4, "tmdbId": 604, "title": "Not here yet", "hasFile": false }));
    b.fake.series.lock().push(json!({ "id": 7, "tmdbId": 66732, "title": "Stranger Things", "statistics": { "episodeFileCount": 2 }, "added": "2026-10-02T00:00:00Z" }));
    let v = ok(&b.s, "library_get", json!({})).await;
    let titles = v["titles"].as_array().unwrap();
    assert_eq!(titles.len(), 2);
    assert_eq!((&titles[0]["id"], &titles[0]["media_type"]), (&json!(66732), &json!("tv")), "newest first");
    assert_eq!(titles[1]["poster"], "https://img/p.jpg");
    assert_eq!(titles[1]["backdrop"], "https://img/f.jpg");
    assert_eq!(titles[1]["year"], "1999");
    assert_eq!(v["free_bytes"], 5000);
    b.s.stop();
}

#[tokio::test]
async fn a_new_movie_is_added_with_the_chosen_quality_and_searched() {
    let b = bench().await;
    let t = ok(&b.s, "library_request", json!({ "mediaType": "movie", "id": 550 })).await;
    let added = b.fake.did("add movie");
    assert_eq!(added.len(), 1);
    assert_eq!(added[0]["qualityProfileId"], 4);
    assert_eq!(added[0]["rootFolderPath"], *b.fake.root.lock());
    assert_eq!(added[0]["addOptions"]["searchForMovie"], true);
    assert_eq!(added[0]["titleSlug"], "looked-up", "what the lookup said goes back whole");
    assert_eq!(t["state"], "wanted");
    b.s.stop();
}

#[tokio::test]
async fn a_movie_asked_for_again_is_searched_not_added() {
    let b = bench().await;
    b.fake.movies.lock().push(json!({ "id": 9, "tmdbId": 550, "title": "Fight Club", "hasFile": false }));
    ok(&b.s, "library_request", json!({ "mediaType": "movie", "id": 550 })).await;
    assert!(b.fake.did("add movie").is_empty());
    assert_eq!(b.fake.did("command"), [json!({ "name": "MoviesSearch", "movieIds": [9] })]);
    b.s.stop();
}

#[tokio::test]
async fn one_season_of_a_new_series_fetches_only_that_season() {
    let b = bench().await;
    *b.fake.episodes.lock() = vec![
        json!({ "id": 1, "seasonNumber": 1, "episodeNumber": 1 }),
        json!({ "id": 2, "seasonNumber": 2, "episodeNumber": 1 }),
        json!({ "id": 3, "seasonNumber": 2, "episodeNumber": 2 }),
    ];
    ok(&b.s, "library_request", json!({ "mediaType": "tv", "id": 66732, "season": 2 })).await;
    let added = b.fake.did("add series");
    assert_eq!(added[0]["addOptions"]["monitor"], "none", "nothing fetched on its own");
    assert_eq!(b.fake.did("monitor"), [json!({ "episodeIds": [2, 3], "monitored": true })]);
    assert_eq!(b.fake.did("command"), [json!({ "name": "EpisodeSearch", "episodeIds": [2, 3] })]);
    b.s.stop();
}

#[tokio::test]
async fn a_title_downloading_shows_qbittorrents_speed() {
    let b = bench().await;
    b.fake.movies.lock().push(json!({ "id": 9, "tmdbId": 550, "title": "Fight Club", "hasFile": false }));
    *b.fake.queue.lock() = vec![json!({
        "movieId": 9, "title": "Fight.Club.1999", "size": 1000.0, "sizeleft": 400.0, "timeleft": "00:10:00",
        "downloadId": "ABC123", "movie": { "tmdbId": 550, "title": "Fight Club" },
    })];
    let t = ok(&b.s, "library_title", json!({ "mediaType": "movie", "id": 550 })).await;
    assert_eq!((&t["state"], &t["progress"], &t["speed"], &t["eta_s"]), (&json!("downloading"), &json!(0.6), &json!(2048), &json!(90)));
    let d = ok(&b.s, "library_downloads", json!({})).await;
    assert_eq!(d["items"][0]["title"], "Fight Club");
    assert_eq!(d["items"][0]["id"], 550);
    assert_eq!(d["items"][0]["progress"], 0.5, "qBittorrent's own figure");
    assert_eq!(d["speed"], 2048);
    b.s.stop();
}

#[tokio::test]
async fn downloads_pause_on_the_older_api_too() {
    let b = bench().await;
    ok(&b.s, "library_download_control", json!({ "hash": "ABC123", "action": "pause" })).await;
    assert_eq!(b.fake.did("pause"), [json!({ "hashes": "abc123" })]);
    b.s.stop();
}

#[tokio::test]
async fn removing_a_title_takes_its_files_and_needs_the_admin() {
    let b = bench().await;
    movie_with_file(&b, "matrix.mkv");
    let (status, _) = post_as(b.s.http, "tablet", "library_remove", json!({ "mediaType": "movie", "id": 603 })).await;
    assert_eq!(status, 401);
    assert!(b.fake.did("delete movie").is_empty());
    let tv = paired_tv(&b.s, "tv-1", "Sala", None);
    let agent = fake_agent(b.s.http, "tv-1", &tv).await;
    agent.call("library_remove", json!({ "mediaType": "movie", "id": 603 })).await.unwrap();
    assert_eq!(b.fake.did("delete movie")[0]["deleteFiles"], "true");
    b.s.stop();
}

#[tokio::test]
async fn removing_a_title_frees_the_disk_by_dropping_its_torrents_too() {
    let b = bench().await;
    let tv = paired_tv(&b.s, "tv-1", "Sala", None);
    let agent = fake_agent(b.s.http, "tv-1", &tv).await;

    movie_with_file(&b, "matrix.mkv");
    agent.call("library_remove", json!({ "mediaType": "movie", "id": 603 })).await.unwrap();
    let gone = b.fake.did("delete torrents");
    assert_eq!(gone.len(), 1);
    assert_eq!(gone[0]["hashes"], "movie1");
    assert_eq!(gone[0]["deleteFiles"], "true");

    b.fake.series.lock().push(json!({ "id": 7, "tmdbId": 66732, "title": "Stranger Things" }));
    agent.call("library_remove", json!({ "mediaType": "tv", "id": 66732 })).await.unwrap();
    assert_eq!(b.fake.did("delete torrents")[1]["hashes"], "series1|series2");
    b.s.stop();
}

#[tokio::test]
async fn a_title_that_was_never_here_drops_no_torrents() {
    let b = bench().await;
    let tv = paired_tv(&b.s, "tv-1", "Sala", None);
    let agent = fake_agent(b.s.http, "tv-1", &tv).await;
    agent.call("library_remove", json!({ "mediaType": "movie", "id": 603 })).await.unwrap();
    assert!(b.fake.did("delete torrents").is_empty());
    b.s.stop();
}

#[tokio::test]
async fn a_tv_pc_gets_a_token_that_streams_with_seeking_and_nothing_else_does() {
    let b = bench().await;
    let path = movie_with_file(&b, "matrix.mkv");
    std::fs::write(path.with_file_name("matrix.pt-BR.srt"), "1\n00:00:01,000 --> 00:00:02,000\nOi\n").unwrap();

    // A phone can't ask for the address.
    let (status, _) = post_as(b.s.http, "tablet", "library_stream", json!({ "mediaType": "movie", "id": 603 })).await;
    assert_eq!(status, 403);

    let tv = paired_tv(&b.s, "tv-1", "Sala", None);
    let agent = fake_agent(b.s.http, "tv-1", &tv).await;
    let v = agent.call("library_stream", json!({ "mediaType": "movie", "id": 603 })).await.unwrap();
    assert_eq!((&v["name"], &v["size"]), (&json!("matrix.mkv"), &json!(4096)));

    let http = reqwest::Client::new();
    let url = format!("http://{}{}", b.s.http, v["path"].as_str().unwrap());
    let r = http.get(&url).header("Range", "bytes=256-259").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 206);
    assert_eq!(r.bytes().await.unwrap().to_vec(), [0, 1, 2, 3], "the bytes at that offset");

    let sub = format!("http://{}{}", b.s.http, v["subtitles"][0]["path"].as_str().unwrap());
    assert_eq!(v["subtitles"][0]["lang"], "pt-BR");
    assert!(http.get(&sub).send().await.unwrap().text().await.unwrap().contains("Oi"));

    let r = http.get(format!("http://{}/api/media/not-a-token", b.s.http)).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 404);
    b.s.stop();
}

#[tokio::test]
async fn a_path_outside_the_root_folders_is_never_served() {
    let b = bench().await;
    let outside = b.dir.join("settings-copy.mkv");
    std::fs::write(&outside, b"secret").unwrap();
    b.fake.movies.lock().push(json!({
        "id": 3, "tmdbId": 603, "title": "Odd", "hasFile": true,
        "movieFile": { "id": 1, "path": outside.to_string_lossy(), "size": 6 },
    }));
    let tv = paired_tv(&b.s, "tv-1", "Sala", None);
    let agent = fake_agent(b.s.http, "tv-1", &tv).await;
    let e = agent.call("library_stream", json!({ "mediaType": "movie", "id": 603 })).await.unwrap_err();
    assert!(e.contains("can't read that file"), "{e}");
    b.s.stop();
}

#[tokio::test]
async fn an_episode_plays_with_the_next_one_here() {
    let b = bench().await;
    let root = b.fake.root.lock().clone();
    let file = std::path::Path::new(&root).join("s01e01.mkv");
    std::fs::write(&file, b"video").unwrap();
    b.fake.series.lock().push(json!({ "id": 7, "tmdbId": 66732, "title": "Stranger Things" }));
    *b.fake.episodes.lock() = vec![
        json!({ "id": 1, "seasonNumber": 1, "episodeNumber": 1, "hasFile": true, "episodeFileId": 11 }),
        json!({ "id": 2, "seasonNumber": 1, "episodeNumber": 2, "hasFile": false }),
        json!({ "id": 3, "seasonNumber": 1, "episodeNumber": 3, "hasFile": true, "episodeFileId": 12 }),
    ];
    *b.fake.files.lock() = vec![json!({ "id": 11, "path": file.to_string_lossy(), "size": 5 })];
    let tv = paired_tv(&b.s, "tv-1", "Sala", None);
    let agent = fake_agent(b.s.http, "tv-1", &tv).await;
    let v = agent.call("library_stream", json!({ "mediaType": "tv", "id": 66732, "season": 1, "episode": 1 })).await.unwrap();
    assert_eq!(v["next"], json!({ "season": 1, "episode": 3 }));
    assert_eq!((&v["season"], &v["episode"]), (&json!(1), &json!(1)));
    b.s.stop();
}

#[tokio::test]
async fn the_librarys_keys_stay_on_the_server() {
    let b = bench().await;
    let masked = serde_json::to_value(b.s.ctx.settings.get().masked()).unwrap();
    assert!(masked["radarr_api_key"].as_str().unwrap().contains('•'));
    assert!(masked["qbittorrent_password"].as_str().unwrap().contains('•'));
    let for_tvs = tvl_server::settings::for_tvs(&b.s.ctx.settings.get());
    assert!(for_tvs.get("radarr_api_key").is_none());
    assert!(for_tvs.get("qbittorrent_password").is_none());
    assert_eq!(for_tvs["library_on"], true);
    b.s.stop();
}
