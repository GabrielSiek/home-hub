#!/usr/bin/env bash
# Puts a new build of the server into /opt/tvl-server and restarts the service. The deploy workflow
# runs it on the server; by hand: deploy-server.sh <folder with tvl-server and companion/> [commit]
#
# The data folder (/var/lib/tvl-server) is never touched. The build that was running is kept in
# /opt/tvl-server/previous; if the new one doesn't answer within a minute, it goes back.

set -euo pipefail

from=${1:?usage: deploy-server.sh <folder> [commit]}
commit=${2:-}
service=tvl-server
app=/opt/tvl-server
previous=$app/previous

step() { echo "== $1"; }

[ -f "$from/tvl-server" ] || { echo "No tvl-server in $from" >&2; exit 1; }
[ -d "$from/companion" ] || { echo "No companion in $from" >&2; exit 1; }

# The companion is mirrored (the files an older build had go), the binary swapped by rename so it
# can never be seen half written.
copy_parts() {
  local src=$1 dst=$2
  install -m 755 "$src/tvl-server" "$dst/tvl-server.new"
  mv -f "$dst/tvl-server.new" "$dst/tvl-server"
  mkdir -p "$dst/companion"
  rsync -a --delete "$src/companion/" "$dst/companion/"
}

# Up, with its data loaded (/ca.crt) and the companion's page served (/).
test_server() {
  for _ in $(seq 30); do
    sleep 2
    systemctl is-active --quiet "$service" || continue
    curl -fsS -o /dev/null -m 3 http://127.0.0.1:47960/ca.crt &&
      curl -fsS -o /dev/null -m 3 http://127.0.0.1:47960/ && return 0
  done
  return 1
}

step "Stopping $service"
sudo systemctl stop "$service"

step "Keeping the running build in $previous"
mkdir -p "$previous"
[ -f "$app/tvl-server" ] && copy_parts "$app" "$previous"

step "Copying the new build${commit:+ ($commit)} into $app"
copy_parts "$from" "$app"
[ -z "$commit" ] || echo "$commit $(date -Iseconds)" > "$app/deployed.txt"

step "Starting $service"
sudo systemctl start "$service"
if test_server; then
  step "Deployed: the server answers on http://localhost:47960"
  exit 0
fi

echo "The new build didn't come up: putting the previous one back. See /var/lib/tvl-server/logs or journalctl -u $service."
sudo systemctl stop "$service"
if [ -f "$previous/tvl-server" ]; then
  copy_parts "$previous" "$app"
fi
sudo systemctl start "$service"
if test_server; then echo "The previous build is running again."; else echo "The previous build did not come up either."; fi
exit 1
