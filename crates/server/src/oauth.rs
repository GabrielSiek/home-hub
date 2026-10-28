//! Sign-ins, with a server that has no browser: the server makes the consent page's address (with
//! its PKCE verifier and state, which never leave the server), a TV PC opens it in its own browser
//! and catches the redirect to 127.0.0.1 there (`oauth.loopback`), and the server checks the
//! state and exchanges the code.

use serde_json::json;
use tvl_kit::oauth::{google, random_string, Token};
use tvl_kit::{Error, Result};

use crate::ctx::Ctx;
use crate::router::Caller;

/// Where a sign-in started by `caller` opens: the TV PC that asked, or the one a phone or tablet
/// controls (it has to be on).
pub fn sign_in_device(app: &Ctx, caller: &Caller) -> Result<String> {
    if let Some(agent) = &caller.agent {
        return Ok(agent.clone());
    }
    app.devices
        .target(caller.screen.as_deref(), caller.target.as_deref())
        .map_err(|_| Error::msg("Sign in from a TV PC that's on: its browser opens Google's page"))
}

/// A Google account's token for `scopes`, signed in through TV PC `device`.
pub async fn google(app: &Ctx, device: &str, scopes: &str) -> Result<Token> {
    let client = crate::calendar::google_client(app)?;
    let verifier = random_string(64);
    let state = random_string(16);
    let url = google::consent_url(&client, scopes, &verifier, &state);
    let args = json!({
        "url": url,
        "port": google::REDIRECT_PORT,
        "path": google::REDIRECT_PATH,
        "service": "Google",
        "state": state,
    });
    let answer = app
        .agents
        .call(device, "oauth.loopback", args, None, 5 * 60_000, false)
        .await
        .map_err(|e| Error::msg(format!("The TV PC couldn't open the sign-in: {e:?}")))?;
    if answer["state"].as_str() != Some(state.as_str()) {
        return Err(Error::msg("The sign-in answered for another request: try again"));
    }
    if let Some(e) = answer["error"].as_str() {
        return Err(google::redirect_error(e));
    }
    let code = answer["code"].as_str().ok_or_else(|| Error::msg("Google sent no code: try again"))?;
    google::exchange(&tvl_kit::http::api(), &client, code, &verifier).await
}
