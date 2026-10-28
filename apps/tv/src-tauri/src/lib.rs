mod agent;
mod assistant;
mod bridge;
mod cdp;
mod commands;
mod config;
mod cursor;
mod device;
mod error;
mod focus_win;
mod games;
mod guide_guard;
mod history;
mod input;
mod launcher;
mod live;
mod livetv;
mod overlay;
mod photos;
mod recs;
mod retro;
mod ring;
mod spotify;
mod sports;
mod state;
mod tmdb;
mod twitch;
mod vdf;
mod watch;
mod youtube;

use tauri::Manager;

/// Passive: next to the old launcher (or on a developer's desk). A window rather than the whole
/// screen, and hands off the pad, the Guide button (both launchers would flip the same Game Bar
/// setting) and the mouse pointer. `TVL_PASSIVE=1`, or by itself while the old launcher runs.
fn passive() -> bool {
    std::env::var_os("TVL_PASSIVE").is_some_and(|v| v != "0") || old_launcher_running()
}

/// The old launcher holds the mutex its single-instance plugin names after its identifier.
#[cfg(windows)]
fn old_launcher_running() -> bool {
    use windows::core::w;
    use windows::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_ACCESS_RIGHTS};
    const SYNCHRONIZE: SYNCHRONIZATION_ACCESS_RIGHTS = SYNCHRONIZATION_ACCESS_RIGHTS(0x0010_0000);
    // SAFETY: opens (and drops) a handle to a named mutex, if there is one.
    match unsafe { OpenMutexW(SYNCHRONIZE, false, w!("dev.tvlauncher.app-sim")) } {
        Ok(h) => {
            // SAFETY: the handle just opened.
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(h) };
            true
        }
        Err(_) => false,
    }
}

#[cfg(not(windows))]
fn old_launcher_running() -> bool {
    false
}

/// Gives the generated handler its type, so it can be kept and called from the router.
fn handler<F: Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static>(f: F) -> F {
    f
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let passive = passive();
    let local = handler(tauri::generate_handler![
        commands::list_apps,
        commands::launch_app,
        commands::launch_app_at,
        commands::close_app,
        commands::get_status,
        commands::get_history,
        commands::forget_opened,
        commands::record_search,
        commands::tmdb_search,
        commands::tmdb_providers,
        commands::tmdb_discover,
        commands::tmdb_search_on,
        commands::tmdb_details,
        commands::tmdb_season,
        commands::tmdb_person,
        commands::tmdb_person_services,
        commands::watch_play,
        commands::watch_pick_profile,
        commands::watch_profile,
        commands::watch_progress,
        commands::watch_dismiss,
        commands::watch_sync,
        commands::watch_title,
        commands::watchlist,
        commands::watchlist_toggle,
        commands::recs_shelves,
        commands::recs_row,
        commands::recs_genre,
        commands::rating_get,
        commands::rating_set,
        commands::get_settings,
        commands::save_settings,
        commands::reload_config,
        commands::get_autostart,
        commands::set_autostart,
        commands::open_data_dir,
        commands::list_games,
        commands::launch_game,
        commands::toggle_favorite,
        commands::retro_get,
        commands::retro_launch,
        commands::retro_install,
        commands::retro_cancel,
        commands::retro_remove,
        commands::retro_link,
        commands::retro_set_folders,
        commands::retro_set_bios,
        commands::retro_scan_bios,
        commands::retro_add_cores,
        commands::retro_check_updates,
        commands::retro_open_bios,
        commands::retro_browse,
        commands::retro_browse_start,
        commands::retro_check,
        commands::spotify_get,
        commands::spotify_sign_in,
        commands::spotify_sign_out,
        commands::spotify_control,
        commands::spotify_play,
        commands::spotify_shelf,
        commands::spotify_playlist,
        commands::spotify_search,
        commands::spotify_artist,
        commands::spotify_artist_popular,
        commands::spotify_play_tracks,
        commands::spotify_show,
        commands::spotify_oldest_episodes,
        commands::spotify_follow,
        commands::twitch_get,
        commands::twitch_sign_in,
        commands::twitch_sign_out,
        commands::twitch_streams,
        commands::twitch_categories,
        commands::twitch_search,
        commands::twitch_channel,
        commands::twitch_send_chat,
        commands::press_space,
        commands::assistant_listen,
        commands::assistant_set_speech,
        commands::assistant_listen_stop,
        commands::assistant_listen_cancel,
        commands::assistant_speak,
        commands::assistant_hush,
        commands::assistant_voice_try,
        commands::assistant_voice_model,
        commands::assistant_voice_prepare,
        commands::photos_get,
        commands::photos_slideshow,
        commands::photos_google_sign_in,
        commands::photos_google_sign_out,
        commands::photos_google_pick,
        commands::photos_pick_here,
        commands::photos_cancel_pick,
        commands::photos_add_folder,
        commands::photos_set_enabled,
        commands::photos_remove,
        commands::sports_home,
        commands::sports_game,
        commands::sports_competition,
        commands::sports_watch,
        commands::sports_tv,
        commands::sports_watch_tv,
        commands::sports_highlights,
        commands::sports_remind,
        commands::sports_set_overlay,
        commands::live_now,
        commands::live_tv,
        commands::live_watch,
        commands::live_favorite,
        commands::live_remind,
        commands::live_news,
        commands::live_news_search,
        commands::live_news_add,
        commands::live_news_remove,
        commands::youtube_get,
        commands::youtube_sign_in,
        commands::youtube_sign_out,
        commands::youtube_watch,
        commands::link_status,
        commands::link_address,
        commands::link_pair_again,
    ]);
    tauri::Builder::default()
        // Must be first: a second launch just brings the running launcher forward.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            focus_win::bring_to_front(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            // Logs to a file (a release build has no console); kept for as long as the app runs.
            let log = tvl_kit::log::init(Some(&data_dir.join("logs")), "tv");
            std::mem::forget(log);
            // Developer keys from the project's .env: debug builds only. An installed app gets
            // the household's keys from the server.
            #[cfg(debug_assertions)]
            let _ = dotenvy::dotenv();
            let config = config::Config::load_or_init(&data_dir)?;
            if passive {
                tracing::info!("passive: windowed, without the pad, the Guide button or the pointer");
            } else {
                // Undone on exit, below.
                guide_guard::disable_game_bar(&data_dir);
            }
            app.manage(state::AppState::new(data_dir, config));

            // TV_LAUNCHER_WINDOWED=1 keeps a normal window while developing.
            if !passive && std::env::var_os("TV_LAUNCHER_WINDOWED").is_none() {
                if let Some(w) = app.get_webview_window("main") {
                    w.set_fullscreen(true)?;
                }
            }

            games::allow_steam_art(app.handle());
            games::spawn_watcher(app.handle().clone());
            spotify::start(app.handle());
            twitch::start(app.handle());
            photos::start(app.handle());
            sports::start(app.handle());
            livetv::start(app.handle());
            ring::start(app.handle());
            retro::start(app.handle());
            agent::start(app.handle());
            if !passive {
                input::spawn(app.handle().clone());
                cursor::spawn(app.handle().clone());
            }
            Ok(())
        })
        .invoke_handler(move |invoke| agent::invoke::route(invoke, &local))
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(link) = agent::link(app) {
                    link.stop();
                }
                if !passive {
                    if let Ok(dir) = app.path().app_data_dir() {
                        guide_guard::restore_game_bar(&dir);
                    }
                }
            }
        });
}
