# Running the home server

The server runs on the old laptop as a Windows service, from boot, with nobody signed in. These
steps set it up; you do them once. Commands run in a PowerShell window; the ones marked *admin*
need "Run as administrator".

## 1. Prepare the laptop

1. **Name it `tvl`** (Settings › System › About › Rename this PC), then restart. iPhones, iPads
   and Windows PCs then reach it at `tvl.local`; Android phones may need its IP address.
2. **Home network = Private** (Settings › Network › your Wi-Fi or Ethernet › Network profile type).
   The firewall rule the install adds is for Private networks only.
3. **Never sleep** while plugged in, and keep running with the lid closed (*admin*):
   ```powershell
   powercfg /change standby-timeout-ac 0
   powercfg /change hibernate-timeout-ac 0
   powercfg /setacvalueindex SCHEME_CURRENT SUB_BUTTONS LIDACTION 0
   powercfg /setactive SCHEME_CURRENT
   ```
4. **Fixed address**: in the Deco app, reserve the laptop's IP (Network › the laptop › Reserve
   IP). Bookmarks, home-screen icons and the HTTPS certificates are tied to it. A cable is better
   than Wi-Fi if the router is near.
5. Windows keeps its clock in sync by itself (alarms depend on it); leave "Set time automatically"
   on.

## 2. Build it (on the dev PC)

```powershell
cd refactor
cargo build --release -p tvl-server
npm install; npm run companion:build
```

Copy to the laptop, e.g. into `C:\Program Files\TVLauncher\`:
- `C:\TVLauncher\build\refactor\release\tvl-server.exe`
- the folder `refactor\apps\companion\dist`, renamed `companion`, next to the exe.

## 3. Bring the household over

The old launcher's data is on the TV PC. Copy `tvl-server.exe` there too and, with the old
launcher **closed**, run on the TV PC:

```powershell
tvl-server import --env "<repo>\.env" --out bundle.zip
```

It reads `%APPDATA%\dev.tvlauncher.app` (never changing it) and packs the smart home's keys and IR
codes, rooms, tasks, the Hub, timers and alarms, the calendar, the home certificate authority and
the household's settings and keys. Carry `bundle.zip` to the laptop, then (*admin*):

```powershell
tvl-server import --bundle bundle.zip
```

Delete `bundle.zip` afterwards from both machines: it holds the keys. Importing **before** the
first start keeps the old home certificate authority, so iPads that trust it need nothing new.

## 4. Install the service (*admin*)

```powershell
tvl-server install
tvl-server pin
```

`install` registers the service `tvl-server` (automatic, delayed start, restarted if it fails)
under its own account `NT SERVICE\tvl-server`, gives that account the data folder
`C:\ProgramData\TVLauncher\server` (nobody else can read the keys there), adds the firewall rule,
and starts it. `pin` shows the admin PIN: phones and tablets need it for settings and sign-ins;
paired TV PCs show it too.

Check: on the laptop, open `http://localhost:47960`; on a phone, `http://tvl.local:47960`.

## Day to day

- **Logs**: `C:\ProgramData\TVLauncher\server\logs\server.<date>.log`, two weeks kept. More detail:
  set `TVL_LOG=debug` for the service and restart it.
- **Restart**: `Restart-Service tvl-server` (*admin*).
- **Settings without a PIN**: `http://localhost:47960` on the laptop itself counts as admin.
- **Backups**: copy `C:\ProgramData\TVLauncher\server` somewhere safe and private: it holds the
  household's keys, the devices' local keys and the home CA's key.
- **Update**: pushes to main deploy by themselves (below). By hand: stop the service, replace
  `tvl-server.exe` (and `companion`), start it, or (*admin*) `scripts\deploy-server.ps1 -From <folder>`.
- **Remove**: `tvl-server uninstall` (*admin*); the data folder stays.

## Deploys on push

`.github/workflows/deploy-server.yml` runs on each push to main that touches `crates/`,
`apps/companion/`, the lock files or the deploy itself (or by hand: Actions › Deploy server › Run
workflow). GitHub tests the crates and builds the server and the companion; then the laptop, a
self-hosted runner labelled `home-hub`, runs `scripts\deploy-server.ps1`: stop the service, keep
the running build in `TVLauncher\previous`, copy the new one in, start it, and check that
`http://localhost:47960` answers. If it doesn't within a minute, the previous build goes back and
the run fails. `TVLauncher\deployed.txt` holds the commit that's running.

The household's data never goes through GitHub: it stays in `C:\ProgramData\TVLauncher\server`.

Set the laptop up once, after step 4 (*admin*):

```powershell
$token = gh api -X POST repos/GabrielSiek/home-hub/actions/runners/registration-token --jq .token
scripts\setup-deploy-runner.ps1 -Repo https://github.com/GabrielSiek/home-hub -Token $token
```

It installs the service if it isn't yet, lets the runner's account (`NETWORK SERVICE`) replace the
files in `C:\Program Files\TVLauncher` and stop/start `tvl-server` (it still can't read the data
folder), and registers the runner in `C:\actions-runner` as a Windows service. Check it under the
repo's Settings › Actions › Runners. Since the runner acts on the laptop, the workflow only runs
for pushes to main, never for pull requests.

## Next to the old launcher, on the dev PC

While testing, the old launcher can keep running:

```powershell
cargo run -p tvl-server -- run --dev --shadow
```

`--dev` uses ports 47970–47972 and `%LOCALAPPDATA%\TVLauncher\server-dev`. `--shadow` leaves the
smart-home LAN, automations, ringing and calendar cards to the old launcher: Tuya devices accept very
few LAN clients, and the two would ring every alarm twice. Even without `--shadow`, the server
stays off the LAN while the old launcher answers on 47960, unless `--force-lan`.
