#!/usr/bin/env bash
# The library's services on the home server: Radarr, Sonarr, Prowlarr and qBittorrent, each a systemd
# service under one account, media, next to the hub. Run it once with sudo, then finish in their web
# pages (docs/OPERATIONS.md, "The library"):
#
#   sudo scripts/setup-media.sh [media folder]
#
# Safe to run again: what's already here is kept, so a second run never overwrites what the services
# have changed since. Radarr, Sonarr and Prowlarr update themselves, qBittorrent comes with apt.

set -euo pipefail

media=${1:-/mnt/dados/media}
account=media

step() { printf '\n== %s\n' "$1"; }

[ "$(id -u)" -eq 0 ] || { echo "Run it with sudo." >&2; exit 1; }
id tvl-server >/dev/null 2>&1 || { echo "No tvl-server account: run scripts/setup-server.sh first." >&2; exit 1; }

# Finished downloads are hardlinked into the library, and the hub reads them through their group.
disk=$(findmnt -no FSTYPE -T "$(dirname "$media")")
case $disk in
  ext4 | xfs | btrfs) ;;
  *) echo "$media is on $disk, which has no hardlinks or Unix permissions." >&2; exit 1 ;;
esac

# Unpacked once: after that they keep themselves up to date, so /opt belongs to their account.
fetch() {
  local name=$1 folder=$2 url=$3 unpacked
  [ -d "/opt/$name" ] && return
  unpacked=$(mktemp -d)
  curl -fsSL "$url" | tar xz -C "$unpacked"
  mv "$unpacked/$folder" "/opt/$name"
  rm -rf "$unpacked"
  chown -R "$account:$account" "/opt/$name"
}

# KillMode=process: their updater outlives the service it restarts. UMask keeps what they write
# readable by the group, which is how the hub gets at it.
unit() {
  local name=$1 description=$2 command=$3
  cat >"/etc/systemd/system/$name.service" <<EOF
# Written by scripts/setup-media.sh.
[Unit]
Description=$description
Wants=network-online.target
After=network-online.target

[Service]
User=$account
Group=$account
ExecStart=$command
Restart=on-failure
RestartSec=5
KillMode=process
TimeoutStopSec=20
UMask=0002
StateDirectory=$name
NoNewPrivileges=true
ProtectSystem=full
ProtectHome=true

[Install]
WantedBy=multi-user.target
EOF
}

key_of() {
  local config=/var/lib/$1/config.xml
  for _ in $(seq 1 60); do
    if grep -q '<ApiKey>' "$config" 2>/dev/null; then
      sed -n 's:.*<ApiKey>\(.*\)</ApiKey>.*:\1:p' "$config"
      return
    fi
    sleep 1
  done
  echo "(not written yet: it's in $config)"
}

step "Packages"
apt-get update
apt-get install -y curl libsqlite3-0 qbittorrent-nox

step "The account and the media folder"
id "$account" >/dev/null 2>&1 || useradd --system --no-create-home --shell /usr/sbin/nologin "$account"
install -d -m 2775 -o "$account" -g "$account" "$media" "$media/movies" "$media/series" "$media/downloads"
if ! id -nG tvl-server | grep -qw "$account"; then
  usermod -aG "$account" tvl-server
  # A service only picks up its groups when it starts.
  restart_hub=1
fi

step "Radarr, Sonarr and Prowlarr"
fetch radarr Radarr 'https://radarr.servarr.com/v1/update/master/updatefile?os=linux&runtime=netcore&arch=x64'
fetch sonarr Sonarr 'https://services.sonarr.tv/v1/download/main/latest?version=4&os=linux&arch=x64'
fetch prowlarr Prowlarr 'https://prowlarr.servarr.com/v1/update/master/updatefile?os=linux&runtime=netcore&arch=x64'
unit radarr "Radarr: finds and organises movies" "/opt/radarr/Radarr -nobrowser -data=/var/lib/radarr"
unit sonarr "Sonarr: finds and organises series" "/opt/sonarr/Sonarr -nobrowser -data=/var/lib/sonarr"
unit prowlarr "Prowlarr: indexers for Radarr and Sonarr" "/opt/prowlarr/Prowlarr -nobrowser -data=/var/lib/prowlarr"

step "qBittorrent"
unit qbittorrent "qBittorrent: fetches what Radarr and Sonarr ask for" "/usr/bin/qbittorrent-nox --profile=/var/lib/qbittorrent"
config=/var/lib/qbittorrent/qBittorrent/config/qBittorrent.conf
if [ ! -f "$config" ]; then
  install -d -o "$account" -g "$account" /var/lib/qbittorrent /var/lib/qbittorrent/qBittorrent "$(dirname "$config")"
  # Radarr, Sonarr and the hub all run here, so this machine needs no password. Anyone else does.
  # A title seeds until it has uploaded its own size or for a day, whichever comes first; after that
  # Radarr and Sonarr clear the torrent and its data from downloads/, and the library's copy stays.
  cat >"$config" <<EOF
[LegalNotice]
Accepted=true

[BitTorrent]
Session\\DefaultSavePath=$media/downloads
Session\\GlobalMaxRatio=1
Session\\GlobalMaxSeedingMinutes=1440

[Preferences]
WebUI\\LocalHostAuth=false
WebUI\\Port=8080
EOF
  chown "$account:$account" "$config"
fi

step "Starting"
systemctl daemon-reload
systemctl enable --now radarr sonarr prowlarr qbittorrent
[ -z "${restart_hub:-}" ] || systemctl restart tvl-server

step "Checking"
radarr_key=$(key_of radarr)
sonarr_key=$(key_of sonarr)
prowlarr_key=$(key_of prowlarr)
sleep 5
systemctl --no-pager --lines=0 status radarr sonarr prowlarr qbittorrent || true

server=$(hostname -I | cut -d' ' -f1)
cat <<EOF

Done. Their pages, and the keys the next steps ask for:
  Radarr       http://$server:7878   $radarr_key
  Sonarr       http://$server:8989   $sonarr_key
  Prowlarr     http://$server:9696   $prowlarr_key
  qBittorrent  http://$server:8080   user admin, temporary password:
    journalctl -u qbittorrent | grep -i 'temporary password' | tail -1

Next: docs/OPERATIONS.md, "The library".
EOF
