//! Every command, once: who runs it and who may call it. It replaces the four lists the old
//! launcher kept by hand (the Tauri handler in `lib.rs`, `commands.rs`, the companion's match in
//! `companion/api.rs`, and `api.ts`), which had drifted: 73 of the 157 commands were reachable
//! over HTTP and nothing said which.
//!
//! Moving a domain to the server (M2) changes `owner` here, together with the port and the data
//! import that make it true.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Where a command runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// On the server.
    Server,
    /// On a TV PC: the caller's own (from its UI), or the one a phone or tablet controls.
    Device,
    /// On the server, which asks a TV PC to take part (opening a sign-in page there).
    Composite,
}

/// Who may call it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Any screen in the house.
    Public,
    /// A paired TV PC, the server's own `localhost`, or a browser holding an admin session (PIN).
    Admin,
    /// Only a TV PC's own UI. Phones and tablets can't, as they couldn't with the old launcher.
    TvOnly,
}

#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    pub name: &'static str,
    pub owner: Owner,
    pub access: Access,
    /// Only reads: safe to ask again after a dropped connection.
    pub idempotent: bool,
    /// Must reach the TV PC in the order sent (remote keys and text).
    pub ordered: bool,
    /// How long the caller waits for the answer.
    pub timeout_ms: u32,
}

const TIMEOUT: u32 = 10_000;
/// Opening a browser, finding a title's address and starting it.
const PLAY: u32 = 60_000;
/// Listings that read several web pages or APIs.
const SLOW: u32 = 30_000;
/// A sign-in page waits for the person to finish.
const SIGN_IN: u32 = 5 * 60_000;

const fn cmd(name: &'static str, owner: Owner, access: Access) -> CommandSpec {
    CommandSpec { name, owner, access, idempotent: false, ordered: false, timeout_ms: TIMEOUT }
}

/// Runs on the server; any screen.
const fn server(name: &'static str) -> CommandSpec {
    cmd(name, Owner::Server, Access::Public)
}

/// Runs on a TV PC; any screen (phones and tablets reach the one they control).
const fn device(name: &'static str) -> CommandSpec {
    cmd(name, Owner::Device, Access::Public)
}

/// Runs on a TV PC; only from its own UI.
const fn tv(name: &'static str) -> CommandSpec {
    cmd(name, Owner::Device, Access::TvOnly)
}

impl CommandSpec {
    const fn read(mut self) -> Self {
        self.idempotent = true;
        self
    }

    const fn ordered(mut self) -> Self {
        self.ordered = true;
        self
    }

    const fn waits(mut self, ms: u32) -> Self {
        self.timeout_ms = ms;
        self
    }

    const fn admin(mut self) -> Self {
        self.access = Access::Admin;
        self
    }

    const fn tv_only(mut self) -> Self {
        self.access = Access::TvOnly;
        self
    }

    pub fn runs_on_server(&self) -> bool {
        matches!(self.owner, Owner::Server | Owner::Composite)
    }
}

pub static COMMANDS: &[CommandSpec] = &[
    // ---------- Apps, what's on ----------
    device("list_apps").read(),
    device("launch_app").waits(PLAY),
    device("launch_app_at").waits(PLAY),
    device("close_app").waits(SLOW),
    device("get_status").read(),
    device("get_history").read(),
    tv("forget_opened"),
    tv("record_search"),
    // ---------- The catalog (TMDB) ----------
    device("tmdb_search").read().waits(SLOW),
    device("tmdb_providers").read().waits(SLOW),
    device("tmdb_discover").read().waits(SLOW),
    device("tmdb_search_on").read().waits(SLOW),
    device("tmdb_details").read().waits(SLOW),
    device("tmdb_season").read().waits(SLOW),
    tv("tmdb_person").read().waits(SLOW),
    tv("tmdb_person_services").read().waits(SLOW),
    // ---------- Watching, recommendations ----------
    device("watch_play").waits(PLAY),
    tv("watch_pick_profile"),
    tv("watch_profile").read(),
    device("watch_progress").read(),
    tv("watch_dismiss"),
    tv("watch_sync"),
    device("watch_title").read(),
    device("watchlist").read(),
    device("watchlist_toggle"),
    device("recs_shelves").read().waits(SLOW),
    device("recs_row").read().waits(SLOW),
    tv("recs_genre").read().waits(SLOW),
    tv("rating_get").read(),
    tv("rating_set"),
    // ---------- The TV PC's own settings (its agent merges in the server's household ones) ----------
    tv("get_settings").read(),
    tv("save_settings"),
    tv("reload_config"),
    tv("get_autostart").read(),
    tv("set_autostart"),
    tv("open_data_dir"),
    // ---------- Games and emulators ----------
    tv("list_games").read(),
    tv("launch_game").waits(PLAY),
    tv("toggle_favorite"),
    tv("retro_get").read(),
    tv("retro_launch").waits(PLAY),
    tv("retro_install"),
    tv("retro_cancel"),
    tv("retro_remove"),
    tv("retro_link"),
    tv("retro_set_folders"),
    tv("retro_set_bios"),
    tv("retro_scan_bios"),
    tv("retro_add_cores"),
    tv("retro_check_updates").waits(SLOW),
    tv("retro_open_bios"),
    tv("retro_browse").read(),
    tv("retro_browse_start").read(),
    tv("retro_check").read(),
    // ---------- Smart home (the server is the only machine talking to the devices) ----------
    server("smarthome_get").read(),
    server("smarthome_control"),
    server("smarthome_watch").tv_only(),
    server("smarthome_refresh"),
    server("smarthome_run_scene").waits(SLOW),
    server("smarthome_save_scene"),
    server("smarthome_delete_scene"),
    server("smarthome_put_scene"),
    server("smarthome_put_automation"),
    server("smarthome_delete_automation"),
    server("smarthome_set_layout"),
    server("smarthome_control_room").waits(SLOW),
    server("smarthome_add_room"),
    server("smarthome_rename_room"),
    server("smarthome_delete_room"),
    server("smarthome_put_task"),
    server("smarthome_task_done"),
    server("smarthome_delete_task"),
    server("smarthome_put_screen"),
    // Removing a screen (or a TV PC: its pairing is revoked) needs the admin PIN.
    server("smarthome_forget_screen").admin(),
    cmd("ewelink_sign_in", Owner::Composite, Access::Admin).waits(SIGN_IN),
    server("ewelink_sign_out").admin(),
    // ---------- Music ----------
    device("spotify_get").read(),
    tv("spotify_sign_in").waits(SIGN_IN),
    tv("spotify_sign_out"),
    device("spotify_control"),
    device("spotify_play").waits(SLOW),
    device("spotify_shelf").read().waits(SLOW),
    device("spotify_playlist").read().waits(SLOW),
    device("spotify_search").read().waits(SLOW),
    device("spotify_artist").read().waits(SLOW),
    tv("spotify_artist_popular").read().waits(SLOW),
    device("spotify_play_tracks").waits(SLOW),
    device("spotify_show").read().waits(SLOW),
    tv("spotify_oldest_episodes").read().waits(SLOW),
    tv("spotify_follow"),
    device("spotify_devices").read(),
    device("spotify_transfer"),
    device("spotify_queue").read(),
    // ---------- Twitch ----------
    device("twitch_get").read(),
    tv("twitch_sign_in").waits(SIGN_IN),
    tv("twitch_sign_out"),
    device("twitch_streams").read().waits(SLOW),
    tv("twitch_categories").read().waits(SLOW),
    tv("twitch_search").read().waits(SLOW),
    tv("twitch_channel").read().waits(SLOW),
    tv("twitch_send_chat"),
    tv("press_space"),
    // ---------- Assistant: the brain on the server; microphone and voice on a TV PC ----------
    server("assistant_ask").waits(PLAY),
    // What a TV PC's microphone heard, made into words on the server (pages POST the WAV instead).
    server("assistant_transcribe").tv_only().waits(SLOW),
    device("assistant_listen").waits(PLAY),
    tv("assistant_set_speech"),
    device("assistant_listen_stop"),
    device("assistant_listen_cancel"),
    tv("assistant_speak").waits(SLOW),
    tv("assistant_hush"),
    tv("assistant_voice_try").waits(SLOW),
    tv("assistant_voice_model").read(),
    tv("assistant_voice_prepare"),
    // ---------- Photos (screensaver) ----------
    tv("photos_get").read(),
    tv("photos_slideshow").read(),
    tv("photos_google_sign_in").waits(SIGN_IN),
    tv("photos_google_sign_out"),
    tv("photos_google_pick").waits(SIGN_IN),
    tv("photos_pick_here").waits(SIGN_IN),
    tv("photos_cancel_pick"),
    tv("photos_add_folder").waits(SLOW),
    tv("photos_set_enabled"),
    tv("photos_remove"),
    // ---------- Sports ----------
    device("sports_home").read().waits(SLOW),
    tv("sports_game").read().waits(SLOW),
    tv("sports_competition").read().waits(SLOW),
    device("sports_watch").waits(PLAY),
    device("sports_tv").read().waits(SLOW),
    device("sports_watch_tv").waits(PLAY),
    tv("sports_highlights").read().waits(SLOW),
    tv("sports_remind"),
    tv("sports_set_overlay"),
    // ---------- Live TV ----------
    device("live_now").read().waits(SLOW),
    device("live_tv").read().waits(SLOW),
    device("live_watch").waits(PLAY),
    device("live_favorite"),
    tv("live_remind"),
    tv("live_news").read().waits(SLOW),
    tv("live_news_search").read().waits(SLOW),
    tv("live_news_add"),
    tv("live_news_remove"),
    // ---------- YouTube ----------
    tv("youtube_get").read(),
    tv("youtube_sign_in").waits(SIGN_IN),
    tv("youtube_sign_out"),
    device("youtube_watch").waits(PLAY),
    // ---------- Calendar ----------
    server("calendar_get").read().waits(SLOW),
    server("calendar_next").read(),
    server("calendar_link").read(),
    cmd("calendar_sign_in", Owner::Composite, Access::Admin).waits(SIGN_IN),
    server("calendar_sign_out").admin(),
    server("calendar_show").tv_only(),
    server("calendar_remind"),
    server("calendar_travel").tv_only().waits(SLOW),
    server("calendar_qr").tv_only().read(),
    // ---------- Timers and alarms ----------
    server("clock_get").read(),
    server("clock_timer_start"),
    server("clock_timer"),
    server("clock_alarm_save"),
    server("clock_alarm_on"),
    server("clock_alarm_delete"),
    server("clock_respond"),
    // A TV PC rang something itself while the server was out of reach (its fail-safe).
    server("clock_rang_locally").tv_only(),
    // ---------- The companion's own ----------
    server("companion_get").read(),
    server("companion_set_hub"),
    server("companion_hello"),
    server("client_log"),
    device("tv_now").read(),
    // Volume drags send many; the latest one wins.
    device("tv_volume").ordered(),
    device("tv_off"),
    device("remote_key").ordered(),
    device("remote_text").ordered(),
    device("tv_play").waits(PLAY),
    // ---------- The house's screens, the server, admin ----------
    server("devices_get").read(),
    // A phone or tablet picks the TV PC it controls (None: its room's).
    server("screen_follow"),
    // Wake-on-LAN for a TV PC that's off.
    server("device_wake"),
    server("server_status").read(),
    server("admin_unlock"),
    server("admin_check").read(),
    server("admin_pin_set").admin(),
    server("pair_list").admin().read(),
    server("pair_approve").admin(),
    server("pair_deny").admin(),
    server("server_settings_get").admin().read(),
    server("server_settings_save").admin(),
    // A sign-in finished on a phone: the 127.0.0.1 address it couldn't open, pasted.
    server("oauth_paste").admin(),
];

fn index() -> &'static HashMap<&'static str, &'static CommandSpec> {
    static INDEX: OnceLock<HashMap<&'static str, &'static CommandSpec>> = OnceLock::new();
    INDEX.get_or_init(|| COMMANDS.iter().map(|c| (c.name, c)).collect())
}

/// The command called `name`, or None: an unknown name is refused, never guessed at.
pub fn spec(name: &str) -> Option<&'static CommandSpec> {
    index().get(name).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    /// The old launcher's source, read (never written) to check nothing was left out. Skipped if
    /// it's been removed.
    fn old_source(rel: &str) -> Option<String> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src-tauri/src").join(rel);
        std::fs::read_to_string(path).ok()
    }

    /// `commands::name,` lines of lib.rs's generate_handler!.
    fn tauri_commands(lib_rs: &str) -> BTreeSet<String> {
        let start = lib_rs.find("generate_handler![").expect("generate_handler! in lib.rs");
        let end = start + lib_rs[start..].find(']').unwrap();
        lib_rs[start..end]
            .lines()
            .filter_map(|l| l.trim().strip_prefix("commands::"))
            .map(|n| n.trim_end_matches(',').to_string())
            .collect()
    }

    /// `"name" =>` arms of companion/api.rs's dispatch.
    fn companion_commands(api_rs: &str) -> BTreeSet<String> {
        api_rs
            .lines()
            .filter_map(|l| {
                let l = l.trim();
                let rest = l.strip_prefix('"')?;
                let (name, after) = rest.split_once('"')?;
                after.trim_start().starts_with("=>").then(|| name.to_string())
            })
            .collect()
    }

    #[test]
    fn names_are_unique() {
        let mut seen = BTreeSet::new();
        for c in COMMANDS {
            assert!(seen.insert(c.name), "{} twice", c.name);
        }
        assert_eq!(index().len(), COMMANDS.len());
    }

    #[test]
    fn every_old_tauri_command_is_here() {
        let Some(lib_rs) = old_source("lib.rs") else { return };
        let old = tauri_commands(&lib_rs);
        assert_eq!(old.len(), 157);
        let missing: Vec<_> = old.iter().filter(|n| spec(n).is_none()).collect();
        assert!(missing.is_empty(), "not in the table: {missing:?}");
    }

    #[test]
    fn browsers_reach_what_the_companion_did_and_nothing_more() {
        let Some(api_rs) = old_source("companion/api.rs") else { return };
        let mut before = companion_commands(&api_rs);
        assert!(before.contains("remote_key") && before.contains("smarthome_get"));
        // Removing a screen now needs the admin PIN (the user's choice).
        assert!(before.remove("smarthome_forget_screen"));
        let new: BTreeSet<String> =
            ["devices_get", "screen_follow", "device_wake", "server_status", "admin_unlock", "admin_check"]
                .into_iter()
                .map(String::from)
                .collect();
        let now: BTreeSet<String> =
            COMMANDS.iter().filter(|c| c.access == Access::Public).map(|c| c.name.to_string()).collect();
        let opened: Vec<_> = now.difference(&before).filter(|n| !new.contains(*n)).collect();
        assert!(opened.is_empty(), "newly open to every screen: {opened:?}");
        let closed: Vec<_> = before.difference(&now).collect();
        assert!(closed.is_empty(), "no longer open to the companion: {closed:?}");
    }

    #[test]
    fn remote_keys_keep_their_order_and_plays_wait_long_enough() {
        assert!(spec("remote_key").unwrap().ordered);
        assert!(spec("remote_text").unwrap().ordered);
        assert!(!spec("smarthome_control").unwrap().idempotent);
        assert!(spec("smarthome_get").unwrap().idempotent);
        assert_eq!(spec("watch_play").unwrap().timeout_ms, PLAY);
        assert!(spec("nonsense").is_none());
    }

    #[test]
    fn sign_ins_are_admin_only() {
        for c in COMMANDS.iter().filter(|c| c.name.ends_with("_sign_in") && c.runs_on_server()) {
            assert_eq!(c.access, Access::Admin, "{}", c.name);
        }
    }
}
