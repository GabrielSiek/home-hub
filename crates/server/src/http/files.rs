//! Pictures for the companion (`/api/file`): app tiles and art that live on a TV PC, asked of that
//! PC; photos the server keeps. Only image files, and only from the folders meant for them.

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use serde::Deserialize;
use serde_json::json;

use crate::ctx::Ctx;

/// The server's own folders that pictures may come from.
const FOLDERS: &[&str] = &["photos", "icons", "art"];

#[derive(Deserialize)]
pub struct FileQuery {
    path: String,
    /// The TV PC the picture lives on (its app tiles, its game art).
    device: Option<String>,
}

pub fn image_type(path: &str) -> Option<&'static str> {
    let ext = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        _ => return None,
    })
}

/// `path`, when it's inside one of the server's picture folders.
fn inside_folders(data_dir: &Path, path: &str) -> Option<PathBuf> {
    let file = std::fs::canonicalize(path).ok()?;
    FOLDERS
        .iter()
        .filter_map(|f| std::fs::canonicalize(data_dir.join(f)).ok())
        .any(|root| file.starts_with(root))
        .then_some(file)
}

pub async fn file(State(ctx): State<Ctx>, Query(q): Query<FileQuery>) -> Response {
    let Some(kind) = image_type(&q.path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let bytes = match q.device {
        Some(device) => {
            // The TV PC checks the folder itself (`device.file`) and answers base64.
            match ctx.agents.call(&device, "device.file", json!({ "path": q.path }), None, 10_000, false).await {
                Ok(v) => match v.as_str().and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok()) {
                    Some(b) => b,
                    None => return StatusCode::NOT_FOUND.into_response(),
                },
                Err(_) => return StatusCode::NOT_FOUND.into_response(),
            }
        }
        None => {
            let Some(path) = inside_folders(&ctx.config.data_dir, &q.path) else {
                return StatusCode::FORBIDDEN.into_response();
            };
            match tokio::fs::read(&path).await {
                Ok(b) => b,
                Err(_) => return StatusCode::NOT_FOUND.into_response(),
            }
        }
    };
    ([(header::CONTENT_TYPE, kind), (header::CACHE_CONTROL, "max-age=3600")], Body::from(bytes)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_pictures_are_served() {
        assert_eq!(image_type(r"C:\x\tile.PNG"), Some("image/png"));
        assert_eq!(image_type(r"C:\x\settings.toml"), None);
        assert_eq!(image_type(r"C:\x\.env"), None);
    }

    #[test]
    fn only_from_the_picture_folders() {
        let dir = tvl_kit::testing::temp_dir("files");
        std::fs::create_dir_all(dir.join("photos")).unwrap();
        std::fs::write(dir.join("photos").join("a.jpg"), b"x").unwrap();
        std::fs::write(dir.join("admin.json.png"), b"x").unwrap();
        assert!(inside_folders(&dir, dir.join("photos").join("a.jpg").to_str().unwrap()).is_some());
        assert!(inside_folders(&dir, dir.join("admin.json.png").to_str().unwrap()).is_none());
        let sneaky = dir.join("photos").join("..").join("admin.json.png");
        assert!(inside_folders(&dir, sneaky.to_str().unwrap()).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
