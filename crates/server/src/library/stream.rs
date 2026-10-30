//! The library's files, handed to a TV PC's player over HTTP. A file is only reachable through a
//! token made for it when a TV PC asked to play it; seeking is the browser-style Range request
//! `ServeFile` already answers. The server never decodes anything: it passes the bytes on.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::{Path as UrlPath, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tower::ServiceExt as _;
use tower_http::services::ServeFile;

use crate::ctx::Ctx;

/// Long enough for the longest film, paused for dinner.
const GRANT_MS: i64 = 12 * 3600 * 1000;
const TOKEN_LEN: usize = 43;

const VIDEO: &[&str] = &["mkv", "mp4", "m4v", "avi", "mov", "webm", "ts", "m2ts", "wmv", "mpg", "mpeg"];
const SUBTITLES: &[&str] = &["srt", "ass", "ssa", "vtt", "sub"];

fn has_extension(path: &Path, list: &[&str]) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| list.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

pub fn is_video(path: &Path) -> bool {
    has_extension(path, VIDEO)
}

/// `file`, resolved, when it's inside one of `roots` (Radarr's and Sonarr's root folders).
pub fn inside_roots(roots: &[PathBuf], file: &Path) -> Option<PathBuf> {
    let file = std::fs::canonicalize(file).ok()?;
    roots.iter().filter_map(|r| std::fs::canonicalize(r).ok()).any(|root| file.starts_with(root)).then_some(file)
}

/// A subtitle file next to a video, as Bazarr leaves them: "<video name>.pt-BR.srt".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sidecar {
    pub path: PathBuf,
    /// "pt-BR"; empty for "<video name>.srt".
    pub lang: String,
}

pub fn sidecars(video: &Path) -> Vec<Sidecar> {
    let (Some(dir), Some(stem)) = (video.parent(), video.file_stem().and_then(|s| s.to_str())) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<Sidecar> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| has_extension(p, SUBTITLES))
        .filter_map(|path| {
            let name = path.file_stem()?.to_str()?;
            let lang = if name == stem { "" } else { name.strip_prefix(stem)?.strip_prefix('.')? };
            Some(Sidecar { lang: lang.to_string(), path })
        })
        .collect();
    found.sort_by(|a, b| a.lang.cmp(&b.lang));
    found
}

struct Grant {
    path: PathBuf,
    expires: i64,
}

/// Which file each token opens.
#[derive(Default)]
pub struct Grants(parking_lot::Mutex<HashMap<String, Grant>>);

impl Grants {
    pub fn give(&self, path: PathBuf, now: i64) -> String {
        let token = tvl_kit::oauth::random_string(TOKEN_LEN);
        let mut all = self.0.lock();
        all.retain(|_, g| g.expires > now);
        all.insert(token.clone(), Grant { path, expires: now + GRANT_MS });
        token
    }

    pub fn path(&self, token: &str, now: i64) -> Option<PathBuf> {
        self.0.lock().get(token).filter(|g| g.expires > now).map(|g| g.path.clone())
    }
}

/// GET /api/media/<token>: the file, in whole or the range asked for.
pub async fn serve(State(ctx): State<Ctx>, UrlPath(token): UrlPath<String>, req: Request) -> Response {
    let Some(path) = ctx.library.grants.path(&token, ctx.time.now_ms()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match ServeFile::new(path).oneshot(req).await {
        Ok(res) => res.map(Body::new).into_response(),
        Err(never) => match never {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_opens_its_file_until_it_runs_out() {
        let grants = Grants::default();
        let token = grants.give(PathBuf::from("a.mkv"), 1000);
        assert_eq!(grants.path(&token, 2000), Some(PathBuf::from("a.mkv")));
        assert_eq!(grants.path(&token, 1000 + GRANT_MS), None);
        assert_eq!(grants.path("guess", 2000), None);
    }

    #[test]
    fn spent_tokens_are_dropped_as_new_ones_come() {
        let grants = Grants::default();
        grants.give(PathBuf::from("a.mkv"), 0);
        grants.give(PathBuf::from("b.mkv"), GRANT_MS + 1);
        assert_eq!(grants.0.lock().len(), 1);
    }

    #[test]
    fn only_videos_inside_the_roots() {
        let dir = tvl_kit::testing::temp_dir("media");
        let root = dir.join("movies");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.MKV"), b"x").unwrap();
        std::fs::write(dir.join("outside.mkv"), b"x").unwrap();
        let roots = [root.clone()];
        assert!(is_video(&root.join("a.MKV")));
        assert!(!is_video(&dir.join("settings.toml")));
        assert!(inside_roots(&roots, &root.join("a.MKV")).is_some());
        assert!(inside_roots(&roots, &dir.join("outside.mkv")).is_none());
        assert!(inside_roots(&roots, &root.join("..").join("outside.mkv")).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn subtitles_are_the_ones_named_after_the_video() {
        let dir = tvl_kit::testing::temp_dir("subs");
        for f in ["Dune.2021.mkv", "Dune.2021.srt", "Dune.2021.pt-BR.srt", "Dune.2021.en.ass", "Dune.2021.nfo", "Other.srt"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        let got: Vec<String> = sidecars(&dir.join("Dune.2021.mkv")).into_iter().map(|s| s.lang).collect();
        assert_eq!(got, ["", "en", "pt-BR"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
