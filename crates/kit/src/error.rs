use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    TomlDe(#[from] toml::de::Error),
    #[error(transparent)]
    TomlSer(#[from] toml::ser::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Never with the request's address: TMDB, Last.fm and Steam take their key in the query
    /// string, and this text reaches the screens and the log.
    #[error(transparent)]
    Http(reqwest::Error),
    #[error("{0}")]
    Msg(String),
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::Http(e.without_url())
    }
}

impl Error {
    pub fn msg(s: impl Into<String>) -> Self {
        Error::Msg(s.into())
    }
}

// Errors reach the screens as plain strings, as Tauri sent them, so the UIs show them as before.
impl Serialize for Error {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn http_errors_never_show_the_address() {
        // Nothing listens on the discard port, so this fails fast with the address in reqwest's
        // own message.
        let e = reqwest::Client::new()
            .get("http://127.0.0.1:9/3/search?api_key=SECRET123")
            .send()
            .await
            .unwrap_err();
        assert!(e.to_string().contains("SECRET123"), "reqwest names the address: {e}");
        let ours: Error = e.into();
        assert!(!ours.to_string().contains("SECRET123"), "{ours}");
        assert!(!serde_json::to_string(&ours).unwrap().contains("SECRET123"));
    }

    #[test]
    fn errors_serialize_as_plain_strings() {
        assert_eq!(serde_json::to_string(&Error::msg("That room is gone")).unwrap(), r#""That room is gone""#);
    }
}
