#!/usr/bin/env bash
# Sets the server up to deploy itself on each push to main (.github/workflows/deploy-server.yml).
# Run once with sudo, after setup-server.sh:
#
#   sudo scripts/setup-deploy-runner.sh https://github.com/<owner>/<repo> <token>
#
#   1. Gives the runner its own account, tvl-runner, which owns /opt/tvl-server and may stop and
#      start the tvl-server service: nothing more. It can't read the data folder or its keys.
#   2. Downloads the GitHub Actions runner (checksum verified) into /opt/actions-runner and
#      registers it with the repo as a systemd service, labelled home-hub
#
# The token comes from the repo's Settings > Actions > Runners > New self-hosted runner.

set -euo pipefail

repo=${1:?usage: setup-deploy-runner.sh <repo url> <token>}
token=${2:?usage: setup-deploy-runner.sh <repo url> <token>}
dir=/opt/actions-runner
account=tvl-runner

step() { printf '\n== %s\n' "$1"; }

[ "$(id -u)" -eq 0 ] || { echo "Run it with sudo." >&2; exit 1; }
[ ! -e "$dir/.runner" ] || { echo "$dir already holds a registered runner: remove it first (config.sh remove)." >&2; exit 1; }

step "The $account account"
id "$account" >/dev/null 2>&1 || useradd --system --create-home --shell /usr/sbin/nologin "$account"
chown -R "$account:$account" /opt/tvl-server
cat > /etc/sudoers.d/tvl-runner <<EOF
$account ALL=(root) NOPASSWD: /usr/bin/systemctl stop tvl-server, /usr/bin/systemctl start tvl-server
EOF
chmod 440 /etc/sudoers.d/tvl-runner
visudo -cf /etc/sudoers.d/tvl-runner

step "The GitHub Actions runner in $dir"
release=$(curl -fsSL https://api.github.com/repos/actions/runner/releases/latest)
version=$(jq -r .tag_name <<<"$release" | sed 's/^v//')
archive=actions-runner-linux-x64-$version.tar.gz
sha=$(jq -r .body <<<"$release" | sed -n 's/.*<!-- BEGIN SHA linux-x64 -->\([0-9a-f]\{64\}\)<!-- END SHA linux-x64 -->.*/\1/p')
[ -n "$sha" ] || { echo "No checksum for $archive in the release notes." >&2; exit 1; }
download=$(mktemp)
curl -fsSL -o "$download" "https://github.com/actions/runner/releases/download/v$version/$archive"
echo "$sha  $download" | sha256sum -c -
mkdir -p "$dir"
tar -xzf "$download" -C "$dir"
rm -f "$download"
chown -R "$account:$account" "$dir"
"$dir/bin/installdependencies.sh"
echo "  Runner $version"

step "Registering it with GitHub, as a systemd service"
cd "$dir"
sudo -u "$account" ./config.sh --unattended --url "$repo" --token "$token" --name "$(hostname)" --labels home-hub --replace
./svc.sh install "$account"
./svc.sh start

printf '\nDone. Each push to main that touches the server or the companion now deploys here.\n'
