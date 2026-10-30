# Architecture: home server + TV PC clients

The launcher used to be one Tauri app on the TV PC that also ran the household's backend. Here it
splits into:

- **The server** (`crates/server`, `tvl-server`): a systemd service on an always-on old laptop.
  It keeps the household's state and integrations, serves the companion, and **passes commands
  between devices**. It never plays video or audio; the library's files only pass through it.
- **The TV app** (`apps/tv`): the desktop app on each TV PC. Its Rust side is the **agent**: it
  keeps everything tied to that PC (the pad, Edge and its streaming sign-ins, games, emulators,
  volume, microphone, voice, the alarm chime) and connects to the server.
- **The companion** (`apps/companion`): the touch UI that phones, tablets and laptops open in a
  browser from the server. Each screen follows the TV PC of its room.

```
      ┌────────────── Home server: old laptop, Ubuntu service, no video/audio ───────────────┐
      │ tvl-server                                                                           │
      │  household  smart home (LAN) · scenes/automations · rooms/tasks · timers/alarms      │
      │             calendar · assistant (LLM, tools, Whisper) · hub · settings · tokens     │
      │  hub        devices · room → TV PC · command router · event bus · admin/pairing      │
      │  web        companion UI · /api/<cmd> · /api/events (WS) · HTTPS via the home CA     │
      └──────▲──────────────────────────────────────────────────────▲────────────────────────┘
   /api/agent│ wss: calls both ways + events          HTTP(S) + WS   │
 ┌───────────┴─────────────────────────┐              ┌──────────────┴───────────────────────┐
 │ TV PC: desktop app (agent + TV UI)  │◀── keys, ────│ iPad / phone / laptop (companion)    │
 │ pad · Edge + DevTools · streaming   │  play, vol   │ follows its room's TV PC: now        │
 │ logins · games · emulators · mic ·  │  (via the    │ playing, remote, volume, music,      │
 │ Kokoro voice · chime · volume ·     │   server)    │ ringing, play-on-TV                  │
 │ media modules (until M2)            │              └──────────────────────────────────────┘
 └─────────────────────────────────────┘
```

## Who owns what

| Area | Server | TV PC (agent) | Companion |
|---|---|---|---|
| Smart home, scenes, automations, layout, rooms, tasks | owns it; the **only** machine talking to devices (Tuya allows few LAN clients) | – | UI |
| Timers & alarms | schedule, state, scenes, chips; picks which devices ring | ring effects (chime, speakers, Spotify, overlay over video); pad answers; fail-safe ringing | ring screen, answer |
| Calendar | Google sync, reminders, travel times, Grêmio rows (cached) | reminder card over apps; Grêmio feed until M2 | Today |
| Assistant | LLM, tools, quick parser, Whisper, one conversation per screen | microphone, Kokoro voice, device tools | own mic (HTTPS), text replies |
| Settings | household, room and per-device settings; secrets masked | caches its own settings | admin behind the PIN |
| Sign-ins | PKCE, code exchange, token storage, single-flight refresh | opens the consent page in its Edge, catches the 127.0.0.1 redirect | paste fallback |
| Streaming sign-ins, watch sessions, profile links | M2: progress per device | **stay on the PC** | – |
| TMDB, recommendations, sports, live TV, Spotify, Twitch, YouTube, photos | M2 | as before, until M2 | through the router |
| Games, emulators | – | stay on the PC | – |
| The library (movies and series of our own) | talks to Radarr, Sonarr and qBittorrent, holds their keys, hands out the files | plays them (mpv), keeps progress | – for now |

## Devices, rooms and following

- Every device says hello with an id, name, kind (`tv-pc`, `tablet`, `phone`, `computer`), room,
  capabilities, protocol version and, for TV PCs, its MAC address. The old fixed `"tv"` screen
  becomes one real device per TV PC.
- A room's **TV PC** is the agent placed in that room; a room with two picks one.
- **Which TV PC a screen controls**, in order:
  1. the screen's explicit pick (kept on the server);
  2. its room's TV PC. If that PC is off, the screen says so ("Quarto TV is off"), offering
     Wake-on-LAN when available and the picker. It never switches to another room's PC by surprise;
  3. for a screen in no room, the only registered TV PC;
  4. otherwise the picker (HTTP 409).
- **What follows the room**: the "On the TV" card and remote header, remote keys and text, volume,
  TV off, play-on-TV, where music plays, which timers and alarms ring there, and the room the
  assistant acts on.
- **Settings scopes**:
  - household: secrets, region, language, home address, calendar reminders, assistant model and
    location;
  - room: its TV PC, and where things ring when it's off;
  - device: browser and its path, home button, Steam id, screensaver, cursor, voice, apps.

  Device settings live on the server and are cached by the agent. Streaming sign-ins never leave
  the PC.

## Command routing

`crates/protocol` declares every command once (it replaces the four hand-kept lists the old app
had). Each entry has:

| Field | Values |
|---|---|
| owner | `Server`, `Device`, or `Composite` (runs on the server and calls a device, or merges both: sign-ins, settings, assistant tools, the calendar card) |
| access | `Public`, `Admin`, or `TvOnly` (the TV PC's own UI only, e.g. `retro_remove`, `open_data_dir`, `set_autostart`) |
| target | `Caller`, `Room`, `Explicit` |
| behaviour | `idempotent`, `ordered`, `timeout_ms` |

- **Browsers** may call server-owned `Public` commands and the device commands the companion
  could already call. Unknown names get a 404.
- **Device calls aren't re-parsed on the server**: `{cmd, args}` goes untouched to the target
  agent, whose executor (the old `companion/api.rs` dispatch) parses the camelCase arguments.
- **The TV UI** keeps calling `invoke(...)`. The agent's invoke handler forwards server-owned
  commands with the caller attached; the rest run locally. Server events are re-emitted under the
  same names, so `on(...)` doesn't change.
- **Server unreachable**: the first call after boot waits up to 2 s for the link, then fails fast
  with "Server offline". Household reads are served from the agent's last copy, marked stale. On
  every reconnect the server pushes all household snapshots again.
- **Versions** are checked at hello; a mismatch shows "Update this TV PC".

## Agent link

- `GET /api/agent` over **wss**; the agent pins the home CA it received at pairing; each device has
  its own 256-bit token, of which the server keeps only a hash.
- Frames: `hello` / `welcome`, `call` / `result` in both directions, `event`.
- Every incoming frame is handled in its own task, replies are matched by id, and no lock is held
  across an `.await` (the TV asks the assistant while the server calls a tool back on that TV).
- The newest session of a device wins. Ping every 15 s; dead after 2 misses; online/offline changes
  wait about 5 s so screens don't flicker.
- Keys, text, play and launch are never retried; reads may retry once. Remote keys go through one
  ordered queue per TV PC; for a volume drag the latest value wins.
- Each subscriber gets each topic coalesced to the latest every ~100 ms (the iOS 12 iPad); a
  lagging socket gets `resync`. The topics agents send and the ones they receive never overlap.
- Pairing: the TV shows a 6-digit code and the CA fingerprint; it's approved from a phone holding
  an admin session, or on `http://localhost:47960` on the server laptop. `tvl-server pin` prints
  or resets the PIN.
- Discovery: a UDP broadcast probe (port 47962), or an address typed in TV Settings.

### Event topics

| Topic | From | To |
|---|---|---|
| `smarthome`, `companion`, `clock`, `calendar`, `devices` | server | everyone |
| `follow`, `resync` | server | one subscriber |
| `settings` (unmasked subset) | server | agents only |
| `mode`, `tv`, `watch`, `watch-sync`, `spotify`, `mic-ready` | agent | the screens following it |
| `device.context`, `ring.note`, `feed.sports` | agent | server only |
| `input`, `cursor`, `pad`, `games-*`, `retro-*`, `voice-*`, `twitch`, `youtube`, `photos`, `sports-*` | agent | its own webview only |

## The library

For what the streaming services don't have. It's one more service to the TV app, id `library`: a
Home tile, the same details screen, curtain, progress, Continue watching and My list. Only the last
step of playing differs. Its tile shows once the server has a Radarr or Sonarr key (`library_on`).

```
  TV PC                              Server                        On the server too
  SvcTitle: Download ──library_request──▶ library/                ──▶ Radarr, Sonarr ──▶ qBittorrent
  SvcHome, Downloads ──library_get, _downloads──▶ (keys stay here) ◀── what's there, the queues
  Play ──library_stream──▶ a token for the file
  mpv ◀──GET /api/media/<token>, Range── the file, over plain HTTP
```

- **Finding titles**: everything goes by TMDB id, as in the rest of the hub. Sonarr goes by TVDB, so
  the server asks TMDB for a series' TVDB id. A series is added with nothing monitored, and only the
  episodes asked for are fetched: one, a season, or all of it. Radarr and Sonarr own the files and
  their names; the hub never moves or renames anything. Removing a title deletes its files (admin).
- **Downloads**: Radarr and Sonarr drive qBittorrent. The hub reads their queues, and qBittorrent
  directly for speeds and to pause. Without qBittorrent's address it still works, without those.
- **Serving**: `GET /api/media/<token>`. A token opens one file for 12 hours and is only made for a
  TV PC's `library_stream`, for a file inside Radarr's or Sonarr's root folders. Seeking is plain
  Range requests. Subtitles next to the file ("<name>.pt-BR.srt", as Bazarr leaves them) get tokens
  too. The server doesn't transcode: this laptop couldn't for long, and mpv doesn't need it.
- **Playing**: mpv on the TV PC, fetched the first time something plays into the app's data folder.
  A browser can't play most of these files (MKV, HEVC, DTS, image subtitles), and would need the
  server to transcode.
- **The player's window**: mpv runs behind the launcher's window, which is transparent. While it
  plays (`Mode::Player`) the page's background goes away, and `Player.tsx` draws the controls as an
  ordinary screen. So the pad, the mouse, the assistant, toasts and the ring screen need nothing of
  their own there. The running app is mpv as far as the launcher goes: Home, a game or a service
  starting, and mpv exiting all take the paths a browser does.
- **Progress** is the same `Session` and store as the services', fed by mpv's position every 250 ms.

## Timers and alarms across devices

- `Timer` and `Alarm` carry a target: a room, a device, or everywhere. By default it's the
  caller's room; for a phone in no room, its TV PC's room.
- The online TV PCs in that room ring. If none is on, every online TV PC rings with a note
  ("Quarto TV is off, ringing in Sala"); if no TV PC is on, only open companion screens show it.
- The server sends each ringer the **full desired state** (`ring.set {generation, rings}`) on
  every change and every reconnect, so a lost "stop" can't leave a bell going.
- Effects run on the agent: chime, speakers up, Spotify paused or the alarm's playlist, the overlay
  over a playing video. Anyone can answer; the first answer wins.
- **Fail-safe**: each agent keeps a copy of what targets it (absolute times). If the server is
  down when one is due, the agent rings by itself and reports it later, so the server doesn't ring
  it again.

## Sign-ins with a headless server

- The server makes the PKCE verifier, state and consent URL. The caller's TV PC (or the room's)
  opens it in its Edge, listens on the registered loopback port (eWeLink 47950, Spotify 47951,
  Google 47952) and hands back `{code, state}`. The server exchanges and stores the token.
- Fallback: paste the failed `127.0.0.1` address into the companion (eWeLink's code lives only 30
  seconds, so it prefers the TV path).
- Twitch's device code works from anywhere.

## Security (home Wi-Fi trusted + admin PIN)

- Host allowlist (server IPs, `<name>.local`, `localhost`, configured names) against DNS rebinding.
- Origin must equal Host on POST and WebSocket upgrades; POST must be `application/json`
  (`audio/wav` for transcription); body and rate limits.
- Admin (settings, sign-ins, pairing and removing devices): a PIN gives a token in an
  `X-TVL-Admin` header, never a cookie, so cross-site requests can't carry it. Paired TV PCs and
  the server's own `localhost` count as admin. Five wrong PINs in a minute lock out. The PIN is
  kept readable in the locked-down data folder, so paired TV PCs can show it.
- Secrets reach browsers masked; agents get only the subset their media modules need.
- Agents cap what forwarded calls can do: `launch_app_at` only https on that app's host,
  `remote_text` ≤ 500 characters, `oauth.loopback` only to allowlisted URLs, `/api/file` only images
  from icons, art and photos.

## The service

- One binary: `tvl-server run` (what systemd starts) · `import` · `pin`. Built for Windows it also
  has `install` and `uninstall`, for a Windows service.
- Runs as the system account `tvl-server`, from `scripts/tvl-server.service`, restarted on failure;
  data in `/var/lib/tvl-server`, readable only by that account and root. It listens on TCP
  47960/47961, UDP 47962 and UDP 6666/6667/7000; Ubuntu's firewall is off by default, so nothing
  needs opening.
- It never touches a window, browser, audio device, key press, OneDrive or its working folder:
  anything that needs those goes through an agent.

## Milestones

| | Scope | Status |
|---|---|---|
| M0 | Workspace, untouched check, findings | done |
| M1a | kit, protocol, server core (security, TLS, devices, admin and pairing, TV PC sessions, router, event relay), import, smart home and Hub on the server, Windows service | server done |
| M1b | Clock and calendar across devices, sign-ins through a TV PC | server done |
| M1c | Assistant (per-screen conversations, TV tools forwarded) and household settings | server done |
| M1 TV side | The TV app as an agent (link, invoke router, ring effects, calendar card, sign-in loopback, assistant tools, device context, Grêmio feed), the TV UI's Server settings, the companion following its room's TV PC | built and tested; the manual run on the real house is next |
| M2 | Media data on the server, one domain at a time; playback stays on the TV PCs | |
| M3 | Viewer profiles, shared front-end package, installers, cutover | |

**Order of work.** The server's domains (smart home, clock, calendar, assistant) were all ported
before the TV app, rather than the TV app right after the smart home as first planned. That way the
TV app's copy drops every household module in one pass, instead of being rewired twice (its clock
and assistant would otherwise have had to reach the smart home remotely for a while).

**What changed from the plan while building it.**
- The eWeLink and Tuya cloud providers aren't ported (not even behind a feature): no keys were set
  and every device answers on the LAN.
- The admin PIN is kept readable in the locked-down data folder, not hashed: paired TVs show it.
- Lint is Biome rather than ESLint: typescript-eslint doesn't support TypeScript 7.
- Grêmio's matches reach the server as a `feed.sports` event from a TV PC (kept in
  gremio-feed.json), until sports moves to the server in M2.
- The link client is its own crate (`crates/agent-link`), tested against the real server: pairing,
  coming back with the token, calls and events both ways, `wss://` pinned to the home CA, the probe.
  `welcome` carries the HTTPS port, so a TV PC paired over `ws://` uses `wss://` from then on.
- A TV PC's microphone is transcribed on the server (`assistant_transcribe`, TV PCs only): the
  DeepInfra key never leaves the server.
- The TV's Settings lost its Smart Home tab (only the cloud keys lived there) and gained a Home
  server tab: status, the pairing code, the admin PIN, the server's address, pair again.
- With the old launcher running, the TV app starts passive by itself (windowed, no pad, Guide
  button or pointer); `TVL_PASSIVE=1` does the same on purpose.
- Lint: the TV copy's 88 hook dependency lists are warnings, not errors. They're the old app's,
  which ESLint never checked; changing them changes behaviour, so each needs a look of its own.

The detailed plan, with file-by-file steps and verification, is
`C:\Users\gabri\.claude\plans\we-have-the-idea-deep-fern.md`.
