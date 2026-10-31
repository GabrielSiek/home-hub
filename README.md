# TV Launcher, split into a home server and TV PC clients

Everything new lives here. The existing launcher (`../src-tauri`, `../src`, `../companion`) is the
read-only source this is copied from, and its data folder (`%APPDATA%\dev.tvlauncher.app`) is only
ever read. `docs/ARCHITECTURE.md` explains the design; `docs/FINDINGS.md` lists what the audit of
the old code found and where each item is fixed.

## Layout

| Path | What |
|---|---|
| `crates/kit` | Shared plumbing: errors, safe JSON files, HTTP, OAuth, time, logging, supervised tasks |
| `crates/protocol` | What server, TV PCs and browsers say to each other: the command table, frames, shared types |
| `crates/agent-link` | The TV PC's connection to the server, without Tauri |
| `crates/server` | `tvl-server`: the home server, a systemd service on Ubuntu |
| `apps/tv` | The desktop app for TV PCs (TV UI + agent), and its video player (mpv) |
| `apps/companion` | The touch UI for phones and tablets, served by the server |
| `scripts/` | `check-untouched.ps1` (nothing outside `refactor\` changed), `test-names-diff.ps1` (old tests carried over), `setup-server.sh` (a new server, from a backup), `setup-media.sh` (the library's Radarr, Sonarr, Prowlarr and qBittorrent), `deploy-server.sh` / `setup-deploy-runner.sh` (deploys on push to main) |

## Build and check

```powershell
cargo test                      # the crates (the TV app is left out: it needs Tauri + sherpa-onnx)
cargo test -p tvl-tv            # the TV app
cargo run -p tvl-server -- run --dev --shadow   # the server in a console, next to the old launcher
npm install                     # front ends
npm run typecheck; npm run lint
npm run tv:build; npm run companion:build
cd apps\tv; npx tauri dev       # the TV app, against the dev server (Vite on 1421)
powershell -NoProfile -File scripts\check-untouched.ps1
powershell -NoProfile -File scripts\test-names-diff.ps1
```

`docs/OPERATIONS.md` sets up the old laptop as the server.

Rust builds go to `C:\TVLauncher\build\refactor` (`.cargo/config.toml`), outside OneDrive.

## Next to the old launcher on one PC

- Ports: the old launcher uses 47960/47961 (and Vite 1420, companion dev 5174). The new server's
  dev ports are 47970/47971 (probe 47972), with data in `%LOCALAPPDATA%\TVLauncher\server-dev`.
- Run the server with `--shadow` while the old launcher runs: it then leaves the smart-home LAN,
  automations, ringing and calendar cards alone. Tuya devices accept very few LAN clients, so the
  old launcher and the new server must never both control them.
- The new TV app (`dev.tvlauncher.tv`, its own data folder) runs passive next to the old one by
  itself: windowed, no pad, no Game Bar or cursor changes. `TVL_PASSIVE=1` forces it. A debug
  build looks for the dev server (probe 47972) first. It shows a pairing code in Settings › Home
  server; approve it with `pair_approve` from an admin page, or on `http://localhost:47970`.
- The sign-in ports 47950-47952 are fixed (registered redirect URLs): never sign in to the same
  provider in both apps at once.
