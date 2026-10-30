import { convertFileSrc, invoke as tauriInvoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { sampleAgenda, sampleNextEvent, sampleQr, sampleRemind, sampleShowCalendar, sampleTravel } from './calendarSample';
import { sampleClock } from './clockSample';
import { SAMPLE_NEWS, sampleLineup, sampleNewsSearch } from './liveSample';
import { retroSample } from './retroSample';
import { sampleCompetition, sampleGameDetail, sampleHighlights, sampleLiveSports, sampleSportsHome, sampleTv } from './sportsSample';

/** False when the UI runs in a plain browser (`npm run dev`), where calls return sample data. */
export const isTauri = '__TAURI_INTERNALS__' in window;

/** Another way to the launcher's backend: the companion (phones and tablets on the home network)
 *  sends every call over HTTP and hears the events over a WebSocket (companion/src/transport.ts). */
export interface Transport {
  invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, cb: (payload: T) => void): () => void;
  fileSrc(path: string): string;
}

let transport: Transport | null = null;

/** Set once, before the first call. */
export function setTransport(t: Transport) {
  transport = t;
}

/** Something answers calls: the app itself, or the launcher through the companion's transport. */
const backend = () => isTauri || transport !== null;

const invoke = <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> =>
  transport ? transport.invoke<T>(cmd, args) : tauriInvoke<T>(cmd, args);

export type Action =
  | 'Up'
  | 'Down'
  | 'Left'
  | 'Right'
  | 'Select'
  | 'Back'
  | 'Home'
  | 'TabPrev'
  | 'TabNext'
  /** LT / RT: section tabs, unless the screen uses them. */
  | 'TriggerPrev'
  | 'TriggerNext'
  /** Y: more controls for the focused item; talk, in the assistant. */
  | 'Options'
  /** X: opens the assistant. */
  | 'Assistant';
export type Mode = 'launcher' | 'streaming' | 'game' | 'player';

export interface AppEntry {
  id: string;
  name: string;
  kind: 'web' | 'uri' | 'exec';
  url: string | null;
  search_url: string | null;
  icon: string | null;
  color: string | null;
  /** TMDB watch-provider ids. */
  tmdb_providers?: number[];
  /** False: the tile opens the website instead of the launcher's copy of the service. */
  emulate?: boolean;
  /** Profile picked on the service's "Who's watching?" (Play learns its id and skips the picker after). */
  profile?: string;
}

export interface Opened {
  app_id: string;
  title: string | null;
  backdrop: string | null;
  at: number;
}

export interface History {
  opened: Opened[];
  searches: string[];
}

export interface Title {
  id: number;
  media_type: 'movie' | 'tv';
  title: string;
  year: string | null;
  overview: string;
  poster: string | null;
  backdrop: string | null;
}

export interface SeasonInfo {
  number: number;
  name: string;
  episode_count: number;
  year: string | null;
}

export interface CastMember {
  /** TMDB person id. */
  id: number;
  name: string;
  character: string;
  photo: string | null;
  /** The movie's director or the series' creator, listed ahead of the cast; null for the cast. */
  job: 'director' | 'creator' | null;
}

/** A title on a person's page and what they did in it. */
export interface Credit extends Title {
  /** The character, or the jobs ("Director, Writer"). */
  role: string;
  /** Behind the camera. */
  crew: boolean;
}

/** An actor's or director's page. */
export interface Person {
  id: number;
  name: string;
  /** TMDB's department, in English: "Acting", "Directing"... */
  department: string | null;
  /** "1995-12-27" */
  birthday: string | null;
  deathday: string | null;
  place: string | null;
  bio: string;
  photo: string | null;
  /** Best known first. */
  known_for: Credit[];
  /** Newest first; announced titles (no date yet) before those. */
  credits: Credit[];
}

/** A movie or series with everything the details screen shows. */
export interface TitleDetails extends Title {
  tagline: string | null;
  genres: string[];
  /** Minutes: the movie, or a typical episode. */
  runtime: number | null;
  /** Age rating in the settings' region. */
  certification: string | null;
  /** The title's logo artwork. */
  logo: string | null;
  seasons: SeasonInfo[];
  cast: CastMember[];
  /** YouTube video id. */
  trailer: string | null;
}

export interface TvEpisode {
  season: number;
  number: number;
  name: string;
  overview: string;
  still: string | null;
  /** Minutes. */
  runtime: number | null;
  air_date: string | null;
}

/** A row of a streaming app's home: what TMDB's discover is asked for. */
export interface DiscoverQuery {
  media_type: Title['media_type'];
  genre?: number | null;
  sort?: string | null;
  page?: number | null;
  /** Only titles first released in the last this many years. */
  since_years?: number | null;
  /** Only titles with at least this many votes. */
  min_votes?: number | null;
}

/** How far into a movie or series the user got on a service, as the launcher saw it play. */
export interface WatchProgress {
  service: string;
  media_type: Title['media_type'];
  id: number;
  title: string;
  poster: string | null;
  backdrop: string | null;
  /** Episode counts by season (index 0: season 1). */
  seasons: number[];
  /** The episode last watched, or the next one once it's done; 0 for a movie. Earlier ones count as watched. */
  season: number;
  episode: number;
  /** Seconds. */
  position: number;
  duration: number;
  /** "s1e3" → watched share, 0..1. */
  episodes: Record<string, number>;
  finished: boolean;
  updated_at: number;
  /** The episode is known for sure. False when only the service's own Continue watching row
   *  was read: say "Resume" and let the service pick the episode. */
  confirmed: boolean;
  /** Watched share when only a percentage is known (`duration` 0). */
  share: number | null;
}

export interface WatchListItem {
  service: string;
  media_type: Title['media_type'];
  id: number;
  title: string;
  poster: string | null;
  backdrop: string | null;
  added_at: number;
}

/** What the user thought of a title; ratings belong to the title, not to a service. */
export type Score = 'dislike' | 'like' | 'love';

export type ShelfKind =
  | 'for_you'
  | 'trending'
  | 'trending_day'
  | 'because'
  | 'liked'
  | 'loved'
  | 'genre'
  | 'person'
  | 'fresh'
  | 'gems'
  | 'acclaimed'
  | 'again';

/** A recommendation row: named by the UI from `kind` and `label`, filled by `recsRow`. */
export interface RecShelf {
  key: string;
  kind: ShelfKind;
  /** The seed title (because, liked, loved). */
  media_type: Title['media_type'] | null;
  /** The seed title's id, the genre (genre) or the person (person). */
  id: number | null;
  /** The seed's title or the person's name. */
  label: string | null;
}

/** A recommended title and the user's apps that stream it (empty when not known). */
/** The rows of a genre's page. */
export type GenreSort = 'for_you' | 'popular' | 'new' | 'acclaimed';

export interface RecTitle extends Title {
  services: string[];
}

/** Part of a paged search: titles with the user's apps that stream them, and the page to ask for next. */
export interface SearchPage {
  results: RecTitle[];
  /** Null once there's nothing more. */
  next: number | null;
}

export interface TitleWatch {
  progress: WatchProgress | null;
  listed: boolean;
}

/** Playing from a service's screens: "watch" events from the backend. */
export interface WatchEvent {
  /** opening: curtain up; preparing: and the player is being fetched; playing: the browser or the
   * launcher's player shows; reveal: shown before the video started; closed: back. */
  phase: 'opening' | 'preparing' | 'playing' | 'reveal' | 'closed';
  service: string;
  media_type: Title['media_type'];
  id: number;
  title: string;
  backdrop: string | null;
  season: number;
  episode: number;
}

/** How a title stands in the house's library. */
export type LibraryState = 'absent' | 'wanted' | 'downloading' | 'ready';

export interface LibraryTitle {
  /** Radarr or Sonarr is set up for this kind of title. */
  configured: boolean;
  state: LibraryState;
  /** Something can be played, even with more on its way. */
  ready: boolean;
  progress: number;
  speed: number;
  eta_s: number | null;
  note: string | null;
  /** A series' episodes, by "s1e3". */
  episodes: Record<string, { state: LibraryState; progress: number }>;
}

export interface LibraryListing {
  configured: boolean;
  titles: (Title & { added: string })[];
  free_bytes: number | null;
  problems: string[];
}

export interface Download {
  title: string;
  media_type: Title['media_type'];
  id: number;
  season: number;
  episode: number;
  progress: number;
  speed: number;
  eta_s: number | null;
  paused: boolean;
  hash: string | null;
  note: string | null;
}

export interface Downloads {
  items: Download[];
  speed: number;
  problems: string[];
}

export interface PlayerTrack {
  id: number;
  name: string;
  on: boolean;
}

/** What the launcher's own player reports. */
export interface PlayerState {
  title: string;
  subtitle: string | null;
  position: number;
  duration: number;
  paused: boolean;
  buffering: boolean;
  /** Seconds fetched ahead of the position. */
  cached: number;
  volume: number;
  ended: boolean;
  audio: PlayerTrack[];
  subtitles: PlayerTrack[];
  /** The next episode the library has. */
  next: string | null;
}

export type PlayerControl =
  | { kind: 'toggle' | 'next' | 'close' }
  | { kind: 'seek'; by: number }
  | { kind: 'seek_to'; at: number }
  | { kind: 'volume'; by: number }
  | { kind: 'audio'; id: number }
  | { kind: 'subtitle'; id: number | null };

/** The service profile Play opens, for a service's header. */
export interface WatchProfile {
  /** Null until one is learned (picked on the service's "Who's watching?") or set in apps.toml. */
  name: string | null;
  /** The service has a "Who's watching?" to pick on. */
  pickable: boolean;
}

export type HomeButton = 'guide' | 'select' | 'start' | 'l3' | 'r3';

export interface Settings {
  tmdb_api_key: string;
  region: string;
  language: string;
  browser: 'edge' | 'chrome';
  browser_path?: string | null;
  home_button: HomeButton;
  steam_api_key: string;
  steam_id: string;
  steamgriddb_api_key: string;
  youtube_api_key: string;
  ewelink_app_id: string;
  ewelink_app_secret: string;
  tuya_access_id: string;
  tuya_access_secret: string;
  /** Tuya data center: us, us-e, eu, eu-w, in, cn, sg. Empty: TUYA_REGION, else us. */
  tuya_region: string;
  deepinfra_api_key: string;
  /** DeepInfra model id. Empty: the default. */
  assistant_model: string;
  assistant_speech: Speech;
  /** Kokoro voice, e.g. pf_dora. Empty: one for the language. */
  assistant_voice: string;
  /** City for the weather. Empty: located by IP. */
  assistant_location: string;
  /** Spotify app Client ID (developer.spotify.com). Empty: SPOTIFY_CLIENT_ID. */
  spotify_client_id: string;
  /** Last.fm API key: everyone's most played songs of an artist. Empty: LASTFM_API_KEY. */
  lastfm_api_key: string;
  /** Twitch app Client ID (dev.twitch.tv, Public type). Empty: TWITCH_CLIENT_ID. */
  twitch_client_id: string;
  /** Google Cloud OAuth client, "Desktop app" type, for Google Photos. Empty: GOOGLE_CLIENT_ID. */
  google_client_id: string;
  /** Empty: GOOGLE_CLIENT_SECRET. */
  google_client_secret: string;
  /** Minutes without input, with no music playing, before the screensaver starts. 0: never. */
  screensaver_minutes: number;
  /** How long each photo stays up. */
  screensaver_seconds: number;
  /** Parks the mouse pointer in the top-right corner, hidden, while the launcher is in front. */
  hide_cursor: boolean;
  /** A card on the TV ten minutes before each event of a calendar shown on the TV. */
  calendar_reminders: boolean;
  /** Where the drive to an event's place starts. Empty: no travel times. */
  home_address: string;
  /** The home server has a library. */
  library_on: boolean;
  /** The house's library: kept by the server, its keys masked. */
  radarr_url: string;
  radarr_api_key: string;
  sonarr_url: string;
  sonarr_api_key: string;
  qbittorrent_url: string;
  qbittorrent_username: string;
  qbittorrent_password: string;
  /** The quality profile new titles get, by name. */
  library_quality: string;
}

export interface PhotoAlbum {
  id: string;
  name: string;
  source: 'google' | 'folder';
  /** The folder picked, or where a Google album was saved. */
  path: string;
  count: number;
  /** Shown by the screensaver. */
  enabled: boolean;
  /** When added or last picked again (ms). */
  updated: number;
}

/** One photo for the screensaver. */
export interface Photo {
  path: string;
  album: string;
  /** When it was taken (ms); Google albums only. */
  taken: number | null;
}

/** A Google Photos pick in progress. */
export type PhotoPick =
  /** Waiting for photos to be chosen at `uri`; `qr` is that link as an SVG QR code. */
  | { stage: 'choosing'; uri: string; qr: string }
  | { stage: 'saving'; done: number; total: number };

export interface PhotosSnapshot {
  albums: PhotoAlbum[];
  google: ProviderLink;
  pick: PhotoPick | null;
}

/** How the assistant's replies are read out: Kokoro on this PC, Windows' voices, or not at all. */
export type Speech = 'local' | 'windows' | 'off';

/** The voice model on this PC ("voice-model" events). `done`/`total` are bytes while downloading. */
export interface VoiceModel {
  state: 'missing' | 'downloading' | 'unpacking' | 'ready' | 'failed';
  done: number;
  total: number;
  error: string | null;
}

/** Something the assistant did, shown under its reply. */
export interface AssistantCard {
  icon: string;
  title: string;
  subtitle: string;
  accent: boolean;
  /** A timer or alarm the card follows live: "timer:<id>", "alarm:<id>". */
  clock?: string;
}

/** A movie, series or game in a reply: a poster to watch or play it from. */
export interface AssistantMedia {
  /** "movie:<tmdb id>", "tv:<tmdb id>", or a game id. */
  id: string;
  kind: 'movie' | 'tv' | 'game';
  title: string;
  subtitle: string;
  /** Poster candidates, best first: URLs or file paths. */
  images: string[];
  backdrop: string | null;
  /** Movies and series: ids of the user's apps that stream it, best first. */
  apps: string[];
  /** Games: ready to play. */
  installed: boolean;
}

export type AssistantUi =
  | { type: 'navigate'; screen: string; query: string | null; pick: string | null }
  | { type: 'close' };

export interface AssistantReply {
  text: string;
  cards: AssistantCard[];
  media: AssistantMedia[];
  ui: AssistantUi[];
}

export interface Status {
  mode: Mode;
  running_app: string | null;
  /** Game id while a game runs, also when the launcher is in front of it. */
  running_game: string | null;
  pad: string | null;
}

export interface Game {
  /** "steam:<appid>" or "epic:<AppName>". */
  id: string;
  store: 'steam' | 'epic';
  name: string;
  installed: boolean;
  playtime_min: number | null;
  /** Unix seconds. */
  last_played: number | null;
  /** Image candidates, best first: file paths or URLs. */
  covers: string[];
  heroes: string[];
  favorite: boolean;
}

export interface Library {
  games: Game[];
  steam_found: boolean;
  steam_owned: boolean;
  epic_found: boolean;
  art_lookup: boolean;
}

export type Emu = 'retroarch' | 'pcsx2' | 'dolphin';

export type RetroStatus = 'ready' | 'installing' | 'bios' | 'no_core' | 'no_emulator' | 'no_folder' | 'empty';

/** A console on the Retro shelf. */
export interface RetroSystem {
  /** "n64", "ps1", "ps2", "gc", "md", "snes", "gba". */
  id: string;
  name: string;
  short: string;
  color: string;
  emu: Emu;
  emu_name: string;
  /** RetroArch core. */
  core: string | null;
  folder: string | null;
  /** ".z64 .n64" */
  exts: string;
  count: number;
  status: RetroStatus;
  /** "RetroArch · ready", "Installing PCSX2 · 64%", "PCSX2 · BIOS missing" */
  status_text: string;
  /** 0–1 while its emulator installs. */
  progress: number | null;
}

export interface RetroInstall {
  stage: 'checking' | 'downloading' | 'extracting' | 'configuring';
  version: string | null;
  /** Bytes (downloading, extracting) or cores (configuring) done, of `total`. */
  done: number;
  total: number;
  /** Bytes per second. */
  speed: number;
  /** Release found, download, extract, set up. */
  steps: [string, string, string, string];
}

export interface RetroEmulator {
  emu: Emu;
  name: string;
  /** "N64 · PlayStation · Mega Drive · SNES · GBA" */
  systems: string;
  /** Installed by the launcher, or the user's own folder; null when neither. */
  source: 'installed' | 'linked' | null;
  path: string | null;
  version: string | null;
  /** A newer version, found by Check for updates. */
  update: string | null;
  /** Bytes. */
  size: number | null;
  /** A linked copy: where its settings are. */
  settings: string | null;
  cores: { name: string; system: string; color: string; installed: boolean }[];
  install: RetroInstall | null;
  error: string | null;
  source_label: string;
  /** Where Install for me puts it. */
  target: string;
  bios_dir: string | null;
  /** Its program isn't in its folder anymore. */
  missing: boolean;
}

export interface SaveState {
  /** "Auto-save", "State 2" */
  label: string;
  /** null: the auto-save, which Resume loads. */
  slot: number | null;
  /** Unix seconds. */
  at: number;
  shot: string | null;
}

export interface RetroGame {
  /** "retro:n64:Super Mario 64 (USA).z64" */
  id: string;
  system: string;
  name: string;
  file: string;
  minutes: number;
  /** Unix seconds. */
  last_played: number | null;
  /** libretro's title screen. */
  art: string | null;
  /** Newest first. */
  states: SaveState[];
  resume: boolean;
}

export interface RetroBios {
  system: string;
  name: string;
  status: 'ok' | 'missing' | 'none';
  text: string;
}

export interface Retro {
  systems: RetroSystem[];
  emulators: RetroEmulator[];
  games: RetroGame[];
  bios: RetroBios[];
  bios_folder: string | null;
  tv_settings: string[];
  running: string | null;
  checked_updates: boolean;
}

/** What the folder picker is choosing a folder for. */
export type RetroTarget = { kind: 'emulator'; emu: Emu } | { kind: 'folder'; system: string } | { kind: 'bios' };

/** What a folder holds, for the picker's badges. */
export type FolderHolds =
  | { kind: 'emulator'; emu: Emu; version: string | null }
  /** Console folders inside: [console, path, games]. */
  | { kind: 'roms'; folders: [string, string, number][] }
  | { kind: 'games'; system: string; count: number }
  | { kind: 'bios'; files: string[] }
  | { kind: 'nothing' };

export interface FolderPlace {
  label: string;
  path: string;
  note: string;
  kind: 'drive' | 'folder' | 'net';
  group: string;
}

export interface FolderEntry {
  name: string;
  path: string;
  sub: string;
  holds: FolderHolds;
}

export interface FolderListing {
  /** null: the top, with the drives. */
  path: string | null;
  /** "" goes back to the drives; null at the top. */
  parent: string | null;
  places: FolderPlace[];
  entries: FolderEntry[];
}

export type FolderAlt =
  | { kind: 'emulator'; emu: Emu; label: string }
  | { kind: 'folders'; folders: [string, string, number][]; label: string }
  | { kind: 'folder'; system: string; label: string }
  | { kind: 'bios'; label: string };

export interface FolderCheck {
  verdict: 'match' | 'other' | 'none';
  title: string;
  path: string;
  lines: { tone: 'yes' | 'note'; text: string }[];
  settings: { keep: string; moving: string } | null;
  suggest: [string, string, number][];
  alt: FolderAlt | null;
}

export type DeviceKind = 'light' | 'strip' | 'switch' | 'plug' | 'fan' | 'ac' | 'sensor';
export type AcMode = 'cool' | 'heat' | 'auto' | 'fan' | 'dry';
export type AcFan = 'auto' | 'low' | 'mid' | 'high';

export interface Hs {
  /** 0–359. */
  h: number;
  /** 0–100. */
  s: number;
}

/** A capability is there when its field isn't null. */
export interface Device {
  /** "local:tuya:<id>", "local:ir:<remote>", "local:ewelink:<id>[:<channel>]", "group:<id>", or
   * from a cloud account "ewelink:<id>[:<channel>]" / "tuya:<id>[:<code>]". */
  id: string;
  provider: 'ewelink' | 'tuya';
  name: string;
  room: string | null;
  kind: DeviceKind;
  online: boolean;
  on: boolean | null;
  /** 1–100 %. */
  brightness: number | null;
  /** 0 warm – 100 cool. */
  color_temp: number | null;
  color: Hs | null;
  color_mode: 'white' | 'color' | null;
  temperature: number | null;
  humidity: number | null;
  power_w: number | null;
  /** 1..fan_speeds while on. */
  fan_speed: number | null;
  fan_speeds: number | null;
  ac: {
    mode: AcMode;
    target: number;
    min: number;
    max: number;
    fan: AcFan | null;
    modes: AcMode[];
    fans: AcFan[];
    /** Driven over infrared: the state is the last one sent. */
    ir: boolean;
    /** When the last IR command went out (ms). */
    sent_at: number | null;
  } | null;
  /** What it is and how it's reached: "Positivo bulb · Tuya local". */
  via: string | null;
  /** A group's devices. */
  members: string[];
}

export type DeviceCommand =
  | { type: 'power'; value: boolean }
  | { type: 'brightness'; value: number }
  | { type: 'color_temp'; value: number }
  | { type: 'color'; value: Hs }
  | { type: 'fan_speed'; value: number }
  | { type: 'ac_target'; value: number }
  | { type: 'ac_mode'; value: AcMode }
  | { type: 'ac_fan'; value: AcFan };

export interface SceneAction {
  device: string;
  on: boolean;
  brightness?: number;
  /** White at this temperature (0 warm – 100 cool)… */
  color_temp?: number;
  /** …or this colour. */
  color?: Hs;
  ac?: { mode: AcMode; target: number; fan?: AcFan | null };
}

export interface Scene {
  /** Empty when saving a new one. */
  id: string;
  name: string;
  icon: string;
  /** The card's colours: purple, blue, teal, warm, night or grey. */
  theme: string;
  /** The room it's one of the routines of; null: the whole house. */
  room?: string | null;
  actions: SceneAction[];
}

/** Runs a scene at a time of day. */
export interface Automation {
  /** Empty when saving a new one. */
  id: string;
  scene: string;
  hour: number;
  minute: number;
  /** Monday 0 … Sunday 6; empty for every day. */
  days: number[];
  enabled: boolean;
}

export interface DeviceGroup {
  id: string;
  name: string;
  members: string[];
}

/** How the dashboard is arranged. Tile ids are device ids, groups "group:<id>". */
export interface HomeLayout {
  order: string[];
  hidden: string[];
  /** Columns a tile spans (1 or 2); the screen picks when a tile isn't listed. */
  sizes: Record<string, number>;
  names: Record<string, string>;
  rooms: Record<string, string>;
  /** The rooms in order; one listed here is there even with nothing in it yet. */
  room_order: string[];
  /** Room → its icon (sofa, bed, pot, desk, bath, plant, door). */
  room_icons: Record<string, string>;
  groups: DeviceGroup[];
}

/** Something to do, on a room's list (or the whole house's). */
export interface HomeTask {
  id: string;
  room: string | null;
  title: string;
  created_at: number;
  /** When it was ticked off; it stays on the list, struck through, for a while. */
  done_at: number | null;
}

export type ScreenKind = 'tv' | 'tablet' | 'phone' | 'computer';

/** What a TV PC can do for the rest of the house. */
export interface ScreenCaps {
  remote: boolean;
  volume: boolean;
  apps: boolean;
  mic: boolean;
  voice: boolean;
  ring: boolean;
  /** Its Spotify Connect device's name, when the Spotify app runs there. */
  spotify_device?: string;
}

/** A TV PC, or a phone, tablet or laptop that opened the companion. */
export interface HomeScreen {
  id: string;
  name: string;
  kind: ScreenKind;
  /** The room it's in, which voice on it controls; null: it moves around or isn't placed. */
  room: string | null;
  last_seen: number;
  online: boolean;
  /** TV PCs: what they can do, once connected. */
  caps?: ScreenCaps;
  /** Phones, tablets, laptops: the TV PC this screen chose to control; none: its room's. */
  picks?: string;
  /** The TV PC this app runs on (marked by it; the home server lists every TV PC). */
  this?: boolean;
}

/** This TV PC's link to the home server (crates/agent-link's `Status`), as the `server` event and
 *  `link_status` carry it. */
export type LinkStatus =
  | { state: 'searching' }
  | { state: 'connecting'; server: string }
  /** Waiting for someone to approve this TV PC with the admin PIN: the code to type. */
  | { state: 'pairing'; server: string; code: string }
  | { state: 'online'; server: string; screen: HomeScreen; admin_pin: string; secure: boolean; fingerprint: string | null }
  | { state: 'offline'; server: string | null; error: string }
  | { state: 'refused'; server: string; reason: string };

export interface LinkInfo {
  status: LinkStatus | null;
  /** Typed in Settings; null: found on the network. */
  address: string | null;
  paired: boolean;
}

const sampleLink = (): LinkInfo => ({
  status: {
    state: 'online',
    server: '192.168.68.20:47960',
    screen: { id: 'tv', name: 'Living room TV', kind: 'tv', room: 'Sala', last_seen: 0, online: true, this: true },
    admin_pin: '4821',
    secure: true,
    fingerprint: '3F:9A:12:C4',
  },
  address: null,
  paired: true,
});

/** This TV PC's own entry. The sample house (and the old launcher's) has one TV, id "tv". */
export const isThisTv = (s: HomeScreen) => s.this === true || (s.kind === 'tv' && s.id === 'tv');

/** The last thing said or typed to the assistant. */
export interface Heard {
  said: string;
  /** The screen it was said to. */
  by: string;
  /** The room a request naming none was about. */
  room: string | null;
  reply: string;
  at: number;
}

export interface ProviderLink {
  configured: boolean;
  signed_in: boolean;
  connected: boolean;
  error: string | null;
}

export interface SmartHome {
  devices: Device[];
  scenes: Scene[];
  automations: Automation[];
  layout: HomeLayout;
  tasks: HomeTask[];
  screens: HomeScreen[];
  heard: Heard | null;
  /** Devices on the home network, reached with their own keys. */
  local: ProviderLink;
  ewelink: ProviderLink;
  tuya: ProviderLink;
}

export interface Track {
  uri: string;
  title: string;
  /** The artists, or the show for a podcast episode. */
  artist: string;
  album: string;
  year: string | null;
  duration_ms: number;
  image: string | null;
}

export type Repeat = 'off' | 'context' | 'track';

export interface Player {
  playing: boolean;
  progress_ms: number;
  track: Track | null;
  device: { name: string; kind: string; volume: number | null; local: boolean } | null;
  shuffle: boolean;
  repeat: Repeat;
  context_uri: string | null;
  /** What it's playing from: a playlist, album or artist name. */
  context: string | null;
  liked: boolean | null;
  next: Track | null;
}

export interface SpotifySnapshot {
  link: ProviderLink;
  /** Null: nothing playing or paused on any device. */
  player: Player | null;
}

export type SpotifyControl =
  | { type: 'play' | 'pause' | 'next' | 'previous' | 'transfer' }
  | { type: 'seek'; value: number }
  | { type: 'shuffle'; value: boolean }
  | { type: 'repeat'; value: Repeat }
  | { type: 'volume'; value: number }
  | { type: 'like'; value: boolean };

/** Something to play: a track, album, playlist, artist, show, or Liked Songs ("collection"). */
export interface MusicItem {
  uri: string;
  kind: 'track' | 'episode' | 'album' | 'playlist' | 'artist' | 'show' | 'collection';
  title: string;
  subtitle: string;
  image: string | null;
  /** What a track plays inside, so the album or playlist carries on after it. */
  context: string | null;
}

export type Shelf = 'recent' | 'playlists' | 'albums' | 'artists' | 'shows';

/** A playlist or Liked Songs with its songs, for the playlist screen. */
export interface Playlist {
  uri: string;
  title: string;
  /** "Your playlist", or who made it. */
  owner: string;
  description: string | null;
  image: string | null;
  /** Songs in it; more than `songs` holds for a long one. */
  total: number;
  /** Empty for someone else's playlist: Spotify only lists the songs of the user's own. */
  songs: Track[];
}

/** An artist with their releases, for the artist screen. */
export interface Artist {
  uri: string;
  name: string;
  image: string | null;
  /** Often empty: Spotify no longer fills this in for many artists. */
  genres: string[];
  /** Their place among the user's most played artists of the last four weeks, from 1. */
  top_rank: number | null;
  /** Null when Spotify couldn't say. */
  following: boolean | null;
  albums: MusicItem[];
  singles: MusicItem[];
  appears_on: MusicItem[];
  /** The user's most played songs of theirs: this month, the last 6 months, and all time. */
  most_played: [MostPlayed, MostPlayed, MostPlayed];
  /** When the launcher started counting plays (ms): "all time" starts there. */
  counted_since: number | null;
  /** Their newest album, EP or single. */
  latest: { release: MusicItem; date: string | null } | null;
  /** The user's plays of each album and single, by URI; never played ones left out. */
  release_plays: Record<string, number>;
}

/** A song in a most played list. */
export interface TopTrack {
  uri: string;
  title: string;
  album: string;
  image: string | null;
  /** Its album, so what follows it keeps playing. */
  context: string | null;
  /** The user's plays, or everyone's on Last.fm. Null for one of the user's Spotify top songs, which have no counts. */
  count: number | null;
  /** When the user last played it (ms). */
  last_played: number | null;
}

/** An artist's most played songs over a period. */
export interface MostPlayed {
  tracks: TopTrack[];
  /** The artist's plays in the period, and how many different songs. */
  plays: number;
  songs: number;
}

export interface Episode {
  uri: string;
  title: string;
  description: string;
  image: string | null;
  /** "2026-10-21"; just the month or year when that's all Spotify knows. */
  date: string | null;
  duration_ms: number;
  explicit: boolean;
  /** Listened to the end. Null when the sign-in can't read playback positions. */
  played: boolean | null;
  /** Where the user stopped, 0 when not started; null as for `played`. */
  resume_ms: number | null;
}

/** A podcast with its newest episodes, for the podcast screen. */
export interface Show {
  uri: string;
  title: string;
  description: string | null;
  image: string | null;
  /** Episodes in it; more than `episodes` holds for a long-running one. */
  total: number;
  /** Language codes, e.g. "pt" or "en-US". */
  languages: string[];
  explicit: boolean;
  following: boolean | null;
  /** Newest first. */
  episodes: Episode[];
}

export interface MusicSearch {
  tracks: MusicItem[];
  artists: MusicItem[];
  albums: MusicItem[];
  playlists: MusicItem[];
}

export interface TwitchMe {
  id: string;
  login: string;
  name: string;
  avatar: string | null;
}

export interface TwitchSnapshot {
  link: ProviderLink;
  me: TwitchMe | null;
}

export interface TwitchStream {
  user_id: string;
  login: string;
  name: string;
  title: string;
  game_id: string;
  game: string;
  viewers: number;
  /** RFC 3339. */
  started_at: string;
  /** Holds `{width}x{height}`: fill it in with `twitchImage`. Empty in the browser preview. */
  thumbnail: string;
  tags: string[];
  mature: boolean;
  avatar: string | null;
}

export interface TwitchCategory {
  id: string;
  name: string;
  /** Holds `{width}x{height}`: fill it in with `twitchImage`. */
  box_art: string;
}

export interface TwitchChannel {
  id: string;
  login: string;
  name: string;
  game: string;
  title: string;
  live: boolean;
  avatar: string | null;
}

export interface TwitchSearch {
  channels: TwitchChannel[];
  categories: TwitchCategory[];
}

export interface TwitchChannelInfo {
  id: string;
  login: string;
  name: string;
  avatar: string | null;
  /** Null while offline. */
  stream: TwitchStream | null;
  following: boolean;
}

/** Which live streams: the followed channels, the most watched, or one category's. */
export type TwitchSource = 'following' | 'top' | 'game';

const minutesAgo = (m: number) => new Date(Date.now() - m * 60_000).toISOString();

/** The design canvas's channels, for `npm run dev` in a plain browser. */
const SAMPLE_STREAMS: TwitchStream[] = (
  [
    ['speedrun_rafa', 'Hades II', 'Any% world record attempts · chat picks the weapon', 12400, 134, ['Português', 'Speedrun', 'Roguelike']],
    ['code_and_coffee', 'Software and Game Development', 'Building a TV launcher in Rust + Tauri, live', 1800, 242, ['English', 'Programming', 'Rust']],
    ['nightowl_tv', 'Just Chatting', 'Late night talk show with chat · Q&A at midnight', 6100, 97, ['Português', 'Talk show']],
    ['pixel_drift', 'Forza Horizon 5', 'Drift tuning session · community lobby open', 3200, 58, ['English', 'Racing']],
    ['mari_plays', 'Stardew Valley', 'Year 3 farm, cozy vibes and lofi', 940, 200, ['Português', 'Cozy']],
    ['retro_ricardo', 'Retro', 'SNES classics marathon · day 2', 2600, 405, ['Português', 'Retro']],
  ] as const
).map(([login, game, title, viewers, min, tags], i) => ({
  user_id: String(i + 1),
  login,
  name: login,
  title,
  game_id: String(100 + i),
  game,
  viewers,
  started_at: minutesAgo(min),
  thumbnail: '',
  tags: [...tags],
  mature: false,
  avatar: null,
}));

const SAMPLE_CATEGORIES: TwitchCategory[] = [
  'Just Chatting',
  'Hades II',
  'Software and Game Development',
  'Stardew Valley',
  'Forza Horizon 5',
  'Retro',
  'Spider-Man 2',
  'Sports',
].map((name, i) => ({ id: String(100 + i), name, box_art: '' }));

const SAMPLE_TWITCH: TwitchSnapshot = {
  link: { configured: true, signed_in: true, connected: true, error: null },
  me: { id: '0', login: 'you', name: 'You', avatar: null },
};

function searchTwitchSample(query: string, liveOnly: boolean): TwitchSearch {
  const q = query.trim().toLowerCase();
  const hit = (s: string) => s.toLowerCase().includes(q);
  const offline: TwitchChannel[] = [
    { id: '50', login: 'speedfan_br', name: 'speedfan_br', game: 'Super Mario 64', title: '120 star practice', live: false, avatar: null },
    { id: '51', login: 'spellbook_tv', name: 'spellbook_tv', game: 'Baldur’s Gate 3', title: 'Honour mode run', live: false, avatar: null },
  ];
  const live: TwitchChannel[] = SAMPLE_STREAMS.map((s) => ({
    id: s.user_id,
    login: s.login,
    name: s.name,
    game: s.game,
    title: s.title,
    live: true,
    avatar: null,
  }));
  return {
    channels: [...live, ...offline].filter((c) => (hit(c.name) || hit(c.game)) && (!liveOnly || c.live)),
    categories: SAMPLE_CATEGORIES.filter((c) => hit(c.name)),
  };
}

const sampleItem = (kind: MusicItem['kind'], title: string, subtitle: string, i: number): MusicItem => ({
  uri: `spotify:${kind}:${i}`,
  kind,
  title,
  subtitle,
  image: null,
  context: kind === 'track' ? 'spotify:album:sample' : null,
});

/** The design canvas's music, for `npm run dev` in a plain browser. */
const SAMPLE_SHELVES: Record<Shelf, MusicItem[]> = {
  recent: [
    ['playlist', 'Late night drive', 'Playlist'],
    ['album', 'Currents', 'Tame Impala · 2015'],
    ['album', 'Indigo Borboleta Anil', 'Liniker · 2021'],
    ['album', 'AM', 'Arctic Monkeys · 2013'],
    ['album', 'Clube da Esquina', 'Milton Nascimento, Lô Borges · 1972'],
    ['album', 'Blonde', 'Frank Ocean · 2016'],
  ].map(([k, t, s], i) => sampleItem(k as MusicItem['kind'], t, s, i)),
  playlists: [
    ['collection', 'Liked songs', '312 songs'],
    ['playlist', 'Late night drive', '48 songs'],
    ['playlist', 'Focus coding', '112 songs'],
    ['playlist', 'Brasilidades', '76 songs'],
    ['playlist', 'Sunday morning', '34 songs'],
  ].map(([k, t, s], i) => sampleItem(k as MusicItem['kind'], t, s, i)),
  albums: [
    ['Currents', 'Tame Impala · 2015'],
    ['AM', 'Arctic Monkeys · 2013'],
    ['Discovery', 'Daft Punk · 2001'],
  ].map(([t, s], i) => sampleItem('album', t, s, i)),
  artists: ['Tame Impala', 'Liniker', 'Milton Nascimento', 'Daft Punk'].map((t, i) =>
    sampleItem('artist', t, 'Artist', i),
  ),
  shows: [sampleItem('show', 'Sample podcast', 'Podcast', 0)],
};

const SAMPLE_SONGS: [string, string, string, string][] = [
  ['The Less I Know the Better', 'Tame Impala', 'Currents', '3:36'],
  ['Let It Happen', 'Tame Impala', 'Currents', '7:47'],
  ['Do I Wanna Know?', 'Arctic Monkeys', 'AM', '4:32'],
  ['Get Lucky', 'Daft Punk, Pharrell Williams, Nile Rodgers', 'Random Access Memories', '6:09'],
  ['Pink + White', 'Frank Ocean', 'Blonde', '3:04'],
  ['Nights', 'Frank Ocean', 'Blonde', '5:07'],
  ['Digital Love', 'Daft Punk', 'Discovery', '4:58'],
  ['R U Mine?', 'Arctic Monkeys', 'AM', '3:21'],
  ['Eventually', 'Tame Impala', 'Currents', '5:19'],
  ['Instant Crush', 'Daft Punk, Julian Casablancas', 'Random Access Memories', '5:37'],
  ['Why’d You Only Call Me When You’re High?', 'Arctic Monkeys', 'AM', '2:41'],
  ['New Person, Same Old Mistakes', 'Tame Impala', 'Currents', '6:03'],
];

/** The design canvas's playlist screen, whichever playlist or album is opened. */
function samplePlaylist(uri: string): Playlist {
  const tile = [...SAMPLE_SHELVES.playlists, ...SAMPLE_SHELVES.albums].find((p) => p.uri === uri);
  return {
    uri,
    title: tile?.title ?? 'Late night drive',
    owner: tile?.kind === 'album' ? tile.subtitle : tile?.kind === 'collection' ? 'Your songs' : 'Your playlist',
    description:
      tile?.kind === 'collection' || tile?.kind === 'album' ? null : 'Slow synths and city lights for the drive home.',
    image: null,
    total: SAMPLE_SONGS.length,
    songs: SAMPLE_SONGS.map(([title, artist, album, len], i) => {
      const [m, s] = len.split(':').map(Number);
      return {
        // The first one is the sample player's song, so it shows as playing.
        uri: i === 0 ? 'spotify:track:sample' : `spotify:track:s${i}`,
        title,
        artist,
        album,
        year: null,
        duration_ms: (m * 60 + s) * 1000,
        image: null,
      };
    }),
  };
}

/** The canvas's "By you" plays: song, album, plays this month / 6 months / all time, days since last played. */
const SAMPLE_PLAYS: [string, string, [number, number, number], number][] = [
  ['The Less I Know the Better', 'Currents', [42, 168, 611], 0],
  ['Let It Happen', 'Currents', [31, 120, 402], 0.2],
  ['Dracula', 'Deadbeat', [27, 54, 54], 1],
  ['Borderline', 'The Slow Rush', [24, 96, 288], 1.2],
  ['Loser', 'Deadbeat', [19, 41, 41], 3],
  ['Eventually', 'Currents', [15, 72, 233], 4],
  ['Feels Like We Only Go Backwards', 'Lonerism', [12, 58, 247], 8],
  ['Lost in Yesterday', 'The Slow Rush', [9, 44, 151], 9],
];

const sampleTop = (title: string, album: string, i: number, count: number | null, lastPlayed: number | null): TopTrack => ({
  // The first one is the sample player's song, so it shows as playing.
  uri: title === SAMPLE_SPOTIFY.player?.track?.title ? 'spotify:track:sample' : `spotify:track:top${i}`,
  title,
  album,
  image: null,
  context: 'spotify:album:sample',
  count,
  last_played: lastPlayed,
});

/** The design canvas's artist screen, whichever artist is opened. */
function sampleArtist(uri: string): Artist {
  const release = (kind: string, rows: [string, string][]) =>
    rows.map(([title, year], i) => ({ ...sampleItem('album', title, `${kind} · ${year}`, i), uri: `spotify:album:${kind}${i}` }));
  const albums = release('Album', [
    ['Deadbeat', '2025'],
    ['The Slow Rush', '2020'],
    ['Currents', '2015'],
    ['Lonerism', '2012'],
    ['Live Versions', '2014'],
    ['Innerspeaker', '2010'],
  ]);
  const now = Date.now();
  const period = (p: 0 | 1 | 2, plays: number, songs: number): MostPlayed => ({
    tracks: SAMPLE_PLAYS.map(([title, album, counts, days], i) => sampleTop(title, album, i, counts[p], now - days * DAY - 3_600_000))
      .sort((a, b) => b.count! - a.count!),
    plays,
    songs,
  });
  return {
    uri,
    name: SAMPLE_SHELVES.artists.find((a) => a.uri === uri)?.title ?? 'Tame Impala',
    image: null,
    genres: ['psychedelic rock', 'neo-psychedelia', 'indie'],
    top_rank: 1,
    following: true,
    albums,
    singles: release('Single', [
      ['Dracula', '2025'],
      ['Loser', '2025'],
      ['End of Summer', '2025'],
      ['Lost in Yesterday', '2020'],
      ['Borderline', '2019'],
      ['Patience', '2019'],
    ]),
    appears_on: [],
    most_played: [period(0, 179, 14), period(1, 653, 26), period(2, 2027, 38)],
    counted_since: now - 400 * DAY,
    latest: { release: albums[0], date: '2025-10-17' },
    release_plays: { [albums[0].uri]: 46, [albums[1].uri]: 33, [albums[2].uri]: 88, [albums[3].uri]: 12 },
  };
}

/** The canvas's "Everyone" list; the counts are made up. */
const samplePopular = (): TopTrack[] =>
  (
    [
      ['The Less I Know the Better', 'Currents'],
      ['Let It Happen', 'Currents'],
      ['Feels Like We Only Go Backwards', 'Lonerism'],
      ['New Person, Same Old Mistakes', 'Currents'],
      ['Borderline', 'The Slow Rush'],
      ['Elephant', 'Lonerism'],
      ['Eventually', 'Currents'],
      ['Dracula', 'Deadbeat'],
    ] as const
  ).map(([title, album], i) => sampleTop(title, album, 20 + i, Math.round(9_000_000 / (1 + i * 0.4)), null));

const DAY = 86_400_000;
const isoDay = (ms: number) => new Date(ms).toISOString().slice(0, 10);

/** A podcast for the browser preview: newest episode new, the next one half heard. */
function sampleShow(uri: string): Show {
  const now = Date.now();
  const episodes: Episode[] = [58, 72, 49, 65, 54, 61].map((min, i) => ({
    uri: `spotify:episode:s${i}`,
    title: `#${214 - i} · Episode title`,
    description: 'The episode description from Spotify goes here, two lines at most on the podcast screen.',
    image: null,
    date: isoDay(now - (2 + i * 7) * DAY),
    duration_ms: min * 60_000,
    explicit: false,
    played: i >= 2 && i < 4,
    resume_ms: i === 1 ? 45 * 60_000 : 0,
  }));
  return {
    uri,
    title: SAMPLE_SHELVES.shows.find((s) => s.uri === uri)?.title ?? 'Sample podcast',
    description: 'The show description from Spotify goes here: what the podcast is about and who hosts it.',
    image: null,
    total: 214,
    languages: ['pt'],
    explicit: true,
    following: true,
    episodes,
  };
}

const SAMPLE_SPOTIFY: SpotifySnapshot = {
  link: { configured: true, signed_in: true, connected: true, error: null },
  player: {
    playing: true,
    progress_ms: 84000,
    track: {
      uri: 'spotify:track:sample',
      title: 'The Less I Know the Better',
      artist: 'Tame Impala',
      album: 'Currents',
      year: '2015',
      duration_ms: 216000,
      image: null,
    },
    device: { name: 'This PC', kind: 'Computer', volume: 70, local: true },
    shuffle: false,
    repeat: 'off',
    context_uri: 'spotify:playlist:sample',
    context: 'Late night drive',
    liked: true,
    next: {
      uri: 'spotify:track:next',
      title: 'Let It Happen',
      artist: 'Tame Impala',
      album: 'Currents',
      year: '2015',
      duration_ms: 467000,
      image: null,
    },
  },
};

/** Browser preview: applies a control to the sample player. */
function controlSpotifySample(c: SpotifyControl) {
  const p = SAMPLE_SPOTIFY.player!;
  switch (c.type) {
    case 'play':
    case 'pause':
      p.playing = c.type === 'play';
      break;
    case 'seek':
      p.progress_ms = c.value;
      break;
    case 'shuffle':
      p.shuffle = c.value;
      break;
    case 'repeat':
      p.repeat = c.value;
      break;
    case 'like':
      p.liked = c.value;
      break;
    case 'next':
    case 'previous':
      p.progress_ms = 0;
      break;
  }
}

const sampleDevice = (
  id: string,
  name: string,
  room: string | null,
  provider: Device['provider'],
  kind: DeviceKind,
  extra: Partial<Device>,
): Device => ({
  id,
  provider,
  name,
  room,
  kind,
  online: true,
  on: false,
  brightness: null,
  color_temp: null,
  color: null,
  color_mode: null,
  temperature: null,
  humidity: null,
  power_w: null,
  fan_speed: null,
  fan_speeds: null,
  ac: null,
  via: null,
  members: [],
  ...extra,
});

const sampleBulb = (id: string, name: string, room: string, on: boolean, extra: Partial<Device> = {}) =>
  sampleDevice(`local:tuya:${id}`, name, room, 'tuya', 'light', {
    on,
    brightness: 80,
    color_temp: 20,
    color: { h: 30, s: 100 },
    color_mode: 'white',
    via: 'Positivo bulb · Tuya local',
    ...extra,
  });

/** The devices found on the user's network, for `npm run dev` in a plain browser. */
const SAMPLE_HOME: SmartHome = {
  devices: [
    sampleDevice('local:ir:ac-gabriel', 'Ar Condicionado Gabriel', 'Quarto', 'tuya', 'ac', {
      on: true,
      via: 'Controle Universal Gabriel · IR',
      ac: {
        mode: 'cool',
        target: 22,
        min: 18,
        max: 30,
        fan: 'auto',
        modes: ['cool', 'heat', 'auto', 'fan', 'dry'],
        fans: ['auto', 'low', 'mid', 'high'],
        ir: true,
        sent_at: Date.now() - 6 * 60_000,
      },
    }),
    sampleDevice('local:ewelink:sw:0', 'Interruptor Gabriel 1', 'Quarto', 'ewelink', 'switch', {
      on: true,
      via: 'Sonoff SwitchMan M5 · LAN',
    }),
    sampleDevice('local:ewelink:sw:1', 'Interruptor Gabriel 2', 'Quarto', 'ewelink', 'switch', { via: 'Sonoff SwitchMan M5 · LAN' }),
    sampleDevice('local:ewelink:sw:2', 'Interruptor Gabriel 3', 'Quarto', 'ewelink', 'switch', { via: 'Sonoff SwitchMan M5 · LAN' }),
    sampleBulb('mesa', 'Abajur mesa', 'Quarto', true, { brightness: 100, color_temp: 70 }),
    sampleBulb('abajur', 'Abajur', 'Quarto', false, { brightness: 2 }),
    sampleBulb('sala', 'Abajur sala', 'Sala', true, { brightness: 60, color: { h: 265, s: 85 }, color_mode: 'color' }),
    sampleBulb('janela', 'Abajur janela', 'Sala', false, { brightness: 100 }),
    sampleBulb('corredor', 'Abajur corredor', 'Corredor', false, { brightness: 52, color_temp: 25 }),
    sampleDevice('local:ewelink:ledmesa', 'led mesa', null, 'ewelink', 'strip', { online: false, via: 'Sonoff LED strip · LAN' }),
    sampleDevice('local:ewelink:ledcama', 'led cama', null, 'ewelink', 'strip', { online: false, via: 'Sonoff LED strip · LAN' }),
  ],
  scenes: [
    {
      id: 'movie',
      name: 'Movie night',
      icon: 'movie',
      theme: 'purple',
      actions: [
        { device: 'local:tuya:sala', on: true, brightness: 10, color: { h: 265, s: 85 } },
        { device: 'local:tuya:janela', on: false },
        { device: 'local:ir:ac-gabriel', on: true, ac: { mode: 'cool', target: 22, fan: 'auto' } },
      ],
    },
    { id: 'night', name: 'Good night', icon: 'moon', theme: 'night', actions: [{ device: 'local:tuya:mesa', on: false }] },
  ],
  automations: [{ id: 'a1', scene: 'night', hour: 23, minute: 30, days: [], enabled: true }],
  layout: { order: [], hidden: [], sizes: {}, names: {}, rooms: {}, room_order: ['Sala', 'Quarto', 'Corredor'], room_icons: {}, groups: [] },
  tasks: [
    { id: 't1', room: 'Sala', title: 'Water the plants', created_at: Date.now() - 26 * 3600_000, done_at: null },
    { id: 't2', room: 'Sala', title: 'Charge the 8BitDo', created_at: Date.now() - 3 * 3600_000, done_at: null },
    { id: 't3', room: 'Quarto', title: 'Clean the AC filter', created_at: Date.now() - 4 * 86400_000, done_at: null },
    { id: 't4', room: 'Quarto', title: 'Change the sheets', created_at: Date.now() - 2 * 86400_000, done_at: Date.now() - 40 * 60_000 },
  ],
  screens: [
    { id: 'tv', name: 'Living room TV', kind: 'tv', room: 'Sala', last_seen: 0, online: true },
    { id: 'sample-ipad', name: 'iPad mini', kind: 'tablet', room: 'Quarto', last_seen: Date.now(), online: true },
    { id: 'sample-phone', name: 'Phone', kind: 'phone', room: null, last_seen: Date.now() - 3 * 3600_000, online: false },
  ],
  heard: { said: 'Turn off the lights', by: 'iPad mini', room: 'Quarto', reply: 'Done, the 2 lights in the Quarto are off.', at: Date.now() - 12 * 60_000 },
  local: { configured: true, signed_in: true, connected: true, error: null },
  ewelink: { configured: false, signed_in: false, connected: false, error: null },
  tuya: { configured: false, signed_in: false, connected: false, error: null },
};

/** Browser preview: the sample home with the layout's names, rooms and groups applied, as Rust does. */
function sampleSnapshot(): SmartHome {
  const h = structuredClone(SAMPLE_HOME);
  const L = h.layout;
  for (const d of h.devices) {
    if (L.names[d.id]) d.name = L.names[d.id];
    if (L.rooms[d.id]) d.room = L.rooms[d.id];
  }
  for (const g of L.groups) {
    const ms = g.members.flatMap((m) => h.devices.find((d) => d.id === m) ?? []);
    if (!ms.length) continue;
    const first = ms[0];
    const lit = ms.filter((m) => m.on && m.online);
    const pool = lit.length ? lit : ms;
    h.devices.push({
      ...first,
      id: `group:${g.id}`,
      name: g.name,
      room: L.rooms[`group:${g.id}`] ?? first.room,
      kind: ms.every((m) => m.kind === first.kind) ? first.kind : 'switch',
      online: ms.some((m) => m.online),
      on: lit.length > 0,
      brightness: ms.every((m) => m.brightness !== null)
        ? Math.round(pool.reduce((n, m) => n + (m.brightness ?? 0), 0) / pool.length)
        : null,
      ac: null,
      via: `Group · ${ms.length} devices`,
      members: ms.map((m) => m.id),
    });
  }
  return h;
}

/** Browser preview: saves a scene into the sample home. */
function putSampleScene(scene: Scene): string {
  const s = { ...scene, id: scene.id || `scene-${Date.now()}` };
  SAMPLE_HOME.scenes = [...SAMPLE_HOME.scenes.filter((x) => x.id !== s.id), s];
  return s.id;
}

/** Browser preview: the lights in a room get `c` (and come on for a level or a warmth). */
function controlSampleRoom(room: string, c: DeviceCommand) {
  const groups = SAMPLE_HOME.layout.groups.filter((g) => SAMPLE_HOME.layout.rooms[`group:${g.id}`] === room);
  for (const d of SAMPLE_HOME.devices) {
    const r = SAMPLE_HOME.layout.rooms[d.id] ?? d.room;
    const inRoom = r === room || groups.some((g) => g.members.includes(d.id));
    if (!inRoom || !d.online || !['light', 'strip', 'switch'].includes(d.kind)) continue;
    if (c.type === 'brightness' && d.brightness === null) continue;
    if (c.type === 'color_temp' && d.color_temp === null) continue;
    if (c.type !== 'power') d.on = true;
    controlSample(d.id, c);
  }
}

/** Browser preview: a room's name changed everywhere. */
function moveSampleRoom(from: string, to: string | null) {
  const h = SAMPLE_HOME;
  const L = h.layout;
  for (const d of h.devices) if (d.room === from) d.room = to;
  for (const [id, r] of Object.entries(L.rooms)) {
    if (r !== from) continue;
    if (to) L.rooms[id] = to;
    else delete L.rooms[id];
  }
  L.room_order = L.room_order.flatMap((r) => (r === from ? (to ? [to] : []) : [r]));
  if (L.room_icons[from] !== undefined) {
    if (to) L.room_icons[to] = L.room_icons[from];
    delete L.room_icons[from];
  }
  for (const s of h.scenes) if (s.room === from) s.room = to;
  for (const t of h.tasks) if (t.room === from) t.room = to;
  for (const s of h.screens) if (s.room === from) s.room = to;
}

/** Browser preview: applies a command to the sample devices (a group's to its members). */
function controlSample(id: string, c: DeviceCommand) {
  const group = SAMPLE_HOME.layout.groups.find((g) => `group:${g.id}` === id);
  if (group) {
    group.members.forEach((m) => controlSample(m, c));
    return;
  }
  const d = SAMPLE_HOME.devices.find((x) => x.id === id);
  if (!d) return;
  switch (c.type) {
    case 'power':
      d.on = c.value;
      if (d.kind === 'fan') d.fan_speed = c.value ? 1 : null;
      break;
    case 'brightness':
      d.brightness = c.value;
      d.on = true;
      break;
    case 'color_temp':
      d.color_temp = c.value;
      if (d.color) d.color_mode = 'white';
      d.on = true;
      break;
    case 'color':
      d.color = c.value;
      d.color_mode = 'color';
      d.on = true;
      break;
    case 'fan_speed':
      d.fan_speed = c.value;
      d.on = true;
      break;
    case 'ac_target':
      if (d.ac) d.ac.target = c.value;
      d.on = true;
      break;
    case 'ac_mode':
      if (d.ac) d.ac.mode = c.value;
      break;
    case 'ac_fan':
      if (d.ac) d.ac.fan = c.value;
      break;
  }
}

const steamArt = (appid: number, file: string) =>
  `https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/${appid}/${file}`;

const SAMPLE_LIBRARY: Library = {
  games: (
    [
      [1145360, 'Hades', 2280, 2],
      [1245620, 'Elden Ring', 6720, 5],
      [1091500, 'Cyberpunk 2077', 3840, 12],
      [367520, 'Hollow Knight', 2460, 30],
      [1551360, 'Forza Horizon 5', 0, 0],
      [413150, 'Stardew Valley', 5400, 60],
      [407530, 'ARK: Survival Of The Fittest', 0, 0],
    ] as const
  ).map(([appid, name, min, daysAgo], i) => ({
    id: `steam:${appid}`,
    store: 'steam' as const,
    name,
    installed: i % 3 !== 2,
    playtime_min: min || null,
    last_played: daysAgo ? Math.floor(Date.now() / 1000) - daysAgo * 86400 : null,
    covers: [steamArt(appid, 'library_600x900.jpg')],
    heroes: [steamArt(appid, 'library_hero.jpg')],
    favorite: i === 1,
  })),
  steam_found: true,
  steam_owned: true,
  epic_found: false,
  art_lookup: false,
};

const SAMPLE_APPS: AppEntry[] = [
  ['netflix', 'Netflix'],
  ['prime', 'Prime Video'],
  ['disney', 'Disney+'],
  ['hbomax', 'HBO Max'],
  ['youtube', 'YouTube'],
  ['twitch', 'Twitch'],
  ['globoplay', 'Globoplay'],
].map(([id, name]) => ({ id, name, kind: 'web', url: null, search_url: null, icon: null, color: null }));

const SAMPLE_REC_SHELVES: RecShelf[] = [
  { key: 'for-you', kind: 'for_you', media_type: null, id: null, label: null },
  { key: 'because:tv:1', kind: 'because', media_type: 'tv', id: 1, label: 'Dark' },
  { key: 'trending', kind: 'trending', media_type: null, id: null, label: null },
  { key: 'genre:878', kind: 'genre', media_type: null, id: 878, label: null },
  { key: 'person:2', kind: 'person', media_type: null, id: 2, label: 'Pedro Pascal' },
];

const sampleRecRow = (shelf: RecShelf): RecTitle[] =>
  ['Severance', 'Arcane', 'Dune: Part Two', 'The Bear', 'Shōgun', 'Andor', 'Past Lives', 'Fallout'].map((title, i) => ({
    id: shelf.key.length * 100 + i,
    media_type: i % 2 ? 'movie' : 'tv',
    title,
    year: String(2020 + (i % 5)),
    overview: 'A sample synopsis: the preview has no TMDB access, so these stand in for the real recommendations.',
    poster: null,
    backdrop: null,
    services: [['netflix', 'prime', 'hbomax', 'disney'][i % 4]],
  }));

const SAMPLE_WATCHLIST: WatchListItem[] = [
  ['Interstellar', 'movie', 'prime'],
  ['Andor', 'tv', 'disney'],
  ['Succession', 'tv', 'hbomax'],
  ['Wednesday', 'tv', 'netflix'],
  ['Oppenheimer', 'movie', 'prime'],
].map(([title, media_type, service], i) => ({
  service,
  media_type: media_type as Title['media_type'],
  id: 900 + i,
  title,
  poster: null,
  backdrop: null,
  added_at: 0,
}));

// ---------- Sports ----------

export type SportsPhase = 'pre' | 'live' | 'post';

export interface SportsTeam {
  id: string;
  name: string;
  abbr: string;
  logo: string | null;
  color: string;
  score: string | null;
  winner: boolean;
}

/** Where a game is on in Brazil: an apps.toml app (at `url`, or its start page) or just a page. */
export interface SportsChannel {
  name: string;
  app: string | null;
  url: string | null;
  /** A live channel whose programme is known: plays with the launcher's overlay. */
  tv?: string | null;
}

/** A programme on a sports channel. */
export interface TvSlot {
  /** Unix ms. */
  start: number;
  end: number;
  /** "Grêmio x Internacional", "SportsCenter". */
  title: string;
  /** "Brasileirão · Round 32", "Studio". */
  sub: string;
  /** live: a live event; air: a show; replay: an event again. */
  kind: 'live' | 'air' | 'replay';
  teams: [string, string] | null;
  competition: string | null;
  desc: string | null;
  /** The programme's own picture (Globoplay). */
  image: string | null;
  /** The ESPN game it shows. */
  game: SportsGame | null;
  /** A competition the launcher follows. */
  followed: boolean;
}

/**
 * A live channel with its programme: Globoplay's channels, ESPN 1–6, CazéTV's live streams, and
 * on the Live tab the news channels on YouTube.
 */
export interface TvChannel {
  key: string;
  name: string;
  color: string;
  /** Sports channels' chips; outside sports "news" (GloboNews, CNN Brasil…) and "tv". */
  group: 'sportv' | 'premiere' | 'espn' | 'cazetv' | 'more' | 'news' | 'tv';
  provider: 'globo' | 'espn' | 'youtube';
  /** "Globoplay Premium", "Premiere", "Free with login", "Disney+ Premium". */
  access: string;
  free: boolean;
  /** A frame of what's on air (Globoplay, YouTube). */
  frame: string | null;
  app: string;
  /** Null while there's nothing to play (a news channel with nothing live). */
  url: string | null;
  /** Oldest first. */
  slots: TvSlot[];
  /** Watching a YouTube stream now. */
  viewers: number | null;
}

export interface SportsGame {
  /** ESPN's event id; "ge-<id>" for a match only ge knows. */
  id: string;
  league: string;
  league_name: string;
  sport: 'soccer' | 'basketball';
  /** Unix ms. */
  start: number;
  phase: SportsPhase;
  /** "Q3 · 7:46", "67'", "HT", "Final", "Full time"; empty before the start. */
  status: string;
  /** 0..1. */
  progress: number;
  home: SportsTeam;
  away: SportsTeam;
  venue: string | null;
  stage: string | null;
  /** Watchable ones first. */
  channels: SportsChannel[];
  mine: 'gremio' | 'lakers' | null;
  aggregate: string | null;
  /** ge's highlights on Globoplay. */
  ge_video: string | null;
  /** Postponed, canceled. */
  note: string | null;
}

export interface SportsHome {
  featured: SportsGame | null;
  /** Featured now: the hero first, the other live games, Grêmio's next. */
  rail: SportsGame[];
  /** The hero's scorers, events and stats, once it started. */
  hero: SportsGameDetail | null;
  gremio_next: SportsGame | null;
  for_you: SportsGame[];
  gremio: SportsGame[];
  nba: SportsGame[];
  football: SportsGame[];
  /** Live on TV: channels with their now and next. */
  tv: TvChannel[];
  competitions: { id: string; name: string; color: string; channels: string }[];
  /** Ids of games with a reminder. */
  reminders: string[];
  sources: { name: string; updated: number | null; error: string | null }[];
}

export interface SportsStat {
  name: string;
  home: string;
  away: string;
  home_v: number;
  away_v: number;
}

export interface MatchEvent {
  minute: number;
  clock: string;
  kind: 'goal' | 'yellow' | 'red' | 'sub';
  home: boolean;
  title: string;
  detail: string;
}

export interface Lineup {
  formation: string;
  /** On the home team's half: depth `x` 0..0.5 from its own goal, width `y` 0..1. */
  players: { n: string; name: string; x: number; y: number }[];
  subs: string[];
}

export interface SoccerDetail {
  /** Most recent first. */
  events: MatchEvent[];
  home_scorers: string;
  away_scorers: string;
  stats: SportsStat[];
  home_lineup: Lineup;
  away_lineup: Lineup;
  /** Attack momentum by 3-minute spell up to now, −1 (away) to 1 (home). */
  momentum: number[];
  /** Both teams in the league table, home first; `moved`: places this result moved it. */
  table: { team: string; rank: number; points: number; moved: number }[];
}

export interface BoxRow {
  name: string;
  pos: string;
  starter: boolean;
  min: string;
  pts: string;
  reb: string;
  ast: string;
  fg: string;
  tp: string;
  ft: string;
  stl: string;
  blk: string;
  pm: string;
}

export interface NbaDetail {
  periods: string[];
  away_line: string[];
  home_line: string[];
  stats: SportsStat[];
  /** [minute, the away team's chance to win 0–100], oldest first. */
  win_prob: [number, number][];
  leaders: { name: string; team: string; color: string; category: string; value: string }[];
  away_box: BoxRow[];
  home_box: BoxRow[];
  /** Most recent first; `score` is away–home. */
  plays: { clock: string; team: string | null; text: string; score: string; scoring: boolean }[];
  info: string[];
  /** Field goals: `x` 0 at the home basket's end, 1 at the away's; `y` across, both 0..1. */
  shots: { home: boolean; x: number; y: number; made: boolean }[];
  facts: { label: string; value: string }[];
}

export interface SportsGameDetail {
  game: SportsGame;
  soccer: SoccerDetail | null;
  nba: NbaDetail | null;
  /** The team's next match, for a finished game of Grêmio. */
  next: SportsGame | null;
  reminder: boolean;
  overlay: boolean;
  /** Live channels showing it now or soon, each with that slot. */
  on_tv: TvChannel[];
}

export interface TableRow {
  rank: number;
  team: SportsTeam;
  played: string;
  wins: string;
  draws: string;
  losses: string;
  goal_diff: string;
  points: string;
  zone: string | null;
  /** Last five, oldest first: "WWDLW" (empty when unknown). */
  form: string;
  /** Playing now: places the live score moves it. */
  live: number | null;
}

export interface SportsTable {
  name: string | null;
  rows: TableRow[];
  zones: { color: string; label: string }[];
}

export interface Tie {
  stage: string;
  home: SportsTeam;
  away: SportsTeam;
  legs: SportsGame[];
  /** Goals over both legs, `home` first. */
  aggregate: [number, number] | null;
  mine: boolean;
}

export interface Competition {
  id: string;
  name: string;
  color: string;
  channels: string;
  season: string | null;
  tables: SportsTable[];
  table_source: string | null;
  ties: Tie[];
  stages: { label: string; detail: string; current: boolean }[];
  days: { day: string; games: SportsGame[] }[];
  gremio: {
    summary: string | null;
    next: SportsGame | null;
    road: { stage: string; opponent: string; legs: string; result: string; next: boolean }[];
    /** The next few here, the live one first. */
    fixtures: SportsGame[];
  } | null;
  round: number | null;
  rounds: number | null;
  round_games: SportsGame[];
}

// ---------- Live now ----------

/** A live stream on a YouTube channel the account subscribes to. */
export interface YouTubeLive {
  id: string;
  title: string;
  channel: string;
  avatar: string | null;
  /** YouTube's category: "Gaming", "Music", "Sports". */
  category: string | null;
  viewers: number;
  /** When it went live, unix ms. */
  started: number | null;
  thumb: string | null;
}

/** Home's Live now row. A source that isn't set up, or fails, comes back empty. */
export interface LiveNow {
  sports: {
    /** Live games that can be watched, by priority (Grêmio, the Lakers, then by region). */
    games: SportsGame[];
    /** Channels with a live event on, each holding only that slot: those showing `games` too. */
    channels: TvChannel[];
  };
  /** The followed channels that are live. */
  twitch: TwitchStream[];
  /** The subscriptions' live streams, the most watched first. */
  youtube: YouTubeLive[];
}

/** The design canvas's YouTube streams, for the plain-browser preview. */
const SAMPLE_YOUTUBE: YouTubeLive[] = (
  [
    ['yt1', 'Resenha FC', 'Pós-rodada ao vivo: analisando os jogos de segunda', 'Sports', 8700, 48],
    ['yt2', 'lofi.radio', 'lofi beats to code and relax to · 24/7 radio', 'Music', 21000, 900],
    ['yt3', 'Fala Dev', 'Live: REST API em Rust do zero', 'Science & Technology', 1100, 65],
  ] as const
).map(([id, channel, title, category, viewers, min]) => ({
  id,
  title,
  channel,
  avatar: null,
  category,
  viewers,
  started: Date.now() - min * 60_000,
  thumb: null,
}));

const sampleLiveNow = (): LiveNow => ({
  sports: sampleLiveSports(),
  twitch: SAMPLE_STREAMS.slice(0, 4),
  youtube: SAMPLE_YOUTUBE,
});

// ---------- Live tab ----------

/** A news channel on YouTube, as listed for the Live tab. */
export interface NewsSource {
  /** Its YouTube channel id, "UC…". */
  id: string;
  name: string;
  color: string;
}

/** A channel YouTube's search found, to add to the news channels. */
export interface NewsFound {
  id: string;
  name: string;
  /** "@bandjornalismo". */
  handle: string | null;
  /** "7,46 mi de inscritos". */
  subscribers: string | null;
  avatar: string | null;
}

export interface LiveTv {
  /** Globoplay's lineup, ESPN's, CazéTV's, then the news channels; the games they show filled in. */
  channels: TvChannel[];
  /** Live sports (games on the services too), Twitch and the YouTube subscriptions, as on Home. */
  now: LiveNow;
  /** Channel keys pinned. */
  favorites: string[];
  /** Programmes with a reminder, as "<channel key>:<start>". */
  reminders: string[];
  news: NewsSource[];
  /** What isn't loading. */
  errors: string[];
}

/** The calendar's reminder card, for a TV programme: `tv` is the channel A tunes in to. */
export type TvReminder = CalendarEvent & { tv?: string };

const sampleLiveTv = (): LiveTv => ({
  channels: sampleLineup(),
  now: sampleLiveNow(),
  favorites: ['premiere', 'yt-UCUE2SZQrlscY_rMphL6G5aQ', 'yt-UCvdwhh_fDyWccR42-rReZLw'],
  reminders: [],
  news: SAMPLE_NEWS,
  errors: [],
});

export interface Highlights {
  /** Played in the launcher, best first. */
  youtube: { id: string; title: string; channel: string; length: string | null }[];
  /** ge's video on Globoplay. */
  globoplay: string | null;
  globoplay_thumb: string | null;
}

/** A calendar on the Calendar screen's chips. */
export interface CalendarInfo {
  id: string;
  name: string;
  color: string;
  /** On the TV: its events on the screen, as Home's next event and as reminders. */
  shown: boolean;
}

export interface CalendarEvent {
  /** Google's id of this occurrence; "gremio-<ESPN id>" for a match. */
  id: string;
  calendar: string;
  title: string;
  /** Unix ms. All day: UTC midnights; place those by `date` and `end_date` instead. */
  start: number;
  end: number;
  /** All day: the first day, "2026-10-27". */
  date: string | null;
  /** All day: the day after the last. */
  end_date: string | null;
  color: string;
  location: string | null;
  description: string | null;
  /** The video call's link. */
  meet: string | null;
  /** Declined in the invitation. */
  declined: boolean;
  /** A reminder will show on the TV. */
  remind: boolean;
  /** Grêmio's match, for its match center. */
  game: SportsGame | null;
}

/** What the Calendar screen shows. */
export interface CalendarAgenda {
  /** The Google Calendar account. */
  link: ProviderLink;
  /** Google's calendars, then Grêmio's matches. */
  calendars: CalendarInfo[];
  /** Those of the range asked for and of the next days, of every calendar, shown or not. */
  events: CalendarEvent[];
  /** When Google was last read. */
  synced: number | null;
  /** Reminders on the TV are on (Settings). */
  reminders: boolean;
}

/** The drive from the home address in Settings. */
export interface Travel {
  km: number;
  minutes: number;
}

export interface ClockTimer {
  id: string;
  label: string;
  /** Named by the user, rather than after its length ("12-minute timer"). */
  named: boolean;
  emoji: string;
  color: string;
  /** Its length, with the time added since. */
  total_ms: number;
  /** Running: when it goes off. */
  end_at: number | null;
  /** Paused: what was left. */
  left_ms: number | null;
}

/** What an alarm plays. */
export type AlarmSound = { type: 'chime' } | { type: 'spotify'; uri: string; name: string };

export interface ClockAlarm {
  id: string;
  hour: number;
  minute: number;
  label: string;
  /** Monday 0 … Sunday 6; none: once. */
  days: number[];
  /** Once, on this day ("2026-10-27"). */
  date: string | null;
  enabled: boolean;
  sound: AlarmSound;
  /** A smart home scene run as it goes off. */
  scene: string | null;
  next_at: number | null;
  /** Snoozed: rings again then. */
  snoozed: number | null;
}

/** A timer or alarm going off. */
export interface ClockRing {
  /** The timer's or the alarm's id. */
  id: string;
  kind: 'timer' | 'alarm';
  label: string;
  emoji: string;
  color: string;
  /** When it went off. */
  since: number;
  /** Timers: how long it was. */
  total_ms: number;
  /** Alarms: what else is on today. */
  chips: { color: string; text: string }[];
  /** What else ringing does: "Netflix is paused until you stop the timer". */
  note: string;
}

export interface Clock {
  timers: ClockTimer[];
  alarms: ClockAlarm[];
  /** The first is on screen; the rest follow as each is answered. */
  ringing: ClockRing[];
}

/** An alarm as the Clock screen saves it: new without an id. */
export interface AlarmEdit {
  id: string | null;
  hour: number;
  minute: number;
  label: string;
  days: number[];
  date: string | null;
  sound: AlarmSound;
  scene: string | null;
}

export type TimerChange = { type: 'pause' } | { type: 'resume' } | { type: 'add'; ms: number } | { type: 'cancel' };
export type RingAnswer = { type: 'stop' } | { type: 'snooze' } | { type: 'add'; ms: number };

/** TMDB calls need the app's backend; the plain-browser preview says so instead. */
const insideApp = <T,>(run: () => Promise<T>): Promise<T> =>
  backend() ? run() : Promise.reject('The catalog loads inside the TV Launcher app');

function call<T>(cmd: string, args?: Record<string, unknown>, sample?: () => T): Promise<T> {
  if (!backend()) return Promise.resolve(sample ? sample() : (undefined as T));
  return invoke<T>(cmd, args);
}

export const api = {
  listApps: () => call<AppEntry[]>('list_apps', undefined, () => SAMPLE_APPS),
  launchApp: (id: string, title?: string | null, backdrop?: string | null) =>
    call<void>('launch_app', { id, title: title ?? null, backdrop: backdrop ?? null }),
  /** Opens a web app at `url` instead of its start page. */
  launchAppAt: (id: string, url: string) => call<void>('launch_app_at', { id, url }),
  closeApp: () => call<void>('close_app'),
  status: () =>
    call<Status>('get_status', undefined, () => ({
      mode: 'launcher',
      running_app: null,
      running_game: null,
      pad: null,
    })),
  listGames: () => call<Library>('list_games', undefined, () => SAMPLE_LIBRARY),
  launchGame: (id: string) => call<void>('launch_game', { id }),
  toggleFavorite: (id: string) => call<boolean>('toggle_favorite', { id }, () => true),
  retro: () => call<Retro>('retro_get', undefined, retroSample.get),
  /** Plays a retro game; `fresh` starts it over instead of from its auto-save. */
  retroLaunch: (id: string, fresh = false) => call<void>('retro_launch', { id, fresh }),
  retroInstall: (emu: Emu) => call<void>('retro_install', { emu }, () => retroSample.install(emu)),
  retroCancel: (emu: Emu) => call<void>('retro_cancel', { emu }, () => retroSample.cancel(emu)),
  /** Unlinks the user's copy, or deletes the launcher's program files (saves stay). */
  retroRemove: (emu: Emu) => call<void>('retro_remove', { emu }, () => retroSample.remove(emu)),
  retroLink: (emu: Emu, path: string, moveSettings: boolean, folders: [string, string][]) =>
    call<void>('retro_link', { emu, path, moveSettings, folders }, () => retroSample.link(emu, path)),
  /** Sets consoles' game folders; null clears one. */
  retroSetFolders: (folders: [string, string | null][]) =>
    call<void>('retro_set_folders', { folders }, () => retroSample.setFolders(folders)),
  retroSetBios: (path: string) => call<void>('retro_set_bios', { path }),
  retroScanBios: () => call<void>('retro_scan_bios'),
  retroAddCores: () => call<void>('retro_add_cores'),
  retroCheckUpdates: () => call<void>('retro_check_updates', undefined, retroSample.checkUpdates),
  retroOpenBios: (emu: Emu) => call<void>('retro_open_bios', { emu }),
  /** A folder's folders; the drives with none. */
  retroBrowse: (path: string | null) => call<FolderListing>('retro_browse', { path }, () => retroSample.browse(path)),
  retroBrowseStart: (target: RetroTarget) =>
    call<string | null>('retro_browse_start', { target }, () => retroSample.browseStart(target)),
  retroCheck: (target: RetroTarget, path: string) =>
    call<FolderCheck>('retro_check', { target, path }, () => retroSample.check(target, path)),
  history: () => call<History>('get_history', undefined, () => ({ opened: [], searches: [] })),
  recordSearch: (query: string) => call<void>('record_search', { query }),
  /** Takes something opened from Search out of Continue watching. */
  forgetOpened: (appId: string, title: string | null) => call<void>('forget_opened', { appId, title }),
  /** Movies and series from `page` on; trending when `query` is empty. `mine`: only on the user's apps. */
  search: (query: string, page = 1, mine = false): Promise<SearchPage> =>
    backend()
      ? invoke<SearchPage>('tmdb_search', { query, page, mine })
      : Promise.reject('Search runs inside the TV Launcher app'),
  providers: (mediaType: Title['media_type'], id: number) =>
    call<string[]>('tmdb_providers', { mediaType, id }, () => []),
  /** Movies or series streaming on `service`, for one row of its home. */
  discover: (service: string, query: DiscoverQuery) =>
    insideApp(() => invoke<Title[]>('tmdb_discover', { service, query })),
  /** Search from `page` on, keeping what streams on `service`; its most popular when `query` is empty. */
  searchOn: (service: string, query: string, page = 1) =>
    insideApp(() => invoke<SearchPage>('tmdb_search_on', { service, query, page })),
  details: (mediaType: Title['media_type'], id: number) =>
    insideApp(() => invoke<TitleDetails>('tmdb_details', { mediaType, id })),
  season: (id: number, number: number) => insideApp(() => invoke<TvEpisode[]>('tmdb_season', { id, number })),
  person: (id: number) => insideApp(() => invoke<Person>('tmdb_person', { id })),
  /** Which of the user's apps stream each of a person's titles, keyed "movie:603" (none: left out). */
  personServices: (id: number) => insideApp(() => invoke<Record<string, string[]>>('tmdb_person_services', { id })),
  /** Plays a movie, or an episode (none given: wherever the user is), on the service's website. */
  watchPlay: (service: string, mediaType: Title['media_type'], id: number, season?: number | null, episode?: number | null) =>
    insideApp(() => invoke<void>('watch_play', { service, mediaType, id, season: season ?? null, episode: episode ?? null })),
  /** What the house's library holds. */
  libraryGet: () =>
    call<LibraryListing>('library_get', undefined, () => ({ configured: false, titles: [], free_bytes: null, problems: [] })),
  libraryTitle: (mediaType: Title['media_type'], id: number) => insideApp(() => invoke<LibraryTitle>('library_title', { mediaType, id })),
  /** Asks for a movie, or for a series' season or episode (none: all of it). */
  libraryRequest: (mediaType: Title['media_type'], id: number, season?: number | null, episode?: number | null) =>
    insideApp(() => invoke<LibraryTitle>('library_request', { mediaType, id, season: season ?? null, episode: episode ?? null })),
  /** Takes a title out of the library, its files too. */
  libraryRemove: (mediaType: Title['media_type'], id: number) => insideApp(() => invoke<void>('library_remove', { mediaType, id })),
  libraryDownloads: () => call<Downloads>('library_downloads', undefined, () => ({ items: [], speed: 0, problems: [] })),
  libraryDownloadControl: (hash: string, action: 'pause' | 'resume') => call<void>('library_download_control', { hash, action }),
  playerControl: (control: PlayerControl) => call<void>('player_control', { control }),
  playerState: () => invoke<PlayerState>('player_state'),
  /** Reads, in the background, what was watched on these services outside the launcher. */
  watchSync: (services: string[]) => call<void>('watch_sync', { services }),
  /** The service profile Play opens. */
  watchProfile: (service: string) =>
    call<WatchProfile>('watch_profile', { service }, () => ({ name: null, pickable: false })),
  /** Opens the service's "Who's watching?" for the user to pick the profile Play uses. */
  watchPickProfile: (service: string) => insideApp(() => invoke<void>('watch_pick_profile', { service })),
  /** Continue watching, most recent first: on `service`, or everywhere. */
  watchProgress: (service?: string | null) =>
    call<WatchProgress[]>('watch_progress', { service: service ?? null }, () => []),
  /** Takes a title out of Continue watching, until it is played again. */
  watchDismiss: (service: string, mediaType: Title['media_type'], id: number) =>
    call<void>('watch_dismiss', { service, mediaType, id }),
  watchTitle: (service: string, mediaType: Title['media_type'], id: number) =>
    call<TitleWatch>('watch_title', { service, mediaType, id }, () => ({ progress: null, listed: false })),
  /** My list on `service`, or on every service (null); newest first. */
  watchlist: (service: string | null) => call<WatchListItem[]>('watchlist', { service }, () => SAMPLE_WATCHLIST),
  /** Adds to My list or takes out; resolves true when it is now listed. */
  watchlistToggle: (item: Omit<WatchListItem, 'added_at'>) =>
    call<boolean>('watchlist_toggle', { item: { ...item, added_at: 0 } }, () => true),
  /** Recommendation rows for `service`'s home, or the launcher's Home (none). */
  recsShelves: (service?: string | null) =>
    call<RecShelf[]>('recs_shelves', { service: service ?? null }, () => SAMPLE_REC_SHELVES),
  recsRow: (service: string | null, shelf: RecShelf) =>
    call<RecTitle[]>('recs_row', { service, shelf }, () => sampleRecRow(shelf)),
  /** A row of a genre's page, on any of the user's services. */
  recsGenre: (genre: number, sort: GenreSort) =>
    call<RecTitle[]>('recs_genre', { genre, sort }, () => sampleRecRow({ ...SAMPLE_REC_SHELVES[0], key: sort })),
  ratingGet: (mediaType: Title['media_type'], id: number) =>
    call<Score | null>('rating_get', { mediaType, id }, () => null),
  /** Rates a title (null takes the rating away); resolves to the rating now. */
  ratingSet: (item: Pick<Title, 'media_type' | 'id' | 'title' | 'poster' | 'backdrop'>, score: Score | null) =>
    call<Score | null>(
      'rating_set',
      // The item's own score is only there for the backend's type; `score` is what's set.
      { item: { ...item, score: score ?? 'like', at: 0 }, score },
      () => score,
    ),
  getSettings: () =>
    call<Settings>('get_settings', undefined, () => ({
      tmdb_api_key: '',
      region: 'BR',
      language: 'pt-BR',
      browser: 'edge',
      home_button: 'guide',
      steam_api_key: '',
      steam_id: '',
      steamgriddb_api_key: '',
      youtube_api_key: '',
      ewelink_app_id: '',
      ewelink_app_secret: '',
      tuya_access_id: '',
      tuya_access_secret: '',
      tuya_region: '',
      deepinfra_api_key: '',
      assistant_model: '',
      assistant_speech: 'windows',
      assistant_voice: '',
      assistant_location: '',
      spotify_client_id: '',
      lastfm_api_key: '',
      twitch_client_id: '',
      google_client_id: '',
      google_client_secret: '',
      screensaver_minutes: 5,
      screensaver_seconds: 15,
      hide_cursor: true,
      calendar_reminders: true,
      home_address: '',
      library_on: false,
      radarr_url: '',
      radarr_api_key: '',
      sonarr_url: '',
      sonarr_api_key: '',
      qbittorrent_url: '',
      qbittorrent_username: '',
      qbittorrent_password: '',
      library_quality: '',
    })),
  saveSettings: (settings: Settings) => call<void>('save_settings', { settings }),
  reloadConfig: () => call<void>('reload_config'),
  linkStatus: () => call<LinkInfo>('link_status', undefined, sampleLink),
  linkAddress: (address: string | null) => call<void>('link_address', { address }),
  linkPairAgain: () => call<void>('link_pair_again'),
  getAutostart: () => call<boolean>('get_autostart', undefined, () => false),
  setAutostart: (enabled: boolean) => call<void>('set_autostart', { enabled }),
  openDataDir: () => call<void>('open_data_dir'),
  smartHome: () => call<SmartHome>('smarthome_get', undefined, sampleSnapshot),
  control: (id: string, command: DeviceCommand) =>
    call<void>('smarthome_control', { id, command }, () => controlSample(id, command)),
  watchSmartHome: (active: boolean) => call<void>('smarthome_watch', { active }),
  refreshSmartHome: () => call<void>('smarthome_refresh'),
  runScene: (id: string) => call<void>('smarthome_run_scene', { id }),
  saveScene: (id: string, name: string, icon: string) => call<void>('smarthome_save_scene', { id, name, icon }),
  putScene: (scene: Scene) => call<string>('smarthome_put_scene', { scene }, () => putSampleScene(scene)),
  deleteScene: (id: string) =>
    call<void>('smarthome_delete_scene', { id }, () => {
      SAMPLE_HOME.scenes = SAMPLE_HOME.scenes.filter((s) => s.id !== id);
    }),
  putAutomation: (automation: Automation) =>
    call<void>('smarthome_put_automation', { automation }, () => {
      const a = { ...automation, id: automation.id || `a${Date.now()}` };
      SAMPLE_HOME.automations = [...SAMPLE_HOME.automations.filter((x) => x.id !== a.id), a];
    }),
  deleteAutomation: (id: string) =>
    call<void>('smarthome_delete_automation', { id }, () => {
      SAMPLE_HOME.automations = SAMPLE_HOME.automations.filter((a) => a.id !== id);
    }),
  setLayout: (layout: HomeLayout) =>
    call<void>('smarthome_set_layout', { layout }, () => {
      SAMPLE_HOME.layout = structuredClone(layout);
    }),
  /** A room's All lights: the command goes to every light in it. */
  controlRoom: (room: string, command: DeviceCommand) =>
    call<void>('smarthome_control_room', { room, command }, () => controlSampleRoom(room, command)),
  /** Saves the room's devices as they are now as one of its routines (null: every device, for
   *  the whole house). Returns its id. */
  saveRoutine: (name: string, icon: string, room: string | null) =>
    call<string>('smarthome_save_scene', { id: '', name, icon, room }, () =>
      putSampleScene({ id: '', name, icon, theme: '', room, actions: [{ device: SAMPLE_HOME.devices[0].id, on: true }] }),
    ),
  addRoom: (name: string, icon: string) =>
    call<string>('smarthome_add_room', { name, icon }, () => {
      const L = SAMPLE_HOME.layout;
      if (!L.room_order.includes(name)) L.room_order.push(name);
      if (icon || L.room_icons[name] === undefined) L.room_icons[name] = icon;
      return name;
    }),
  renameRoom: (from: string, to: string) => call<void>('smarthome_rename_room', { from, to }, () => moveSampleRoom(from, to)),
  deleteRoom: (name: string) => call<void>('smarthome_delete_room', { name }, () => moveSampleRoom(name, null)),
  /** Adds a task to a room (null: the whole house), or retitles task `id`. */
  putTask: (id: string | null, room: string | null, title: string) =>
    call<string>('smarthome_put_task', { id, room, title }, () => {
      const t = SAMPLE_HOME.tasks.find((x) => x.id === id);
      if (t) Object.assign(t, { room, title });
      else SAMPLE_HOME.tasks.push({ id: `t${Date.now()}`, room, title, created_at: Date.now(), done_at: null });
      return t?.id ?? SAMPLE_HOME.tasks[SAMPLE_HOME.tasks.length - 1].id;
    }),
  taskDone: (id: string, done: boolean) =>
    call<void>('smarthome_task_done', { id, done }, () => {
      const t = SAMPLE_HOME.tasks.find((x) => x.id === id);
      if (t) t.done_at = done ? Date.now() : null;
    }),
  deleteTask: (id: string) =>
    call<void>('smarthome_delete_task', { id }, () => {
      SAMPLE_HOME.tasks = SAMPLE_HOME.tasks.filter((t) => t.id !== id);
    }),
  /** Renames a screen or moves it to a room (null: none); voice on it follows its room. */
  putScreen: (id: string, name: string, room: string | null) =>
    call<void>('smarthome_put_screen', { id, name, room }, () => {
      const x = SAMPLE_HOME.screens.find((s) => s.id === id);
      if (x) Object.assign(x, { name, room });
    }),
  forgetScreen: (id: string) =>
    call<void>('smarthome_forget_screen', { id }, () => {
      SAMPLE_HOME.screens = SAMPLE_HOME.screens.filter((s) => s.id !== id);
    }),
  ewelinkSignIn: () => call<void>('ewelink_sign_in'),
  ewelinkSignOut: () => call<void>('ewelink_sign_out'),
  spotify: () => call<SpotifySnapshot>('spotify_get', undefined, () => structuredClone(SAMPLE_SPOTIFY)),
  spotifySignIn: () => call<void>('spotify_sign_in'),
  spotifySignOut: () => call<void>('spotify_sign_out'),
  spotifyControl: (control: SpotifyControl) =>
    call<void>('spotify_control', { control }, () => controlSpotifySample(control)),
  /** Plays on this PC; a track plays inside `context` when given, an episode from `positionMs`. */
  spotifyPlay: (uri: string, context?: string | null, positionMs?: number) =>
    call<void>('spotify_play', { uri, context: context ?? null, positionMs: positionMs ?? null }),
  spotifyShelf: (kind: Shelf) => call<MusicItem[]>('spotify_shelf', { kind }, () => SAMPLE_SHELVES[kind]),
  spotifyPlaylist: (uri: string) => call<Playlist>('spotify_playlist', { uri }, () => samplePlaylist(uri)),
  spotifyArtist: (uri: string) => call<Artist>('spotify_artist', { uri }, () => sampleArtist(uri)),
  /** Everyone's most played songs of an artist, from Last.fm; null without a Last.fm key. */
  spotifyArtistPopular: (uri: string, name: string) =>
    call<TopTrack[] | null>('spotify_artist_popular', { uri, name }, samplePopular),
  /** Plays songs one after another on this PC. */
  spotifyPlayTracks: (uris: string[]) => call<void>('spotify_play_tracks', { uris }),
  spotifyShow: (uri: string) => call<Show>('spotify_show', { uri }, () => sampleShow(uri)),
  spotifyOldestEpisodes: (uri: string, total: number) =>
    call<Episode[]>('spotify_oldest_episodes', { uri, total }, () => sampleShow(uri).episodes.reverse()),
  /** Follows or unfollows an artist or podcast. */
  spotifyFollow: (uri: string, on: boolean) => call<void>('spotify_follow', { uri, on }),
  spotifySearch: (query: string) =>
    call<MusicSearch>('spotify_search', { query }, () => ({
      tracks: SAMPLE_SHELVES.albums.map((a, i) => sampleItem('track', `Song ${i + 1}`, a.subtitle, i)),
      artists: SAMPLE_SHELVES.artists,
      albums: SAMPLE_SHELVES.albums,
      playlists: [],
    })),
  photos: () =>
    call<PhotosSnapshot>('photos_get', undefined, () => ({
      albums: [],
      google: { configured: false, signed_in: false, connected: false, error: null },
      pick: null,
    })),
  /** Every photo of the albums switched on, for the screensaver. */
  photosSlideshow: () => call<Photo[]>('photos_slideshow', undefined, () => []),
  photosSignIn: () => call<void>('photos_google_sign_in'),
  photosSignOut: () => call<void>('photos_google_sign_out'),
  /** Shows a QR code to choose photos on the phone; resolves once they're saved. `album`: pick it again. */
  photosPick: (album?: string) =>
    backend()
      ? invoke<PhotoAlbum>('photos_google_pick', { album: album ?? null })
      : Promise.reject('Google Photos works inside the TV Launcher app'),
  /** Opens the pick in progress in the browser on this PC instead. */
  photosPickHere: () => call<void>('photos_pick_here'),
  photosCancelPick: () => call<void>('photos_cancel_pick'),
  photosAddFolder: (path: string) =>
    isTauri ? invoke<PhotoAlbum>('photos_add_folder', { path }) : Promise.reject('Folders work inside the TV Launcher app'),
  photosSetEnabled: (id: string, enabled: boolean) => call<void>('photos_set_enabled', { id, enabled }),
  photosRemove: (id: string) => call<void>('photos_remove', { id }),
  twitch: () => call<TwitchSnapshot>('twitch_get', undefined, () => structuredClone(SAMPLE_TWITCH)),
  twitchSignIn: () => call<void>('twitch_sign_in'),
  twitchSignOut: () => call<void>('twitch_sign_out'),
  twitchStreams: (source: TwitchSource, gameId?: string) =>
    call<TwitchStream[]>('twitch_streams', { source, gameId: gameId ?? null }, () =>
      source === 'game' ? SAMPLE_STREAMS.slice(0, 3) : SAMPLE_STREAMS,
    ),
  twitchCategories: () => call<TwitchCategory[]>('twitch_categories', undefined, () => SAMPLE_CATEGORIES),
  twitchSearch: (query: string, liveOnly: boolean) =>
    call<TwitchSearch>('twitch_search', { query, liveOnly }, () => searchTwitchSample(query, liveOnly)),
  twitchChannel: (login: string) =>
    call<TwitchChannelInfo>('twitch_channel', { login }, () => {
      const stream = SAMPLE_STREAMS.find((s) => s.login === login) ?? null;
      return { id: stream?.user_id ?? '0', login, name: stream?.name ?? login, avatar: null, stream, following: !!stream };
    }),
  twitchSendChat: (broadcasterId: string, message: string) => call<void>('twitch_send_chat', { broadcasterId, message }),
  /** A real Space key press, to whatever has the keyboard (the Twitch player's frame). */
  pressSpace: () => call<void>('press_space'),
  sportsHome: () => call<SportsHome>('sports_home', undefined, sampleSportsHome),
  /** One game with everything its screen shows; `game` is the card it was opened from. */
  sportsGame: (game: SportsGame) => call<SportsGameDetail>('sports_game', { game }, () => sampleGameDetail(game)),
  sportsCompetition: (id: string) => call<Competition>('sports_competition', { id }, () => sampleCompetition(id)),
  /** Opens a channel's app or page; with `game`, its score can show over the broadcast. */
  sportsWatch: (channel: SportsChannel, game?: SportsGame | null) =>
    call<void>('sports_watch', { channel, game: game ?? null }),
  sportsHighlights: (game: SportsGame) =>
    call<Highlights>('sports_highlights', { game }, () => sampleHighlights(game)),
  sportsTv: () => call<TvChannel[]>('sports_tv', undefined, sampleTv),
  sportsWatchTv: (key: string) => call<void>('sports_watch_tv', { key }),
  /** Resolves to whether the reminder is on now. */
  sportsRemind: (game: SportsGame, on: boolean) => call<boolean>('sports_remind', { game, on }, () => on),
  sportsSetOverlay: (on: boolean) => call<void>('sports_set_overlay', { on }),
  liveNow: () => call<LiveNow>('live_now', undefined, sampleLiveNow),
  liveTv: () => call<LiveTv>('live_tv', undefined, sampleLiveTv),
  /** Plays a live channel with the overlay, its rail holding the whole lineup. */
  liveWatch: (key: string) => call<void>('live_watch', { key }),
  /** Resolves to the favorites. */
  liveFavorite: (key: string, on: boolean, now: string[]) =>
    call<string[]>('live_favorite', { key, on }, () => (on ? [...now.filter((k) => k !== key), key] : now.filter((k) => k !== key))),
  /** A reminder 5 minutes before; resolves to whether it's on. */
  liveRemind: (key: string, start: number, on: boolean) => call<boolean>('live_remind', { key, start, on }, () => on),
  liveNews: () => call<NewsSource[]>('live_news', undefined, () => SAMPLE_NEWS),
  liveNewsSearch: (query: string) => call<NewsFound[]>('live_news_search', { query }, () => sampleNewsSearch(query)),
  /** Resolves to the news channels. */
  liveNewsAdd: (id: string, name: string) => call<NewsSource[]>('live_news_add', { id, name }, () => SAMPLE_NEWS),
  liveNewsRemove: (id: string) => call<NewsSource[]>('live_news_remove', { id }, () => SAMPLE_NEWS.filter((s) => s.id !== id)),
  youtube: () =>
    call<ProviderLink>('youtube_get', undefined, () => ({ configured: true, signed_in: false, connected: false, error: null })),
  /** Google's consent page for read-only YouTube; resolves once it redirected back. */
  youtubeSignIn: () => call<void>('youtube_sign_in'),
  youtubeSignOut: () => call<void>('youtube_sign_out'),
  /** Plays a video (a live stream) in the YouTube TV app. */
  youtubeWatch: (id: string) => call<void>('youtube_watch', { id }),
  /** Every calendar's events between `from` and `to` (unix ms), and those of the next days. */
  calendar: (from: number, to: number) => call<CalendarAgenda>('calendar_get', { from, to }, () => sampleAgenda(from, to)),
  /** Home's next event, within a week. */
  calendarNext: () => call<CalendarEvent | null>('calendar_next', undefined, sampleNextEvent),
  calendarLink: () =>
    call<ProviderLink>('calendar_link', undefined, () => ({ configured: true, signed_in: true, connected: true, error: null })),
  /** Google's consent page for read-only Calendar access; resolves once the calendars were read. */
  calendarSignIn: () => call<void>('calendar_sign_in'),
  calendarSignOut: () => call<void>('calendar_sign_out'),
  /** Shows or hides a calendar on the TV. */
  calendarShow: (id: string, on: boolean) => call<void>('calendar_show', { id, on }, () => sampleShowCalendar(id, on)),
  /** Resolves to whether the event's reminder is on now. */
  calendarRemind: (id: string, end: number, on: boolean) =>
    call<boolean>('calendar_remind', { id, end, on }, () => sampleRemind(id, on)),
  /** The drive from the home address in Settings; null without one, or when the place isn't found. */
  calendarTravel: (place: string) => call<Travel | null>('calendar_travel', { place }, () => sampleTravel(place)),
  /** A meeting's link as a QR code (inline SVG). */
  calendarQr: (text: string) => call<string>('calendar_qr', { text }, () => sampleQr(text)),
  /** The timers, the alarms and what's ringing; each change below resolves to all of it again. */
  clock: () => call<Clock>('clock_get', undefined, sampleClock.get),
  timerStart: (ms: number, label: string | null = null) =>
    call<Clock>('clock_timer_start', { ms, label }, () => sampleClock.start(ms, label)),
  timerChange: (id: string, change: TimerChange) =>
    call<Clock>('clock_timer', { id, change }, () => sampleClock.change(id, change)),
  /** Adds an alarm (no id) or changes one; it's turned on. */
  alarmSave: (alarm: AlarmEdit) => call<Clock>('clock_alarm_save', { alarm }, () => sampleClock.save(alarm)),
  alarmOn: (id: string, on: boolean) => call<Clock>('clock_alarm_on', { id, on }, () => sampleClock.setOn(id, on)),
  alarmDelete: (id: string) => call<Clock>('clock_alarm_delete', { id }, () => sampleClock.remove(id)),
  /** Stops, snoozes or adds time to what's ringing. */
  ringRespond: (id: string, answer: RingAnswer) =>
    call<Clock>('clock_respond', { id, answer }, () => sampleClock.respond(id, answer)),
  /** `from`: the companion screen it was said on (none: this TV), and the room it's about when
   *  the request names none, if not that screen's. */
  assistantAsk: (text: string, now: string, from?: { screen?: string | null; room?: string | null }) =>
    call<AssistantReply>('assistant_ask', { text, now, screen: from?.screen ?? null, room: from?.room ?? null }, () => ({
      text: 'The assistant runs inside the TV Launcher app.',
      cards: [],
      media: [],
      ui: [],
    })),
  /** Resolves with what was said once the speaker goes quiet; null when nothing was heard. The
   *  launcher PC's microphone, from the companion too. */
  assistantListen: () =>
    backend() ? invoke<string | null>('assistant_listen') : Promise.reject('The microphone works inside the TV Launcher app'),
  assistantListenStop: () => call<void>('assistant_listen_stop'),
  /** Saves just the spoken-replies choice, right away. */
  assistantSetSpeech: (speech: Speech) => call<void>('assistant_set_speech', { speech }),
  assistantListenCancel: () => call<void>('assistant_listen_cancel'),
  /** Reads the reply out on the PC's speakers; resolves once it has been said or hushed.
   *  "voice-start" (with `id`) comes when the sound starts. */
  assistantSpeak: (text: string, id: number) =>
    isTauri ? invoke<void>('assistant_speak', { text, id }) : Promise.reject('The voice works inside the TV Launcher app'),
  assistantHush: () => call<void>('assistant_hush'),
  /** Reads a line out in `voice` (empty: the one in Settings). */
  assistantVoiceTry: (voice: string) => call<void>('assistant_voice_try', { voice }),
  assistantVoiceModel: () =>
    call<VoiceModel>('assistant_voice_model', undefined, () => ({ state: 'ready', done: 0, total: 0, error: null })),
  /** Loads the voice, or downloads it first ("voice-model" events). */
  assistantVoicePrepare: () => call<void>('assistant_voice_prepare'),
};

/** Subscribes to a backend event; returns the unsubscribe function. */
export function on<T>(event: string, cb: (payload: T) => void): () => void {
  if (transport) return transport.listen(event, cb);
  if (!isTauri) return () => {};
  let unlisten: (() => void) | undefined;
  let gone = false;
  listen<T>(event, (e) => cb(e.payload)).then((u) => (gone ? u() : (unlisten = u)));
  return () => {
    gone = true;
    unlisten?.();
  };
}

export const fileSrc = (path: string) => (transport ? transport.fileSrc(path) : isTauri ? convertFileSrc(path) : path);
