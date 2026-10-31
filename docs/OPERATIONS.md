# Running the home server

The server runs on the old laptop under Ubuntu Server, as a systemd service, from boot, with nobody
signed in. These steps set it up; you do them once.

## 1. Prepare the laptop

1. **Install Ubuntu Server** (latest LTS) with the host name `server`, and turn OpenSSH on in the
   installer. iPhones, iPads and Windows PCs then reach it at `server.local`; Android phones may
   need its IP address. If the installer finds no disk, set the BIOS's SATA Operation to AHCI:
   Intel RST hides the drive from Linux.
2. **Fixed address**: in the Deco app, reserve the laptop's IP (Network › the laptop › Reserve
   IP). Bookmarks, home-screen icons and the HTTPS certificates are tied to it. A cable is better
   than Wi-Fi if the router is near.

## 2. A build

Download the `tvl-server` artifact from the repo's Actions tab (any run of Deploy server) and unzip
it: `tvl-server` and `companion/`. Or build on any Linux PC:

```bash
CARGO_TARGET_DIR=target cargo build --release -p tvl-server
npm install && npm run companion:build
```

and put `target/release/tvl-server` and `apps/companion/dist`, renamed `companion`, in one folder.

## 3. Set it up

Needs a backup of the old server: `tvl-server/`, its data folder.

```bash
sudo scripts/setup-server.sh <backup folder> <build folder>
sudo -u tvl-server /opt/tvl-server/tvl-server pin
```

`setup-server.sh` sets the time zone and clock (alarms depend on them), keeps the laptop running
with the lid closed, then installs the hub in `/opt/tvl-server` as the service `tvl-server`, under
its own account, with the data in `/var/lib/tvl-server` (nobody else can read the keys there). Data
already there is kept. `pin` shows the admin PIN: phones and tablets need it for settings and
sign-ins; paired TV PCs show it too.

Keeping the host name, the address and the data folder keeps everything that knows the server: the
home certificate authority (iPads that trust it need nothing new), bookmarks and paired TV PCs.

Then Tailscale, which must never replace the local DNS:

```bash
curl -fsSL https://tailscale.com/install.sh | sh
sudo tailscale up --accept-dns=false
```

Check: on the laptop, `curl -I http://localhost:47960`; on a phone, `http://server.local:47960`.

## Day to day

- **Status**: `systemctl status tvl-server`.
- **Logs**: `/var/lib/tvl-server/logs/server.<date>.log` (as root), two weeks kept;
  `journalctl -u tvl-server` for what happened before logging started. More detail: add
  `Environment=TVL_LOG=debug` with `sudo systemctl edit tvl-server` and restart it.
- **Restart**: `sudo systemctl restart tvl-server`.
- **Settings without a PIN**: `http://localhost:47960` on the laptop itself counts as admin.
- **Backups**: copy `/var/lib/tvl-server` somewhere safe and private: it holds the household's keys,
  the devices' local keys and the home CA's key.
- **Update**: pushes to main deploy by themselves (below). By hand:
  `sudo -u tvl-runner scripts/deploy-server.sh <folder>`.
- **Remove**: `sudo systemctl disable --now tvl-server`; the data folder stays.

## The library (Radarr, Sonarr, qBittorrent)

Optional: without it the Library tile just doesn't show. All of this runs on the laptop, next to the
hub; only the player (mpv) runs on the TV PCs, and it fetches itself the first time something plays.

1. **A disk for the media.** The media lives on `/mnt/dados`, the internal 880 GB SSD (ext4, mounted
   from `/etc/fstab`): a 1080p film is 2 to 15 GB, a 4K one up to 60 GB. The system disk is too small,
   and an exFAT USB drive won't do: no hardlinks, so every finished title is copied instead of linked,
   and no permissions for the hub to read them through. Keep `downloads` on the same disk as the
   library for the same reason.
2. **Install the services** (*sudo*). From a checkout of this repo:
   ```bash
   sudo scripts/setup-media.sh
   ```
   It creates `/mnt/dados/media` with `movies`, `series` and `downloads`, installs Radarr, Sonarr,
   Prowlarr and qBittorrent as services under the `media` account, adds the hub's account to that
   group so it can hand the files to the TVs (restarting the hub once), and prints their addresses
   and API keys. Their data is in `/var/lib/<name>`: back it up with the hub's.
3. **Sign in to each one.** Radarr, Sonarr and Prowlarr ask for a username and password on the first
   visit. qBittorrent's first password is temporary, and changes at every restart until you set your
   own (Tools › Options › Web UI); the script prints how to find it.
4. **qBittorrent in Radarr and Sonarr.** Settings › Download Clients, add qBittorrent at `localhost`,
   port `8080`, with the username and password left empty: it trusts its own machine. Use the category
   `radarr` in Radarr and `sonarr` in Sonarr, and leave Remove Completed on.
5. **Root folders.** In Radarr add `/mnt/dados/media/movies`, in Sonarr `/mnt/dados/media/series`. Keep
   the quality profile the hub uses (Settings › Movies & TV, by default `HD-1080p`): "Any" takes
   whatever turns up, however big.
6. **Prowlarr** holds the indexers and feeds the other two. Add the indexers under Indexers, then
   Radarr and Sonarr under Settings › Apps, at `http://localhost:7878` and `http://localhost:8989`
   with the keys the script printed.
7. **Connect them**: on a TV PC, Settings › Movies & TV › Library, enter the addresses and the API
   keys. Addresses are as the server reaches them, so `http://localhost:7878`, `http://localhost:8989`
   and `http://localhost:8080`, with qBittorrent's username and password left empty. The keys stay on
   the server. The tile appears. qBittorrent's address is what lets removing a title free the disk.

Optional: **Bazarr** fetches subtitles and leaves them next to the files, where the player finds
them; pt-BR subtitles rarely come with the file.

The services are `radarr`, `sonarr`, `prowlarr` and `qbittorrent`: `journalctl -u radarr -f` for
their logs.

**Space.** Nothing goes by itself: a title stays until it's removed, by holding A on the trash button
on its page in the Library on a TV PC. That takes the whole title, a series included, with its files
and torrents, and the space comes back at once.

A finished file has two names on the disk: one in the library, one in `downloads/`, which
qBittorrent seeds from. Both have to go before the space is free. Seeding stops when a title has
uploaded its own size or after a day (Tools › Options › BitTorrent); Radarr and Sonarr then clear the
torrent and leave the library's copy. Removing a title sooner clears it straight away.

The laptop has 8 GB of memory and Wi-Fi: Radarr, Sonarr, Prowlarr and qBittorrent take about 1 GB
between them, and a cable to the router helps the film as much as the rest of the house.

## Deploys on push

`.github/workflows/deploy-server.yml` runs on each push to main that touches `crates/`,
`apps/companion/`, the lock files or the deploy itself (or by hand: Actions › Deploy server › Run
workflow). GitHub tests the crates and builds the server and the companion; then the laptop, a
self-hosted runner labelled `home-hub`, runs `scripts/deploy-server.sh`: stop the service, keep the
running build in `/opt/tvl-server/previous`, copy the new one in, start it, and check that
`http://localhost:47960` answers. If it doesn't within a minute, the previous build goes back and
the run fails. `/opt/tvl-server/deployed.txt` holds the commit that's running.

The household's data never goes through GitHub: it stays in `/var/lib/tvl-server`.

Set the laptop up once, after step 3, with a token from the repo's Settings › Actions › Runners ›
New self-hosted runner:

```bash
sudo scripts/setup-deploy-runner.sh https://github.com/GabrielSiek/home-hub <token>
```

It gives the runner its own account, `tvl-runner`, which owns `/opt/tvl-server` and may stop and
start `tvl-server` (it still can't read the data folder), and registers the runner in
`/opt/actions-runner` as a systemd service. Check it under the repo's Settings › Actions › Runners.
Since the runner acts on the laptop, deploys only run for pushes to main, never for pull requests.

## The TV app's installers

`.github/workflows/build-tv.yml` runs on each push to main that touches `apps/tv/`, the crates it
shares with the server or the lock files (or by hand: Actions › Build TV app › Run workflow). One of
GitHub's Windows machines tests the app and builds its installers; no PC of ours is involved.

To update a TV PC, open the run in the repo's Actions tab, download `tvl-tv-windows` (it needs a
GitHub sign-in), unzip it and run the `.exe` from `nsis`. The installers aren't signed, so Windows
warns the first time: More info › Run anyway. The app keeps its data and its pairing.

## Next to the old launcher, on the dev PC

While testing, the old launcher can keep running:

```powershell
cargo run -p tvl-server -- run --dev --shadow
```

`--dev` uses ports 47970–47972 and `%LOCALAPPDATA%\TVLauncher\server-dev`. `--shadow` leaves the
smart-home LAN, automations, ringing and calendar cards to the old launcher: Tuya devices accept very
few LAN clients, and the two would ring every alarm twice. Even without `--shadow`, the server
stays off the LAN while the old launcher answers on 47960, unless `--force-lan`.
