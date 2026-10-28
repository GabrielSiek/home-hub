//! Who hears which event. Everything a TV PC sends and everything it receives are separate lists,
//! so an event can never bounce back and forth between a TV PC and the server.

/// From the server to every screen and TV PC.
/// `pairing` carries only how many TV PCs wait to be approved; the codes need the admin PIN.
pub const TO_EVERYONE: &[&str] = &["smarthome", "companion", "clock", "calendar", "devices", "pairing"];

/// From the server to one browser: the TV PC it controls changed, or it fell behind and should
/// load everything again.
pub const TO_ONE: &[&str] = &["follow", "resync"];

/// From the server to TV PCs only (secrets their media modules need, unmasked).
pub const TO_AGENTS: &[&str] = &["settings"];

/// From a TV PC to the screens that control it.
pub const FROM_AGENT_TO_FOLLOWERS: &[&str] = &["mode", "tv", "watch", "watch-sync", "spotify", "mic-ready"];

/// From a TV PC to the server only.
pub const FROM_AGENT_TO_SERVER: &[&str] = &["device.context", "ring.note", "feed.sports"];

/// Events a TV PC may send; anything else it sends is dropped.
pub fn agent_may_send(name: &str) -> bool {
    FROM_AGENT_TO_FOLLOWERS.contains(&name) || FROM_AGENT_TO_SERVER.contains(&name)
}

/// Events the server passes on to the screens following the TV PC that sent them.
pub fn for_followers(name: &str) -> bool {
    FROM_AGENT_TO_FOLLOWERS.contains(&name)
}

/// Events a TV PC receives from the server.
pub fn agent_receives(name: &str) -> bool {
    TO_EVERYONE.contains(&name) || TO_AGENTS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_tv_pc_sends_never_comes_back_to_it() {
        for name in FROM_AGENT_TO_FOLLOWERS.iter().chain(FROM_AGENT_TO_SERVER) {
            assert!(!agent_receives(name), "{name} would loop");
            assert!(!TO_ONE.contains(name), "{name}");
        }
        assert!(agent_may_send("tv") && agent_receives("clock"));
        assert!(!agent_may_send("smarthome"), "only the server tells the house about devices");
    }
}
