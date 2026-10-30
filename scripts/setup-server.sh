#!/usr/bin/env bash
# Turns a fresh Ubuntu Server into the home server, from the backup of the old one. Run it once with
# sudo, with the backup's disk mounted and a build of the server at hand:
#
#   sudo scripts/setup-server.sh <backup folder> <folder with tvl-server and companion/>
#
# The backup holds tvl-server/ (the hub's data folder) and adguard/ (AdGuard Home's). It's only read.
# DNS comes first, since the whole house depends on it, then the hub. Safe to run again: data that's
# already here is kept, so a second run never overwrites what the server has changed since.

set -euo pipefail

backup=${1:?usage: setup-server.sh <backup folder> <build folder>}
build=${2:?usage: setup-server.sh <backup folder> <build folder>}
here=$(cd "$(dirname "$0")" && pwd)
zone=America/Sao_Paulo
data=/var/lib/tvl-server
app=/opt/tvl-server
adguard=/opt/AdGuardHome

step() { printf '\n== %s\n' "$1"; }

[ "$(id -u)" -eq 0 ] || { echo "Run it with sudo." >&2; exit 1; }
[ -f "$backup/tvl-server/companion-ca.key" ] || { echo "No tvl-server/companion-ca.key in $backup: is it the backup?" >&2; exit 1; }
[ -f "$backup/adguard/AdGuardHome.yaml" ] || { echo "No adguard/AdGuardHome.yaml in $backup: is it the backup?" >&2; exit 1; }
[ -f "$build/tvl-server" ] && [ -d "$build/companion" ] || { echo "No tvl-server and companion/ in $build" >&2; exit 1; }

step "Packages"
apt-get update
apt-get install -y avahi-daemon curl jq rsync tzdata

step "Time zone and clock (alarms depend on them)"
timedatectl set-timezone "$zone"
timedatectl set-ntp true

step "Keep running with the lid closed, never sleep"
mkdir -p /etc/systemd/logind.conf.d
cat > /etc/systemd/logind.conf.d/server.conf <<EOF
[Login]
HandleLidSwitch=ignore
HandleLidSwitchExternalPower=ignore
HandleLidSwitchDocked=ignore
EOF
systemctl mask sleep.target suspend.target hibernate.target hybrid-sleep.target
systemctl restart systemd-logind

step "Free port 53 for AdGuard Home"
mkdir -p /etc/systemd/resolved.conf.d
cat > /etc/systemd/resolved.conf.d/adguard.conf <<EOF
[Resolve]
DNSStubListener=no
EOF
ln -sf /run/systemd/resolve/resolv.conf /etc/resolv.conf
systemctl restart systemd-resolved

step "AdGuard Home"
if [ ! -x "$adguard/AdGuardHome" ]; then
  url=https://github.com/AdguardTeam/AdGuardHome/releases/latest/download
  download=$(mktemp -d)
  curl -fsSL -o "$download/adguard.tar.gz" "$url/AdGuardHome_linux_amd64.tar.gz"
  curl -fsSL -o "$download/checksums.txt" "$url/checksums.txt"
  (cd "$download" && grep ' AdGuardHome_linux_amd64.tar.gz$' checksums.txt | sed 's/ AdGuardHome_linux_amd64.tar.gz$/ adguard.tar.gz/' | sha256sum -c -)
  tar -xzf "$download/adguard.tar.gz" -C /opt
  rm -rf "$download"
fi
if [ ! -f "$adguard/AdGuardHome.yaml" ]; then
  # Its yaml keeps one Windows path, which Linux wouldn't take as a path at all.
  sed 's#^\( *- \)C:\\ProgramData\\AdGuardHome\\userfilters\\\*#\1/opt/AdGuardHome/userfilters/*#' \
    "$backup/adguard/AdGuardHome.yaml" > "$adguard/AdGuardHome.yaml"
  chmod 600 "$adguard/AdGuardHome.yaml"
  cp -r "$backup/adguard/data" "$adguard/data"
  chmod -R go-rwx "$adguard/data"
fi
if grep -q 'C:\\' "$adguard/AdGuardHome.yaml"; then
  echo "AdGuardHome.yaml still names a Windows path:" >&2
  grep -n 'C:\\' "$adguard/AdGuardHome.yaml" >&2
  exit 1
fi
systemctl is-enabled --quiet AdGuardHome 2>/dev/null || "$adguard/AdGuardHome" -s install
systemctl restart AdGuardHome

step "The hub's account and folders"
id tvl-server >/dev/null 2>&1 || useradd --system --no-create-home --shell /usr/sbin/nologin tvl-server
mkdir -p "$app/companion"
install -d -m 700 -o tvl-server -g tvl-server "$data"

step "The hub's data"
if [ -e "$data/companion-ca.key" ]; then
  echo "  Already here: kept."
else
  cp -r "$backup/tvl-server/." "$data/"
  chown -R tvl-server:tvl-server "$data"
  chmod -R go-rwx "$data"
fi

step "The hub"
install -m 755 "$build/tvl-server" "$app/tvl-server"
rsync -a --delete "$build/companion/" "$app/companion/"
install -m 644 "$here/tvl-server.service" /etc/systemd/system/tvl-server.service
systemctl daemon-reload
systemctl enable tvl-server
systemctl restart tvl-server

step "Checking"
sleep 5
systemctl --no-pager --lines=0 status AdGuardHome tvl-server || true
curl -fsS -o /dev/null -m 5 http://127.0.0.1:47960/ca.crt && echo "  Hub: answers."
getent hosts google.com >/dev/null && echo "  Name lookups: fine."

printf '\nDone. Next: Tailscale (--accept-dns=false), then scripts/setup-deploy-runner.sh.\n'
