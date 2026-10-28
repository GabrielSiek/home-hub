//! The admin PIN, admin sessions on phones and tablets, and the TV PCs' pairing.
//!
//! Home Wi-Fi stays trusted for everyday use; settings, sign-ins and adding or removing devices
//! need the PIN. Paired TV PCs show it in their Settings (the user's choice), so it's kept as it
//! is, in admin.json in the data folder, which only the service's account can read. Session and
//! TV PC tokens are only kept as SHA-256 hashes.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use base64::Engine;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;
use tvl_kit::store::JsonStore;
use tvl_kit::time::{Clock, DAY, MIN};
use tvl_kit::{Error, Result};

pub const FILE: &str = "admin.json";
/// An unlocked phone or tablet stays admin this long.
const SESSION_MS: i64 = 30 * DAY;
/// Wrong PINs allowed per minute, across all screens, before unlocking pauses.
const TRIES_PER_MIN: usize = 5;
/// A TV PC's pairing code is good for this long.
const PAIR_MS: i64 = 10 * MIN;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentKey {
    device: String,
    token_sha256: String,
    paired_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionKey {
    token_sha256: String,
    expires: i64,
    /// The screen that unlocked it.
    screen: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    pin: String,
    #[serde(default)]
    agents: Vec<AgentKey>,
    #[serde(default)]
    sessions: Vec<SessionKey>,
}

/// A TV PC waiting for someone to approve it.
#[derive(Debug, Clone, Serialize)]
pub struct Pairing {
    pub code: String,
    pub device: String,
    pub name: String,
    pub since: i64,
}

struct Waiting {
    pairing: Pairing,
    /// Gets the token once approved, None when denied.
    answer: Option<oneshot::Sender<Option<String>>>,
}

#[derive(Debug, PartialEq)]
pub enum UnlockError {
    WrongPin,
    TooManyTries,
}

pub struct Admin {
    store: JsonStore<Saved>,
    clock: Arc<dyn Clock>,
    /// When the last wrong PINs were typed.
    misses: Mutex<VecDeque<i64>>,
    waiting: Mutex<HashMap<String, Waiting>>,
}

/// A 6-digit code from the OS's secure random source.
fn digits() -> String {
    let mut b = [0u8; 4];
    getrandom::fill(&mut b).expect("the OS random source works");
    format!("{:06}", u32::from_le_bytes(b) % 1_000_000)
}

/// 256 random bits, URL-safe.
fn new_token() -> String {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).expect("the OS random source works");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

fn hash(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Compares without stopping at the first difference, so timing says nothing about the PIN.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl Admin {
    pub fn open(dir: &std::path::Path, clock: Arc<dyn Clock>) -> Result<Self> {
        let store: JsonStore<Saved> = JsonStore::open(dir.join(FILE));
        if store.read(|s| s.pin.is_empty()) {
            store.change(|s| {
                s.pin = digits();
                Ok(())
            })?;
            tracing::info!("made an admin PIN: see it with `tvl-server pin` or in a TV PC's Settings");
        }
        Ok(Self { store, clock, misses: Mutex::default(), waiting: Mutex::default() })
    }

    pub fn pin(&self) -> String {
        self.store.read(|s| s.pin.clone())
    }

    /// A new PIN: 4 to 8 digits. Sessions already unlocked stay.
    pub fn set_pin(&self, pin: &str) -> Result<()> {
        let pin = pin.trim();
        if !(4..=8).contains(&pin.len()) || !pin.chars().all(|c| c.is_ascii_digit()) {
            return Err(Error::msg("The PIN is 4 to 8 digits"));
        }
        self.store.change(|s| {
            s.pin = pin.to_string();
            Ok(())
        })
    }

    pub fn reset_pin(&self) -> Result<String> {
        let pin = digits();
        self.set_pin(&pin)?;
        Ok(pin)
    }

    /// Unlocks admin on a phone or tablet: its session token and when it expires.
    pub fn unlock(&self, pin: &str, screen: Option<&str>) -> std::result::Result<(String, i64), UnlockError> {
        let now = self.clock.now_ms();
        {
            let mut misses = self.misses.lock();
            while misses.front().is_some_and(|t| now - t > MIN) {
                misses.pop_front();
            }
            if misses.len() >= TRIES_PER_MIN {
                return Err(UnlockError::TooManyTries);
            }
            if !same(pin.trim(), &self.pin()) {
                misses.push_back(now);
                return Err(UnlockError::WrongPin);
            }
        }
        let token = new_token();
        let expires = now + SESSION_MS;
        let key = SessionKey { token_sha256: hash(&token), expires, screen: screen.map(str::to_string) };
        let saved = self.store.change(|s| {
            s.sessions.retain(|x| x.expires > now);
            s.sessions.push(key);
            Ok(())
        });
        if let Err(e) = saved {
            tracing::error!("can't keep the admin session: {e}");
        }
        Ok((token, expires))
    }

    pub fn session_ok(&self, token: &str) -> bool {
        let (h, now) = (hash(token), self.clock.now_ms());
        self.store.read(|s| s.sessions.iter().any(|x| x.token_sha256 == h && x.expires > now))
    }

    /// The TV PC a token belongs to.
    pub fn agent_for(&self, token: &str) -> Option<String> {
        let h = hash(token);
        self.store.read(|s| s.agents.iter().find(|a| a.token_sha256 == h).map(|a| a.device.clone()))
    }

    /// A TV PC asks to join: the code it shows, and where the answer comes. Asking again replaces
    /// its earlier request.
    pub fn request_pairing(&self, device: &str, name: &str) -> (String, oneshot::Receiver<Option<String>>) {
        let now = self.clock.now_ms();
        let (tx, rx) = oneshot::channel();
        let mut waiting = self.waiting.lock();
        waiting.retain(|_, w| w.pairing.device != device && now - w.pairing.since < PAIR_MS);
        let code = loop {
            let c = digits();
            if !waiting.contains_key(&c) {
                break c;
            }
        };
        let pairing = Pairing { code: code.clone(), device: device.to_string(), name: name.trim().chars().take(40).collect(), since: now };
        waiting.insert(code.clone(), Waiting { pairing, answer: Some(tx) });
        (code, rx)
    }

    pub fn pairings(&self) -> Vec<Pairing> {
        let now = self.clock.now_ms();
        let mut w = self.waiting.lock();
        w.retain(|_, w| now - w.pairing.since < PAIR_MS);
        w.values().map(|w| w.pairing.clone()).collect()
    }

    /// Approves the TV PC showing `code`: it gets its token.
    pub fn approve(&self, code: &str) -> Result<Pairing> {
        let mut w = self.waiting.lock().remove(code.trim()).ok_or_else(|| Error::msg("No TV PC is showing that code"))?;
        if self.clock.now_ms() - w.pairing.since >= PAIR_MS {
            return Err(Error::msg("That code expired: restart the pairing on the TV"));
        }
        let token = new_token();
        let device = w.pairing.device.clone();
        let now = self.clock.now_ms();
        self.store.change(|s| {
            s.agents.retain(|a| a.device != device);
            s.agents.push(AgentKey { device: device.clone(), token_sha256: hash(&token), paired_at: now });
            Ok(())
        })?;
        if let Some(answer) = w.answer.take() {
            let _ = answer.send(Some(token));
        }
        Ok(w.pairing)
    }

    pub fn deny(&self, code: &str) -> Result<()> {
        let mut w = self.waiting.lock().remove(code.trim()).ok_or_else(|| Error::msg("No TV PC is showing that code"))?;
        if let Some(answer) = w.answer.take() {
            let _ = answer.send(None);
        }
        Ok(())
    }

    /// A TV PC forgotten: its token stops working.
    pub fn revoke(&self, device: &str) -> Result<()> {
        self.store.change(|s| {
            s.agents.retain(|a| a.device != device);
            Ok(())
        })
    }

    /// A pairing that doesn't wait for a person: tests, and `tvl-server pair` on the server
    /// itself. Returns the token.
    pub fn pair_now(&self, device: &str) -> Result<String> {
        let token = new_token();
        let now = self.clock.now_ms();
        self.store.change(|s| {
            s.agents.retain(|a| a.device != device);
            s.agents.push(AgentKey { device: device.to_string(), token_sha256: hash(&token), paired_at: now });
            Ok(())
        })?;
        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tvl_kit::time::FakeClock;

    fn admin() -> (Admin, Arc<FakeClock>, std::path::PathBuf) {
        let dir = tvl_kit::testing::temp_dir("admin");
        let clock = Arc::new(FakeClock::new(1_000 * DAY));
        (Admin::open(&dir, clock.clone()).unwrap(), clock, dir)
    }

    #[test]
    fn a_pin_is_made_once_and_kept() {
        let (a, clock, dir) = admin();
        let pin = a.pin();
        assert_eq!(pin.len(), 6);
        assert_eq!(Admin::open(&dir, clock).unwrap().pin(), pin);
        assert!(a.set_pin("12a4").is_err());
        a.set_pin("2468").unwrap();
        assert_eq!(a.pin(), "2468");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_right_pin_unlocks_for_a_month_and_guessing_is_slowed() {
        let (a, clock, dir) = admin();
        a.set_pin("1357").unwrap();
        for _ in 0..TRIES_PER_MIN {
            assert_eq!(a.unlock("0000", Some("phone")), Err(UnlockError::WrongPin));
        }
        // Even the right PIN waits out the minute.
        assert_eq!(a.unlock("1357", Some("phone")), Err(UnlockError::TooManyTries));
        clock.advance(MIN + 1);
        let (token, expires) = a.unlock("1357", Some("phone")).unwrap();
        assert!(a.session_ok(&token));
        assert!(!a.session_ok("made-up"));
        // Only the hash is on disk.
        let file = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(!file.contains(&token));
        clock.set(expires + 1);
        assert!(!a.session_ok(&token));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn an_approved_tv_pc_gets_a_token_that_works_until_revoked() {
        let (a, _clock, dir) = admin();
        let (code, answer) = a.request_pairing("pc-quarto", "DESKTOP-QUARTO");
        assert_eq!(a.pairings().len(), 1);
        assert!(a.approve("000000x").is_err());
        let p = a.approve(&code).unwrap();
        assert_eq!(p.device, "pc-quarto");
        let token = answer.await.unwrap().unwrap();
        assert_eq!(a.agent_for(&token).as_deref(), Some("pc-quarto"));
        assert!(a.pairings().is_empty());
        a.revoke("pc-quarto").unwrap();
        assert_eq!(a.agent_for(&token), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn a_denied_or_expired_pairing_gets_nothing() {
        let (a, clock, dir) = admin();
        let (code, answer) = a.request_pairing("pc", "PC");
        a.deny(&code).unwrap();
        assert_eq!(answer.await.unwrap(), None);
        let (code, _answer) = a.request_pairing("pc", "PC");
        clock.advance(PAIR_MS);
        assert!(a.approve(&code).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
