//! The house's other screens: smart TVs the server reaches directly, with no TV PC involved. Today
//! that's anything speaking Google Cast, and on a Google TV the remote protocol too, once paired.
//! The server still plays nothing: it tells the TV what to open, and hands it the address of a
//! library file to fetch.

pub mod cast;
pub mod mdns;
pub mod remote;
pub mod streaming;

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;
use tvl_kit::store::JsonStore;
use tvl_kit::{Error, Result};

use crate::ctx::Ctx;

const CAST_SERVICE: &str = "_googlecast._tcp.local";
const CAST_PORT: u16 = 8009;
/// How long devices get to answer.
const LISTEN: Duration = Duration::from_secs(3);
/// How often the network is asked again: TVs come and go, and addresses change.
const LOOK_EVERY: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Player {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    /// Takes the remote's links: it can open streaming apps.
    pub paired: bool,
    #[serde(skip)]
    addr: SocketAddr,
}

/// What the TVs know us by, kept: losing it means pairing each TV again.
#[derive(Clone, Default, Serialize, Deserialize)]
struct Remote {
    identity: Option<remote::Identity>,
    paired: Vec<String>,
}

pub struct Players {
    known: Mutex<BTreeMap<String, Player>>,
    remote: JsonStore<Remote>,
    /// The pairing whose code is on a TV's screen, and which TV. One at a time.
    pairing: tokio::sync::Mutex<Option<(String, remote::Pairing)>>,
}

impl Players {
    pub fn open(data_dir: &Path) -> Self {
        Players { known: Mutex::default(), remote: JsonStore::open(data_dir.join("tv-remote.json")), pairing: tokio::sync::Mutex::default() }
    }
}

pub fn list(ctx: &Ctx) -> Vec<Player> {
    let mut all: Vec<Player> = ctx.players.known.lock().values().cloned().collect();
    ctx.players.remote.read(|r| all.iter_mut().for_each(|p| p.paired = r.paired.contains(&p.id)));
    all
}

fn get(ctx: &Ctx, id: &str) -> Result<Player> {
    list(ctx).into_iter().find(|p| p.id == id).ok_or_else(|| Error::msg("That TV isn't on the network"))
}

fn from_answer(f: mdns::Found) -> Option<Player> {
    let id = f.txt.get("id")?.clone();
    let name = f.txt.get("fn").cloned().unwrap_or_else(|| "Google TV".into());
    Some(Player { id, name, kind: "cast", paired: false, addr: SocketAddr::new(f.ip, f.port.unwrap_or(CAST_PORT)) })
}

/// What answers now, added to what's known: a TV asleep may miss a round.
async fn look(ctx: &Ctx) {
    let found = match mdns::browse(CAST_SERVICE, LISTEN).await {
        Ok(found) => found,
        Err(e) => return tracing::warn!("looking for TVs: {e}"),
    };
    let changed = {
        let mut known = ctx.players.known.lock();
        let before = known.clone();
        known.extend(found.into_iter().filter_map(from_answer).map(|p| (p.id.clone(), p)));
        *known != before
    };
    if changed {
        ctx.bus.emit("players", &list(ctx));
    }
}

pub fn start(ctx: &Ctx) {
    let ctx = ctx.clone();
    tvl_kit::task::spawn_supervised("players", ctx.shutdown.clone(), move || {
        let ctx = ctx.clone();
        async move {
            loop {
                look(&ctx).await;
                tokio::time::sleep(LOOK_EVERY).await;
            }
        }
    });
}

/// This server's address as `peer` sees it: the laptop has both a cable and Wi-Fi.
async fn address_for(peer: IpAddr) -> Result<IpAddr> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
    sock.connect((peer, 9)).await?;
    Ok(sock.local_addr()?.ip())
}

/// What the TV is told a file is. Cast's player goes by this, not by the file.
fn content_type(name: &str) -> &'static str {
    match name.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("mp4" | "m4v") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mov") => "video/quicktime",
        Some("ts") => "video/mp2t",
        _ => "video/x-matroska",
    }
}

/// Plays a movie or episode of the library on TV `id`. It decodes the file itself: nothing here
/// converts it, so a format the TV doesn't know comes back as an error.
pub async fn play(ctx: &Ctx, id: &str, media_type: &str, title: u64, season: Option<u32>, episode: Option<u32>) -> Result<()> {
    let tv = get(ctx, id)?;
    let file = crate::library::stream(ctx, media_type, title, season, episode).await?;
    let here = address_for(tv.addr.ip()).await?;
    let url = format!("http://{}{}", SocketAddr::new(here, ctx.config.http_port), file.path);
    cast::play(tv.addr, &url, content_type(&file.name), &file.name).await
}

pub async fn control(ctx: &Ctx, id: &str, action: cast::Action) -> Result<()> {
    cast::control(get(ctx, id)?.addr, action).await
}

/// Opens the Spotify app on TV `id`: by the remote when it's paired, which also wakes it.
pub async fn open_spotify(ctx: &Ctx, id: &str) -> Result<()> {
    let tv = get(ctx, id)?;
    if tv.paired {
        return open_link(ctx, &tv, &streaming::link("spotify", None)?).await;
    }
    cast::open_app(tv.addr, cast::SPOTIFY_APP).await
}

async fn identity(ctx: &Ctx) -> Result<remote::Identity> {
    if let Some(id) = ctx.players.remote.read(|r| r.identity.clone()) {
        return Ok(id);
    }
    let id = remote::Identity::create().await?;
    ctx.players.remote.change(|r| {
        r.identity = Some(id.clone());
        Ok(())
    })?;
    Ok(id)
}

/// Makes TV `id` show its pairing code.
pub async fn pair_start(ctx: &Ctx, id: &str) -> Result<()> {
    let tv = get(ctx, id)?;
    let pairing = remote::start_pairing(identity(ctx).await?, tv.addr.ip()).await?;
    *ctx.players.pairing.lock().await = Some((tv.id, pairing));
    Ok(())
}

/// Finishes the pairing with the code on TV `id`. A code mistyped can be typed again only by
/// starting over: the TV drops the pairing at a wrong answer.
pub async fn pair_finish(ctx: &Ctx, id: &str, code: &str) -> Result<()> {
    let Some((tv, pairing)) = ctx.players.pairing.lock().await.take() else {
        return Err(Error::msg("No pairing is waiting: start it again"));
    };
    if tv != id {
        return Err(Error::msg("Another TV was pairing: start it again"));
    }
    pairing.finish(code).await?;
    ctx.players.remote.change(|r| {
        if !r.paired.contains(&tv) {
            r.paired.push(tv);
        }
        Ok(())
    })?;
    ctx.bus.emit("players", &list(ctx));
    Ok(())
}

async fn open_link(ctx: &Ctx, tv: &Player, link: &str) -> Result<()> {
    let identity = ctx.players.remote.read(|r| r.identity.clone()).filter(|_| tv.paired).ok_or_else(|| Error::msg("Pair the TV first"))?;
    remote::open(&identity, tv.addr.ip(), link).await
}

/// Opens a streaming service on TV `id`: its app, or a title in it.
pub async fn open_service(ctx: &Ctx, id: &str, service: &str, url: Option<&str>) -> Result<()> {
    open_link(ctx, &get(ctx, id)?, &streaming::link(service, url)?).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tv_is_known_by_its_cast_id() {
        let f = mdns::Found {
            ip: IpAddr::V4(Ipv4Addr::new(192, 168, 68, 120)),
            port: Some(8009),
            txt: [("id".to_string(), "7e3b".to_string()), ("fn".to_string(), "SmartTV 4K FFM".to_string())].into(),
        };
        let p = from_answer(f.clone()).unwrap();
        assert_eq!((p.id.as_str(), p.name.as_str()), ("7e3b", "SmartTV 4K FFM"));
        assert_eq!(p.addr.port(), 8009);
        assert!(from_answer(mdns::Found { txt: Default::default(), ..f }).is_none(), "no id, nothing to call it");
    }

    #[test]
    fn the_tv_is_told_what_a_file_is() {
        assert_eq!(content_type("Dune.2021.MP4"), "video/mp4");
        assert_eq!(content_type("Dune.2021.mkv"), "video/x-matroska");
        assert_eq!(content_type("no-extension"), "video/x-matroska");
    }
}
