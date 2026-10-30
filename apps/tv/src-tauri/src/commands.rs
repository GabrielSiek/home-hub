use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

use crate::assistant;
use crate::config::{AppEntry, Config, Settings, Speech};
use crate::error::{Error, Result};
use crate::games;
use crate::history::History;
use crate::launcher;
use crate::live;
use crate::livetv;
use crate::photos;
use crate::recs;
use crate::retro;
use crate::spotify;
use crate::sports;
use crate::state::{AppState, Mode};
use crate::tmdb;
use crate::twitch;
use crate::watch;
use crate::youtube;

#[derive(Serialize)]
pub struct Status {
    mode: Mode,
    running_app: Option<String>,
    /// Game id while a game runs, also when the launcher is in front of it.
    running_game: Option<String>,
    pad: Option<String>,
}

/// Apps from apps.toml, with each tile image resolved to an absolute path (see `config::tile_image`).
#[tauri::command]
pub fn list_apps(state: State<'_, AppState>) -> Vec<AppEntry> {
    let cfg = state.config.read().unwrap();
    // The house's library, once the server has one, is a tile like the services'.
    let library = cfg.settings.library_on.then(crate::config::library_app);
    cfg.apps
        .iter()
        .cloned()
        .chain(library)
        .map(|mut a| {
            a.icon = crate::config::tile_image(&state.data_dir, &a)
                .map(|p| p.to_string_lossy().into_owned());
            a
        })
        .collect()
}

#[tauri::command]
pub async fn launch_app(
    app: AppHandle,
    id: String,
    title: Option<String>,
    backdrop: Option<String>,
) -> Result<()> {
    launcher::launch(&app, &id, title, backdrop)
}

/// Opens a web app at `url` (a trailer in the YouTube app, say).
#[tauri::command]
pub async fn launch_app_at(app: AppHandle, id: String, url: String) -> Result<()> {
    launcher::launch_web(&app, &id, &url, None)
}

#[tauri::command]
pub async fn close_app(app: AppHandle) {
    launcher::close(&app);
}

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> Status {
    Status {
        mode: state.mode(),
        running_app: state.running.lock().unwrap().as_ref().map(|r| r.app_id.clone()),
        running_game: state.games.running().or_else(|| state.retro.session.lock().unwrap().as_ref().map(|s| s.game.clone())),
        pad: state.pad.lock().unwrap().clone(),
    }
}

#[tauri::command]
pub async fn list_games(app: AppHandle) -> games::Library {
    games::library(&app)
}

#[tauri::command]
pub async fn launch_game(app: AppHandle, id: String) -> Result<()> {
    games::launch(&app, &id)
}

#[tauri::command]
pub fn toggle_favorite(app: AppHandle, id: String) -> Result<bool> {
    games::toggle_favorite(&app, &id)
}

/// The retro shelf: consoles, emulators, games, BIOS (retro/mod.rs).
#[tauri::command]
pub async fn retro_get(app: AppHandle) -> retro::Retro {
    retro::snapshot(&app)
}

/// Plays a retro game; `fresh` starts it over instead of from its auto-save.
#[tauri::command]
pub async fn retro_launch(app: AppHandle, id: String, fresh: Option<bool>) -> Result<()> {
    retro::launch(&app, &id, fresh.unwrap_or(false))
}

#[tauri::command]
pub fn retro_install(app: AppHandle, emu: retro::emu::Emu) -> Result<()> {
    retro::install(&app, emu)
}

#[tauri::command]
pub fn retro_cancel(app: AppHandle, emu: retro::emu::Emu) {
    retro::cancel(&app, emu)
}

/// Unlinks the user's copy, or deletes the launcher's program files (keeping saves).
#[tauri::command]
pub async fn retro_remove(app: AppHandle, emu: retro::emu::Emu) -> Result<()> {
    retro::remove(&app, emu)
}

#[tauri::command]
pub async fn retro_link(
    app: AppHandle,
    emu: retro::emu::Emu,
    path: String,
    move_settings: bool,
    folders: Vec<(String, String)>,
) -> Result<()> {
    retro::link(&app, emu, &path, move_settings, folders)
}

/// Sets consoles' game folders; a null path clears one.
#[tauri::command]
pub async fn retro_set_folders(app: AppHandle, folders: Vec<(String, Option<String>)>) -> Result<()> {
    retro::set_folders(&app, folders)
}

#[tauri::command]
pub async fn retro_set_bios(app: AppHandle, path: String) -> Result<()> {
    retro::set_bios(&app, &path)
}

#[tauri::command]
pub async fn retro_scan_bios(app: AppHandle) {
    retro::scan_bios(&app)
}

#[tauri::command]
pub fn retro_add_cores(app: AppHandle) -> Result<()> {
    retro::add_cores(&app)
}

#[tauri::command]
pub fn retro_check_updates(app: AppHandle) {
    retro::check_updates(&app)
}

#[tauri::command]
pub fn retro_open_bios(app: AppHandle, emu: retro::emu::Emu) -> Result<()> {
    retro::open_bios(&app, emu)
}

/// A folder's folders for the folder picker; the drives with no path.
#[tauri::command]
pub async fn retro_browse(app: AppHandle, path: Option<String>) -> retro::Listing {
    retro::browse(&app, path.as_deref())
}

#[tauri::command]
pub fn retro_browse_start(app: AppHandle, target: retro::Target) -> Option<String> {
    retro::browse_start(&app, &target)
}

#[tauri::command]
pub async fn retro_check(target: retro::Target, path: String) -> retro::Check {
    retro::check(&target, &path)
}

#[tauri::command]
pub fn get_history(state: State<'_, AppState>) -> History {
    state.history.lock().unwrap().clone()
}

/// Takes something opened from Search out of Continue watching.
#[tauri::command]
pub fn forget_opened(state: State<'_, AppState>, app_id: String, title: Option<String>) -> Result<()> {
    let mut h = state.history.lock().unwrap();
    h.forget_opened(&app_id, title.as_deref());
    h.save(&state.data_dir)
}

#[tauri::command]
pub fn record_search(state: State<'_, AppState>, query: String) -> Result<()> {
    let mut h = state.history.lock().unwrap();
    h.record_search(&query);
    h.save(&state.data_dir)
}

/// A page of search results, with which of the user's apps stream each one. `mine`: only those.
#[tauri::command]
pub async fn tmdb_search(
    state: State<'_, AppState>,
    query: String,
    page: Option<u32>,
    mine: Option<bool>,
) -> Result<tmdb::SearchPage> {
    let (settings, apps) = {
        let cfg = state.config.read().unwrap();
        (cfg.settings.clone(), cfg.apps.clone())
    };
    tmdb::search_page(&state.http, &settings, &apps, &query, page.unwrap_or(1), mine.unwrap_or(false)).await
}

#[tauri::command]
pub async fn tmdb_providers(
    state: State<'_, AppState>,
    media_type: String,
    id: u64,
) -> Result<Vec<String>> {
    let (settings, apps) = {
        let cfg = state.config.read().unwrap();
        (cfg.settings.clone(), cfg.apps.clone())
    };
    tmdb::providers(&state.http, &settings, &apps, &media_type, id).await
}

/// TMDB watch-provider ids of the app `service`.
fn service_providers(state: &AppState, service: &str) -> Result<(Settings, Vec<u32>)> {
    let cfg = state.config.read().unwrap();
    let app = cfg
        .apps
        .iter()
        .find(|a| a.id == service)
        .ok_or_else(|| Error::msg(format!("unknown app '{service}'")))?;
    Ok((cfg.settings.clone(), app.tmdb_providers.clone()))
}

#[tauri::command]
pub async fn tmdb_discover(
    state: State<'_, AppState>,
    service: String,
    query: tmdb::Discover,
) -> Result<Vec<tmdb::Title>> {
    let (settings, ids) = service_providers(&state, &service)?;
    tmdb::discover(&state.http, &settings, &ids, &query).await
}

/// A page of what streams on `service` matching `query`; its most popular when `query` is empty.
#[tauri::command]
pub async fn tmdb_search_on(
    state: State<'_, AppState>,
    service: String,
    query: String,
    page: Option<u32>,
) -> Result<tmdb::SearchPage> {
    // The library can get anything: its search is the whole catalog.
    if service == watch::library::SERVICE {
        if query.trim().is_empty() {
            return Ok(tmdb::SearchPage { results: Vec::new(), next: None });
        }
        let settings = state.config.read().unwrap().settings.clone();
        return tmdb::search_page(&state.http, &settings, &[], &query, page.unwrap_or(1), false).await;
    }
    let (settings, app) = {
        let cfg = state.config.read().unwrap();
        let app = cfg
            .apps
            .iter()
            .find(|a| a.id == service)
            .ok_or_else(|| Error::msg(format!("unknown app '{service}'")))?;
        (cfg.settings.clone(), app.clone())
    };
    tmdb::search_page(&state.http, &settings, &[app], &query, page.unwrap_or(1), true).await
}

#[tauri::command]
pub async fn tmdb_details(state: State<'_, AppState>, media_type: String, id: u64) -> Result<tmdb::Details> {
    let settings = state.config.read().unwrap().settings.clone();
    tmdb::details(&state.http, &settings, &media_type, id).await
}

/// An actor's or director's page.
#[tauri::command]
pub async fn tmdb_person(state: State<'_, AppState>, id: u64) -> Result<tmdb::Person> {
    let settings = state.config.read().unwrap().settings.clone();
    tmdb::person(&state.http, &settings, id).await
}

/// Which of the user's apps stream each of a person's titles, keyed "movie:603".
#[tauri::command]
pub async fn tmdb_person_services(
    state: State<'_, AppState>,
    id: u64,
) -> Result<std::collections::HashMap<String, Vec<String>>> {
    let (settings, apps) = {
        let cfg = state.config.read().unwrap();
        (cfg.settings.clone(), cfg.apps.clone())
    };
    tmdb::person_services(&state.http, &settings, &apps, id).await
}

#[tauri::command]
pub async fn tmdb_season(state: State<'_, AppState>, id: u64, number: u32) -> Result<Vec<tmdb::Episode>> {
    let settings = state.config.read().unwrap().settings.clone();
    tmdb::season(&state.http, &settings, id, number).await
}

/// Plays a movie, or a series' episode (none given: wherever the user is), on `service`.
#[tauri::command]
pub async fn watch_play(
    app: AppHandle,
    service: String,
    media_type: String,
    id: u64,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<()> {
    watch::play(&app, &service, &media_type, id, season, episode).await
}

/// Reads what the user watched on these services outside the launcher, in the background.
#[tauri::command]
pub fn watch_sync(app: AppHandle, services: Vec<String>) {
    watch::sync::request(&app, services);
}

/// What the screen asks of the launcher's own player.
#[tauri::command]
pub fn player_control(app: AppHandle, control: crate::mpv::Control) {
    crate::mpv::control(&app, control);
}

/// How the launcher's own player is doing, for a screen that just opened.
#[tauri::command]
pub fn player_state(app: AppHandle) -> crate::mpv::PlayerState {
    crate::mpv::snapshot(&app)
}

/// Opens the service's "Who's watching?" for the user to pick the profile Play uses.
#[tauri::command]
pub async fn watch_pick_profile(app: AppHandle, service: String) -> Result<()> {
    blocking(move || watch::pick_profile(&app, &service)).await
}

/// The service profile Play opens, for the header.
#[tauri::command]
pub fn watch_profile(state: State<'_, AppState>, service: String) -> watch::ProfileState {
    watch::profile_state(&state, &service)
}

/// Continue watching, most recent first: on `service`, or everywhere.
#[tauri::command]
pub fn watch_progress(state: State<'_, AppState>, service: Option<String>) -> Vec<watch::Progress> {
    watch::continue_watching(&state, service.as_deref())
}

/// Takes a title out of Continue watching, until it is played again.
#[tauri::command]
pub fn watch_dismiss(state: State<'_, AppState>, service: String, media_type: String, id: u64) -> Result<()> {
    watch::dismiss(&state, &service, &media_type, id)
}

#[tauri::command]
pub fn watch_title(state: State<'_, AppState>, service: String, media_type: String, id: u64) -> watch::TitleState {
    watch::title_state(&state, &service, &media_type, id)
}

#[tauri::command]
pub fn watchlist(state: State<'_, AppState>, service: Option<String>) -> Vec<watch::ListItem> {
    watch::watchlist(&state, service.as_deref())
}

/// Adds the title to My list, or takes it out; true when it is now listed.
#[tauri::command]
pub fn watchlist_toggle(state: State<'_, AppState>, item: watch::ListItem) -> Result<bool> {
    watch::toggle_listed(&state, item)
}

/// Recommendation rows for a service's home, or the launcher's Home (no service).
#[tauri::command]
pub async fn recs_shelves(app: AppHandle, service: Option<String>) -> Result<Vec<recs::profile::Shelf>> {
    recs::shelves(&app, service.as_deref()).await
}

/// The titles of one recommendation row.
#[tauri::command]
pub async fn recs_row(app: AppHandle, service: Option<String>, shelf: recs::profile::Shelf) -> Result<Vec<recs::RecTitle>> {
    recs::row(&app, service.as_deref(), &shelf).await
}

/// One row of a genre's page: "for_you", "popular", "new" or "acclaimed".
#[tauri::command]
pub async fn recs_genre(app: AppHandle, genre: u32, sort: String) -> Result<Vec<recs::RecTitle>> {
    recs::genre(&app, genre, &sort).await
}

#[tauri::command]
pub fn rating_get(state: State<'_, AppState>, media_type: String, id: u64) -> Option<recs::Score> {
    recs::rating(&state, &media_type, id)
}

/// Dislike, like or love a title; null takes the rating away. Returns the rating now.
#[tauri::command]
pub fn rating_set(state: State<'_, AppState>, item: recs::Rating, score: Option<recs::Score>) -> Result<Option<recs::Score>> {
    recs::rate(&state, item, score)
}

/// This TV PC's settings, with the household's from the server (its keys masked).
#[tauri::command]
pub async fn get_settings(app: AppHandle) -> Settings {
    crate::agent::settings::get(&app).await
}

/// Saves this TV PC's own settings here and the household's on the server.
#[tauri::command]
pub async fn save_settings(app: AppHandle, settings: Settings) -> Result<()> {
    crate::agent::settings::save(&app, settings).await
}

/// The link to the home server, for Settings › Server and the "Server offline" banner.
#[derive(Serialize)]
pub struct LinkInfo {
    status: Option<tvl_agent_link::Status>,
    /// The address typed in Settings; None: found on the network.
    address: Option<String>,
    paired: bool,
}

#[tauri::command]
pub fn link_status(app: AppHandle) -> LinkInfo {
    let link = crate::agent::link(&app);
    LinkInfo {
        status: crate::agent::status(&app),
        address: link.as_ref().and_then(|l| l.address()),
        paired: link.is_some_and(|l| l.is_paired()),
    }
}

/// The server's address, typed in Settings; none (or blank): look for it on the network.
#[tauri::command]
pub fn link_address(app: AppHandle, address: Option<String>) -> Result<()> {
    let link = crate::agent::link(&app).ok_or_else(|| Error::msg("Not started yet"))?;
    Ok(link.set_address(address.as_deref())?)
}

/// Forgets this TV PC's pairing and shows a new code.
#[tauri::command]
pub fn link_pair_again(app: AppHandle) -> Result<()> {
    let link = crate::agent::link(&app).ok_or_else(|| Error::msg("Not started yet"))?;
    Ok(link.pair_again()?)
}

/// After this TV PC's settings changed (from Settings, or the household's from the server): what
/// uses a changed key starts again.
pub fn settings_changed(app: &AppHandle, old: &Settings, new: &Settings) {
    if (&old.steam_api_key, &old.steam_id, &old.steamgriddb_api_key) != (&new.steam_api_key, &new.steam_id, &new.steamgriddb_api_key) {
        games::invalidate(app);
    }
    if old.spotify_client_id != new.spotify_client_id {
        spotify::start(app);
    }
    if old.twitch_client_id != new.twitch_client_id {
        twitch::start(app);
    }
}

#[tauri::command]
pub fn spotify_get(app: AppHandle) -> spotify::Snapshot {
    spotify::snapshot(&app)
}

/// Shows Spotify's consent page; resolves once it redirected back to the launcher.
#[tauri::command]
pub async fn spotify_sign_in(app: AppHandle) -> Result<()> {
    spotify::sign_in(&app).await
}

#[tauri::command]
pub fn spotify_sign_out(app: AppHandle) -> Result<()> {
    spotify::sign_out(&app)
}

#[tauri::command]
pub async fn spotify_control(app: AppHandle, control: spotify::Control) -> Result<()> {
    spotify::control(&app, control).await
}

/// Plays a playlist, album, artist or show, or a track inside `context`, on this PC.
#[tauri::command]
pub async fn spotify_play(app: AppHandle, uri: String, context: Option<String>, position_ms: Option<u64>) -> Result<()> {
    spotify::play(&app, &uri, context.as_deref(), position_ms).await
}

/// One library shelf: recent, playlists, albums, artists or shows.
#[tauri::command]
pub async fn spotify_shelf(app: AppHandle, kind: String) -> Result<Vec<spotify::Item>> {
    spotify::shelf(&app, &kind).await
}

/// A playlist or Liked Songs with its songs, for the playlist screen.
#[tauri::command]
pub async fn spotify_playlist(app: AppHandle, uri: String) -> Result<spotify::Playlist> {
    spotify::playlist(&app, &uri).await
}

/// An artist with their releases, for the artist screen.
#[tauri::command]
pub async fn spotify_artist(app: AppHandle, uri: String) -> Result<spotify::Artist> {
    spotify::artist(&app, &uri).await
}

/// Everyone's most played songs of an artist, from Last.fm; null without a Last.fm key.
#[tauri::command]
pub async fn spotify_artist_popular(app: AppHandle, uri: String, name: String) -> Result<Option<Vec<spotify::TopTrack>>> {
    spotify::popular(&app, &uri, &name).await
}

/// Plays songs one after another on this PC.
#[tauri::command]
pub async fn spotify_play_tracks(app: AppHandle, uris: Vec<String>) -> Result<()> {
    spotify::play_tracks(&app, &uris).await
}

/// A podcast with its newest episodes, for the podcast screen.
#[tauri::command]
pub async fn spotify_show(app: AppHandle, uri: String) -> Result<spotify::Show> {
    spotify::show(&app, &uri).await
}

/// The oldest episodes of a podcast with `total` of them, oldest first.
#[tauri::command]
pub async fn spotify_oldest_episodes(app: AppHandle, uri: String, total: u64) -> Result<Vec<spotify::Episode>> {
    spotify::oldest_episodes(&app, &uri, total).await
}

/// Follows or unfollows an artist or podcast.
#[tauri::command]
pub async fn spotify_follow(app: AppHandle, uri: String, on: bool) -> Result<()> {
    spotify::follow(&app, &uri, on).await
}

#[tauri::command]
pub async fn spotify_search(app: AppHandle, query: String) -> Result<spotify::SearchResults> {
    spotify::search(&app, &query).await
}

#[tauri::command]
pub fn photos_get(app: AppHandle) -> photos::Snapshot {
    photos::snapshot(&app)
}

/// Every photo of the albums switched on, for the screensaver.
#[tauri::command]
pub async fn photos_slideshow(app: AppHandle) -> Result<Vec<photos::Photo>> {
    photos::slideshow(&app).await
}

#[tauri::command]
pub async fn photos_google_sign_in(app: AppHandle) -> Result<()> {
    photos::sign_in(&app).await
}

#[tauri::command]
pub fn photos_google_sign_out(app: AppHandle) -> Result<()> {
    photos::sign_out(&app)
}

/// Shows a QR code to choose photos on the phone; resolves with the album once they're saved.
/// `album`: pick again for that album instead of making a new one.
#[tauri::command]
pub async fn photos_google_pick(app: AppHandle, album: Option<String>) -> Result<photos::Album> {
    photos::pick(&app, album).await
}

#[tauri::command]
pub async fn photos_pick_here(app: AppHandle) -> Result<()> {
    blocking(move || photos::pick_here(&app)).await
}

#[tauri::command]
pub fn photos_cancel_pick(app: AppHandle) {
    photos::cancel_pick(&app)
}

#[tauri::command]
pub fn photos_add_folder(app: AppHandle, path: String) -> Result<photos::Album> {
    photos::add_folder(&app, &path)
}

#[tauri::command]
pub fn photos_set_enabled(app: AppHandle, id: String, enabled: bool) -> Result<()> {
    photos::set_enabled(&app, &id, enabled)
}

#[tauri::command]
pub fn photos_remove(app: AppHandle, id: String) -> Result<()> {
    photos::remove(&app, &id)
}

#[tauri::command]
pub async fn sports_home(app: AppHandle) -> Result<sports::Home> {
    sports::home(&app).await
}

/// One game with everything its screen shows; `game` is the card it was opened from.
#[tauri::command]
pub async fn sports_game(app: AppHandle, game: sports::Game) -> Result<sports::GameDetail> {
    sports::game(&app, game).await
}

#[tauri::command]
pub async fn sports_competition(app: AppHandle, id: String) -> Result<sports::Competition> {
    sports::competition(&app, &id).await
}

/// Opens a channel's app or page; with `game`, its score can show over the broadcast.
#[tauri::command]
pub async fn sports_watch(app: AppHandle, channel: sports::Channel, game: Option<sports::Game>) -> Result<()> {
    sports::watch(&app, channel, game).await
}

/// The TV guide: every sports channel's programme, with the games on it.
#[tauri::command]
pub async fn sports_tv(app: AppHandle) -> Result<Vec<sports::TvChannel>> {
    sports::tv(&app).await
}

/// Plays a live sports channel with the launcher's overlay (channel rail, score).
#[tauri::command]
pub async fn sports_watch_tv(app: AppHandle, key: String) -> Result<()> {
    sports::watch_tv(&app, &key, sports::Lineup::Sports).await
}

#[tauri::command]
pub async fn sports_highlights(app: AppHandle, game: sports::Game) -> Result<sports::Highlights> {
    sports::highlights(&app, &game).await
}

/// Resolves to whether the reminder is on now.
#[tauri::command]
pub fn sports_remind(app: AppHandle, game: sports::Game, on: bool) -> Result<bool> {
    sports::remind(&app, &game, on)
}

#[tauri::command]
pub fn sports_set_overlay(app: AppHandle, on: bool) -> Result<()> {
    sports::set_overlay(&app, on)
}

/// The Home's Live now row: live sports, followed Twitch channels, subscribed YouTube channels.
#[tauri::command]
pub async fn live_now(app: AppHandle) -> live::LiveNow {
    live::now(&app).await
}

/// The Live tab: every live channel with its programme, the streams followed, favorites.
#[tauri::command]
pub async fn live_tv(app: AppHandle) -> livetv::LiveTv {
    livetv::tab(&app).await
}

/// Plays a live channel with the overlay, zapping through the whole lineup.
#[tauri::command]
pub async fn live_watch(app: AppHandle, key: String) -> Result<()> {
    livetv::watch(&app, &key).await
}

/// Pins or unpins a channel; resolves to the favorites.
#[tauri::command]
pub fn live_favorite(app: AppHandle, key: String, on: bool) -> Result<Vec<String>> {
    livetv::favorite(&app, &key, on)
}

/// A reminder 5 minutes before what starts on `key` at `start` (unix ms); resolves to whether
/// it's on.
#[tauri::command]
pub fn live_remind(app: AppHandle, key: String, start: i64, on: bool) -> Result<bool> {
    livetv::remind(&app, &key, start, on)
}

#[tauri::command]
pub fn live_news(app: AppHandle) -> Vec<livetv::NewsSource> {
    livetv::news_list(&app)
}

/// YouTube channels named like `query`, to add to the news channels.
#[tauri::command]
pub async fn live_news_search(app: AppHandle, query: String) -> Result<Vec<livetv::Found>> {
    livetv::search_news(&app, &query).await
}

#[tauri::command]
pub fn live_news_add(app: AppHandle, id: String, name: String) -> Result<Vec<livetv::NewsSource>> {
    livetv::add_news(&app, &id, &name)
}

#[tauri::command]
pub fn live_news_remove(app: AppHandle, id: String) -> Result<Vec<livetv::NewsSource>> {
    livetv::remove_news(&app, &id)
}

#[tauri::command]
pub fn youtube_get(app: AppHandle) -> tvl_protocol::ServiceLink {
    youtube::snapshot(&app)
}

/// Google's consent page for read-only YouTube; resolves once it redirected back.
#[tauri::command]
pub async fn youtube_sign_in(app: AppHandle) -> Result<()> {
    youtube::sign_in(&app).await
}

#[tauri::command]
pub fn youtube_sign_out(app: AppHandle) -> Result<()> {
    youtube::sign_out(&app)
}

/// Plays a video (a live stream) in the YouTube TV app. Off the UI thread: closing the app in
/// front can take seconds.
#[tauri::command]
pub async fn youtube_watch(app: AppHandle, id: String) -> Result<()> {
    blocking(move || youtube::watch(&app, &id)).await
}

/// Runs `f`, which waits on processes (closing the app in front), on a thread of its own: a sync
/// command runs on the UI thread, and the TV froze for up to 5 s.
pub async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| Error::msg(e.to_string()))?
}

#[tauri::command]
pub fn twitch_get(app: AppHandle) -> twitch::Snapshot {
    twitch::snapshot(&app)
}

/// Shows Twitch's activation page; resolves once Authorize was pressed on it.
#[tauri::command]
pub async fn twitch_sign_in(app: AppHandle) -> Result<()> {
    twitch::sign_in(&app).await
}

#[tauri::command]
pub async fn twitch_sign_out(app: AppHandle) -> Result<()> {
    twitch::sign_out(&app).await
}

/// Live streams: `source` is "following", "top", or "game" with `game_id`.
#[tauri::command]
pub async fn twitch_streams(app: AppHandle, source: String, game_id: Option<String>) -> Result<Vec<twitch::Stream>> {
    twitch::streams(&app, &source, game_id.as_deref()).await
}

#[tauri::command]
pub async fn twitch_categories(app: AppHandle) -> Result<Vec<twitch::Category>> {
    twitch::categories(&app).await
}

#[tauri::command]
pub async fn twitch_search(app: AppHandle, query: String, live_only: bool) -> Result<twitch::SearchResults> {
    twitch::search(&app, &query, live_only).await
}

#[tauri::command]
pub async fn twitch_channel(app: AppHandle, login: String) -> Result<twitch::ChannelInfo> {
    twitch::channel(&app, &login).await
}

#[tauri::command]
pub async fn twitch_send_chat(app: AppHandle, broadcaster_id: String, message: String) -> Result<()> {
    twitch::send_chat(&app, &broadcaster_id, &message).await
}

/// A real Space press. The Twitch embed ignores play() on a paused live stream, but its own
/// Space shortcut resumes it, so the page focuses the player's frame and asks for this.
#[tauri::command]
pub fn press_space() -> Result<()> {
    use enigo::{Direction, Enigo, Key, Keyboard};
    let mut enigo = Enigo::new(&enigo::Settings::default()).map_err(|e| Error::msg(e.to_string()))?;
    enigo.key(Key::Space, Direction::Click).map_err(|e| Error::msg(e.to_string()))
}

/// Re-reads apps.toml and settings.toml after they were edited by hand.
#[tauri::command]
pub fn reload_config(app: AppHandle) -> Result<()> {
    let state = app.state::<AppState>();
    *state.config.write().unwrap() = Config::load_or_init(&state.data_dir)?;
    spotify::start(&app);
    twitch::start(&app);
    Ok(())
}

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> Result<bool> {
    app.autolaunch().is_enabled().map_err(|e| Error::msg(e.to_string()))
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<()> {
    let al = app.autolaunch();
    if enabled { al.enable() } else { al.disable() }.map_err(|e| Error::msg(e.to_string()))
}

/// Opens the folder holding apps.toml, settings.toml and history.json.
#[tauri::command]
pub fn open_data_dir(app: AppHandle) -> Result<()> {
    let dir = app.state::<AppState>().data_dir.to_string_lossy().into_owned();
    app.opener()
        .open_path(dir, None::<&str>)
        .map_err(|e| Error::msg(e.to_string()))
}

/// Records from the microphone until the speaker goes quiet and returns what was said;
/// null when cancelled or nothing was heard.
#[tauri::command]
pub async fn assistant_listen(app: AppHandle) -> Result<Option<String>> {
    assistant::voice::listen(&app).await
}

/// Changes only how replies are read out, saved at once (the assistant's voice toggle).
#[tauri::command]
pub fn assistant_set_speech(state: State<'_, AppState>, speech: Speech) -> Result<()> {
    let mut cfg = state.config.write().unwrap();
    cfg.settings.assistant_speech = speech;
    Config::save_settings(&state.data_dir, &cfg.settings)
}

/// Ends the recording now and transcribes it.
#[tauri::command]
pub fn assistant_listen_stop(state: State<'_, AppState>) {
    state.assistant.recorder.finish();
}

/// Ends the recording and drops it.
#[tauri::command]
pub fn assistant_listen_cancel(state: State<'_, AppState>) {
    state.assistant.recorder.cancel();
}

/// Reads `text` out on the speakers with the voice on this PC; resolves once it has been said
/// or hushed. "voice-start" (with `id`) goes out when the sound starts.
#[tauri::command]
pub async fn assistant_speak(app: AppHandle, text: String, id: u32) -> Result<()> {
    assistant::speech::say(&app, &text, None, id).await
}

/// Stops the reply being read out.
#[tauri::command]
pub fn assistant_hush(state: State<'_, AppState>) {
    assistant::speech::hush(&state);
}

/// Reads a line out in `voice` (empty: the one in Settings), to hear what it sounds like.
#[tauri::command]
pub async fn assistant_voice_try(app: AppHandle, voice: String) -> Result<()> {
    let voice = Some(voice.trim()).filter(|v| !v.is_empty());
    let line = assistant::speech::sample(&app.state::<AppState>().config.read().unwrap().settings, voice);
    assistant::speech::say(&app, line, voice, 0).await
}

/// Whether the voice model is on this PC, or how its download is going.
#[tauri::command]
pub fn assistant_voice_model(state: State<'_, AppState>) -> assistant::speech::VoiceModel {
    assistant::speech::status(&state)
}

/// Gets the voice ready: loads the model, or downloads it first ("voice-model" events).
#[tauri::command]
pub fn assistant_voice_prepare(app: AppHandle) {
    tauri::async_runtime::spawn(assistant::speech::prepare(app));
}
