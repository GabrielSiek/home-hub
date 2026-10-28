//! A signed-in account's token, kept in a JSON file, refreshed shortly before it expires. Only one
//! refresh runs at a time: providers that hand out a new refresh token each time (eWeLink,
//! Spotify) would otherwise see the second refresh use a spent one and sign the user out.

use std::path::PathBuf;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::store;
use crate::time::now_ms;

/// The same fields as the old launcher's `google.json`, so its files load as they are.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Token {
    pub access: String,
    pub refresh: String,
    /// When `access` expires, unix ms.
    pub expires: i64,
}

impl Token {
    /// Good for at least another minute.
    pub fn fresh(&self) -> bool {
        self.expires - now_ms() > 60_000
    }
}

/// Why a provider refused to refresh a token (revoked, expired consent): the account must sign in
/// again.
pub type Refused = String;

pub struct TokenStore {
    path: PathBuf,
    /// Names the account in messages ("Google").
    service: &'static str,
    token: Mutex<Option<Token>>,
    refreshing: tokio::sync::Mutex<()>,
}

impl TokenStore {
    pub fn open(path: impl Into<PathBuf>, service: &'static str) -> Self {
        let path = path.into();
        let token = store::load(&path);
        Self { path, service, token: Mutex::new(token), refreshing: tokio::sync::Mutex::new(()) }
    }

    pub fn get(&self) -> Option<Token> {
        self.token.lock().clone()
    }

    pub fn signed_in(&self) -> bool {
        self.token.lock().is_some()
    }

    /// Keeps `token`, or signs out with None (the file is removed).
    pub fn set(&self, token: Option<Token>) -> Result<()> {
        match &token {
            Some(t) => store::save(&self.path, t, true)?,
            None => match std::fs::remove_file(&self.path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            },
        }
        *self.token.lock() = token;
        Ok(())
    }

    /// A valid access token, refreshed with `renew` when it's about to expire. When the provider
    /// refuses, the account is signed out and the error says to sign in again.
    pub async fn access<F, Fut>(&self, renew: F) -> Result<String>
    where
        F: FnOnce(Token) -> Fut,
        Fut: std::future::Future<Output = Result<std::result::Result<Token, Refused>>>,
    {
        let current = || self.get().ok_or_else(|| Error::msg(format!("Not signed in to {}", self.service)));
        let tok = current()?;
        if tok.fresh() {
            return Ok(tok.access);
        }
        let _one = self.refreshing.lock().await;
        // Refreshed by another caller while this one waited.
        let tok = current()?;
        if tok.fresh() {
            return Ok(tok.access);
        }
        match renew(tok).await? {
            Ok(t) => {
                let access = t.access.clone();
                self.set(Some(t))?;
                Ok(access)
            }
            Err(why) => {
                self.set(None)?;
                Err(Error::msg(format!("{} sign-in expired ({why}): sign in again", self.service)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn temp_path(name: &str) -> PathBuf {
        crate::testing::temp_dir(name).join("token.json")
    }

    fn expired() -> Token {
        Token { access: "old".into(), refresh: "r1".into(), expires: now_ms() - 1 }
    }

    #[tokio::test]
    async fn many_callers_share_one_refresh() {
        let path = temp_path("single-flight");
        let store = Arc::new(TokenStore::open(&path, "Google"));
        store.set(Some(expired())).unwrap();
        let refreshes = Arc::new(AtomicUsize::new(0));
        let mut calls = Vec::new();
        for _ in 0..8 {
            let (store, refreshes) = (store.clone(), refreshes.clone());
            calls.push(tokio::spawn(async move {
                store
                    .access(|old| async move {
                        refreshes.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                        Ok(Ok(Token { access: "new".into(), refresh: format!("{}+", old.refresh), expires: now_ms() + 3_600_000 }))
                    })
                    .await
                    .unwrap()
            }));
        }
        for c in calls {
            assert_eq!(c.await.unwrap(), "new");
        }
        assert_eq!(refreshes.load(Ordering::SeqCst), 1);
        // Saved, and read back by a new store.
        assert_eq!(TokenStore::open(&path, "Google").get().unwrap().refresh, "r1+");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_refused_refresh_signs_out() {
        let path = temp_path("refused");
        let store = TokenStore::open(&path, "Google");
        store.set(Some(expired())).unwrap();
        let e = store.access(|_| async { Ok(Err("invalid_grant (400)".to_string())) }).await.unwrap_err();
        assert_eq!(e.to_string(), "Google sign-in expired (invalid_grant (400)): sign in again");
        assert!(!store.signed_in());
        assert!(!path.exists());
    }

    #[test]
    fn the_old_launchers_token_file_loads() {
        let path = temp_path("old-format");
        std::fs::write(&path, r#"{"access":"a","refresh":"r","expires":1800000000000}"#).unwrap();
        let t = TokenStore::open(&path, "Google").get().unwrap();
        assert_eq!((t.access.as_str(), t.refresh.as_str(), t.expires), ("a", "r", 1_800_000_000_000));
        let _ = std::fs::remove_file(&path);
    }
}
