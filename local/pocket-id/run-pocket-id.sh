#!/usr/bin/env bash
# Local Pocket-ID for the lab, from nixpkgs, as production logs in (docs/login.md): on
# http://localhost:8190, data (SQLite, keys) in $LAB_POCKET_ID_DATA (default
# ../../.lab-netbox/pocket-id). Its API answers the key in api-key there (made once, header
# X-API-KEY), which local/pocket-id/seed.py sets it up with. Nobody needs a passkey: a login
# link comes from `run-pocket-id.sh token <user>`. Stays in the foreground.
#   local/pocket-id/run-pocket-id.sh             start
#   local/pocket-id/run-pocket-id.sh token USER  print a one-time login link for USER
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
data="${LAB_POCKET_ID_DATA:-$here/../../.lab-netbox/pocket-id}"
mkdir -p "$data"
data="$(cd "$data" && pwd)"
nix=(nix --extra-experimental-features 'nix-command flakes')
package="$("${nix[@]}" build --no-link --print-out-paths nixpkgs#pocket-id)"
for secret in api-key encryption-key; do
  if [ ! -s "$data/$secret" ]; then
    head -c 32 /dev/urandom | base64 | tr -d "/+=" >"$data/$secret"
  fi
done
export APP_URL=http://localhost:8190 HOST=127.0.0.1 PORT=8190
export DB_CONNECTION_STRING="file:$data/pocket-id.db?_pragma=journal_mode(WAL)&_pragma=busy_timeout(2500)"
export UPLOAD_PATH="$data/uploads" KEYS_STORAGE=database
export ENCRYPTION_KEY="$(cat "$data/encryption-key")" STATIC_API_KEY="$(cat "$data/api-key")"
cd "$data"
if [ "${1:-}" = token ]; then
  exec "$package/bin/pocket-id" one-time-access-token "$2"
fi
exec "$package/bin/pocket-id"
