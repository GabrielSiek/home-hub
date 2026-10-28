//! OAuth sign-ins, split between the two machines that take part: the server builds the consent
//! address, exchanges the code and keeps the token (`token`, `google`); a TV PC opens the address
//! in its browser and catches the redirect to 127.0.0.1 (`loopback`).

pub mod google;
pub mod loopback;
pub mod token;

use std::collections::HashMap;

use base64::Engine;
use sha2::{Digest, Sha256};

pub use token::{Token, TokenStore};

/// `n` random characters that are safe in a URL (the PKCE verifier alphabet), from the OS's
/// secure random source. Also the `state` of every sign-in.
pub fn random_string(n: usize) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes).expect("the OS random source works");
    bytes.iter().map(|b| CHARS[(b & 63) as usize] as char).collect()
}

/// The PKCE S256 challenge for `verifier`.
pub fn challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub fn parse_query(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            Some((k.to_string(), urlencoding::decode(v).ok()?.into_owned()))
        })
        .collect()
}

/// The query of a redirect address, from what a browser shows when 127.0.0.1 doesn't answer
/// ("http://127.0.0.1:47952/google?state=…&code=…"): the fallback when the sign-in was opened on
/// a phone and its address is pasted into the companion.
pub fn query_of(address: &str) -> HashMap<String, String> {
    address.trim().split_once('?').map(|(_, q)| parse_query(q.split('#').next().unwrap_or(q))).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_query_is_decoded() {
        let q = parse_query("code=ab%2Bc&region=us&state=xyz");
        assert_eq!(q["code"], "ab+c");
        assert_eq!(q["region"], "us");
    }

    #[test]
    fn pkce_challenge_matches_rfc7636() {
        // RFC 7636, appendix B.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn random_strings_are_url_safe() {
        let s = random_string(64);
        assert_eq!(s.len(), 64);
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_ne!(s, random_string(64));
    }

    #[test]
    fn a_pasted_redirect_address_gives_its_query() {
        let q = query_of(" http://127.0.0.1:47952/google?state=abc&code=4%2F0Ad#frag ");
        assert_eq!(q["state"], "abc");
        assert_eq!(q["code"], "4/0Ad");
        assert!(query_of("no query here").is_empty());
    }
}
