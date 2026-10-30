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

Needs a backup of the old server: `tvl-server/`, its data folder, and `adguard/`, AdGuard Home's.

```bash
sudo scripts/setup-server.sh <backup folder> <build folder>
sudo -u tvl-server /opt/tvl-server/tvl-server pin
```

`setup-server.sh` sets the time zone and clock (alarms depend on them), keeps the laptop running
with the lid closed, frees port 53, installs AdGuard Home with its old settings, then the hub in
`/opt/tvl-server` as the service `tvl-server`, under its own account, with the data in
`/var/lib/tvl-server` (nobody else can read the keys there). Data already there is kept. `pin` shows
the admin PIN: phones and tablets need it for settings and sign-ins; paired TV PCs show it too.

Keeping the host name, the address and the data folder keeps everything that knows the server: the
home certificate authority (iPads that trust it need nothing new), bookmarks and paired TV PCs.

Then Tailscale, which must never replace the local DNS:

```bash
curl -fsSL https://tailscale.com/install.sh | sh
sudo tailscale up --accept-dns=false
```

Check: on the laptop, `curl -I http://localhost:47960`; on a phone, `http://server.local:47960`.

## Day to day

- **Status**: `systemctl status tvl-server AdGuardHome`.
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

## Next to the old launcher, on the dev PC

While testing, the old launcher can keep running:

```powershell
cargo run -p tvl-server -- run --dev --shadow
```

`--dev` uses ports 47970–47972 and `%LOCALAPPDATA%\TVLauncher\server-dev`. `--shadow` leaves the
smart-home LAN, automations, ringing and calendar cards to the old launcher: Tuya devices accept very
few LAN clients, and the two would ring every alarm twice. Even without `--shadow`, the server
stays off the LAN while the old launcher answers on 47960, unless `--force-lan`.
