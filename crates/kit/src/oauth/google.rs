//! Google sign-in with a "Desktop app" OAuth client and PKCE. Google's device flow (a code typed
//! on the phone) doesn't allow the Photos picker scope, so the consent page opens in a TV PC's
//! browser and redirects to 127.0.0.1 there. The server makes the address and exchanges the code;
//! the TV PC only catches the redirect.

use serde::Deserialize;
use serde_json::Value;

use super::{challenge, Token};
use crate::error::{Error, Result};
use crate::time::now_ms;

pub const AUTH: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const REDIRECT_PORT: u16 = 47952;
/// Desktop app clients take any port on 127.0.0.1 without listing it in Google Cloud.
pub const REDIRECT_URL: &str = "http://127.0.0.1:47952/google";
pub const REDIRECT_PATH: &str = "/google";

/// The "Desktop app" client from Google Cloud. Desktop clients need the secret too; Google
/// doesn't treat it as secret for installed apps.
#[derive(Debug, Clone)]
pub struct Client {
    pub id: String,
    pub secret: String,
}

/// The consent page for `scopes` (space-separated). `offline` + `consent` give a refresh token
/// every time, so a screen never has to sign in again; `include_granted_scopes` lets one account
/// add Calendar, YouTube and Photos to the same token over time.
pub fn consent_url(client: &Client, scopes: &str, verifier: &str, state: &str) -> String {
    let enc = |s: &str| urlencoding::encode(s).into_owned();
    format!(
        "{AUTH}?response_type=code&client_id={}&scope={}&redirect_uri={}&code_challenge_method=S256\
         &code_challenge={}&state={}&access_type=offline&prompt=consent&include_granted_scopes=true",
        enc(&client.id),
        enc(scopes),
        enc(REDIRECT_URL),
        challenge(verifier),
        enc(state),
    )
}

/// The redirect's query turned into a token: the code exchanged with the verifier the consent
/// address was made with.
pub async fn exchange(http: &reqwest::Client, client: &Client, code: &str, verifier: &str) -> Result<Token> {
    let form = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", REDIRECT_URL),
        ("client_id", client.id.as_str()),
        ("client_secret", client.secret.as_str()),
        ("code_verifier", verifier),
    ];
    let t = token_request(http, &form).await?.map_err(|why| Error::msg(format!("Google sign-in: {why}")))?;
    let refresh = t
        .refresh_token
        .ok_or_else(|| Error::msg("Google sign-in: no refresh token came back; try again"))?;
    Ok(Token { access: t.access_token, refresh, expires: now_ms() + t.expires_in * 1000 })
}

/// A new access token for `tok`. Ok(Err(why)) when Google won't refresh it: revoked, a new
/// client, or a consent screen still in Testing (its tokens last a week).
pub async fn renew(http: &reqwest::Client, client: &Client, tok: &Token) -> Result<std::result::Result<Token, String>> {
    let form = [
        ("grant_type", "refresh_token"),
        ("refresh_token", tok.refresh.as_str()),
        ("client_id", client.id.as_str()),
        ("client_secret", client.secret.as_str()),
    ];
    Ok(token_request(http, &form).await?.map(|t| Token {
        access: t.access_token,
        refresh: t.refresh_token.unwrap_or_else(|| tok.refresh.clone()),
        expires: now_ms() + t.expires_in * 1000,
    }))
}

/// The error a redirect carries ("access_denied" when the person said no), as a message.
pub fn redirect_error(error: &str) -> Error {
    Error::msg(if error == "access_denied" {
        "Google sign-in cancelled".to_string()
    } else {
        format!("Google sign-in failed: {error}")
    })
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
}

/// Ok(Err(why)) when Google turns the request down (a 4xx), e.g. a revoked refresh token.
async fn token_request(
    http: &reqwest::Client,
    form: &[(&str, &str)],
) -> Result<std::result::Result<TokenResponse, String>> {
    let r = http.post(TOKEN_URL).form(form).send().await?;
    let status = r.status();
    if status.is_success() {
        return Ok(Ok(r.json().await?));
    }
    if !status.is_client_error() {
        return Err(Error::msg(format!("Google accounts: {status}")));
    }
    let body: Value = r.json().await.unwrap_or_default();
    let why = body["error_description"].as_str().or(body["error"].as_str()).unwrap_or("");
    Ok(Err(format!("{why} ({status})")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_consent_address_carries_pkce_state_and_offline_access() {
        let client = Client { id: "id.apps.googleusercontent.com".into(), secret: "TOP-SECRET-VALUE".into() };
        let url = consent_url(&client, "https://www.googleapis.com/auth/calendar.readonly", "verifier", "st+1");
        assert!(url.starts_with(AUTH));
        assert!(url.contains("client_id=id.apps.googleusercontent.com"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A47952%2Fgoogle"));
        assert!(url.contains(&format!("code_challenge={}", challenge("verifier"))));
        assert!(url.contains("state=st%2B1"));
        assert!(url.contains("access_type=offline"));
        assert!(!url.contains("TOP-SECRET"), "the secret never goes in the address");
    }

    #[test]
    fn a_refusal_reads_as_cancelled() {
        assert_eq!(redirect_error("access_denied").to_string(), "Google sign-in cancelled");
        assert_eq!(redirect_error("invalid_scope").to_string(), "Google sign-in failed: invalid_scope");
    }
}
