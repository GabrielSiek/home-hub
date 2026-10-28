//! SteamGridDB: cover and hero art for games whose store doesn't provide any locally.
//! Needs a key (free, steamgriddb.com › Preferences › API) in Settings or STEAMGRIDDB_API_KEY.

use std::path::Path;

use serde::Deserialize;

use crate::error::Result;

const API: &str = "https://www.steamgriddb.com/api/v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 600×900 portrait cover.
    Cover,
    /// Wide background.
    Hero,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Cover => "cover",
            Kind::Hero => "hero",
        }
    }
}

#[derive(Deserialize)]
struct Response<T> {
    #[serde(default = "Vec::new")]
    data: Vec<T>,
}

#[derive(Deserialize)]
struct Hit {
    id: u64,
}

#[derive(Deserialize)]
struct Image {
    url: String,
}

/// SteamGridDB's id for the best match on a game name.
pub async fn find(client: &reqwest::Client, key: &str, name: &str) -> Result<Option<u64>> {
    let url = format!("{API}/search/autocomplete/{}", urlencoding::encode(name));
    let r: Response<Hit> = client.get(url).bearer_auth(key).send().await?.error_for_status()?.json().await?;
    Ok(r.data.first().map(|h| h.id))
}

/// URL of the top-rated image of a kind for a game.
pub async fn image_url(client: &reqwest::Client, key: &str, game: u64, kind: Kind) -> Result<Option<String>> {
    let url = match kind {
        Kind::Cover => format!("{API}/grids/game/{game}?dimensions=600x900"),
        Kind::Hero => format!("{API}/heroes/game/{game}"),
    };
    let r: Response<Image> = client.get(url).bearer_auth(key).send().await?.error_for_status()?.json().await?;
    Ok(r.data.into_iter().next().map(|i| i.url))
}

/// Downloads an image next to `stem`, keeping the URL's extension. Returns the file written.
pub async fn download(client: &reqwest::Client, url: &str, stem: &Path) -> Result<std::path::PathBuf> {
    let ext = url
        .rsplit('.')
        .next()
        .filter(|e| ["png", "jpg", "jpeg", "webp"].contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or("jpg");
    let bytes = client.get(url).send().await?.error_for_status()?.bytes().await?;
    let path = stem.with_extension(ext);
    std::fs::write(&path, &bytes)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses_parse() {
        let hits: Response<Hit> =
            serde_json::from_str(r#"{"success":true,"data":[{"id":5248,"name":"Fortnite","types":["steam"]}]}"#).unwrap();
        assert_eq!(hits.data[0].id, 5248);
        let none: Response<Image> = serde_json::from_str(r#"{"success":true,"data":[]}"#).unwrap();
        assert!(none.data.is_empty());
    }
}
