//! mpv itself: fetched the first time something plays, like the emulators, from the Windows builds
//! mpv's own site points to. It lives in the data folder, so nothing is installed on the PC.

use std::path::{Path, PathBuf};

use serde_json::Value;
use tauri::{AppHandle, Manager};
use tokio::io::AsyncWriteExt;

use crate::error::{Error, Result};
use crate::state::AppState;

const LATEST: &str = "https://api.github.com/repos/shinchiro/mpv-winbuild-cmake/releases/latest";

fn folder(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().data_dir.join("mpv")
}

/// The player is here already: nothing to wait for.
pub fn installed(app: &AppHandle) -> bool {
    folder(app).join("mpv.exe").is_file()
}

/// The build for any x86-64 PC, from a release's assets: not the debug-symbols ("dev") one, nor the
/// "v3" one that needs a newer CPU.
fn pick_asset(release: &Value) -> Option<(String, String)> {
    release["assets"].as_array()?.iter().find_map(|a| {
        let name = a["name"].as_str()?;
        let rest = name.strip_prefix("mpv-x86_64-")?;
        if !name.ends_with(".7z") || rest.starts_with("v3") {
            return None;
        }
        Some((name.to_string(), a["browser_download_url"].as_str()?.to_string()))
    })
}

fn unpack(archive: &Path, dest: &Path) -> Result<()> {
    sevenz_rust2::decompress_file(archive, dest).map_err(|e| Error::msg(format!("Couldn't unpack the player: {e}")))
}

/// Where mpv.exe is, fetching it when it isn't here yet.
pub async fn ensure(app: &AppHandle) -> Result<PathBuf> {
    let dir = folder(app);
    let exe = dir.join("mpv.exe");
    if exe.is_file() {
        return Ok(exe);
    }
    tracing::info!("player: fetching mpv");
    let http = tvl_kit::http::download();
    let release: Value = http.get(LATEST).send().await?.error_for_status()?.json().await?;
    let (name, url) = pick_asset(&release).ok_or_else(|| Error::msg("No player build to download right now"))?;
    tokio::fs::create_dir_all(&dir).await?;
    let archive = dir.join(&name);
    let mut res = http.get(url).send().await?.error_for_status()?;
    let mut file = tokio::fs::File::create(&archive).await?;
    while let Some(chunk) = res.chunk().await? {
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);
    let (from, to) = (archive.clone(), dir.clone());
    let unpacked = tauri::async_runtime::spawn_blocking(move || unpack(&from, &to)).await.map_err(|e| Error::msg(e.to_string()))?;
    let _ = tokio::fs::remove_file(&archive).await;
    unpacked?;
    if exe.is_file() {
        tracing::info!("player: {name} is ready");
        Ok(exe)
    } else {
        Err(Error::msg("The player's download had no mpv.exe in it"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_plain_x86_64_build_is_the_one_taken() {
        let names = [
            "ffmpeg-x86_64-git-9a861e968.7z",
            "mpv-aarch64-20261021-git-e471496c56.7z",
            "mpv-dev-x86_64-20261021-git-e471496c56.7z",
            "mpv-i686-20261021-git-e471496c56.7z",
            "mpv-x86_64-v3-20261021-git-e471496c56.7z",
            "mpv-x86_64-20261021-git-e471496c56.7z",
        ];
        let assets: Vec<Value> = names.iter().map(|n| json!({ "name": n, "browser_download_url": format!("https://x/{n}") })).collect();
        let (name, url) = pick_asset(&json!({ "assets": assets })).unwrap();
        assert_eq!(name, "mpv-x86_64-20261021-git-e471496c56.7z");
        assert_eq!(url, format!("https://x/{name}"));
        assert!(pick_asset(&json!({ "assets": [] })).is_none());
    }
}
