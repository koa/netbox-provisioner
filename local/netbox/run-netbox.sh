#!/usr/bin/env bash
# Local NetBox for the lab: PostgreSQL, Redis and NetBox from nixpkgs, as plain user
# processes (no root, no containers). Data in $LAB_NETBOX_DATA (default
# ../../.lab-netbox). Creates the database, the superuser admin/admin and the API
# token printed at the end on first start; NetBox on http://127.0.0.1:8001/.
#   local/netbox/run-netbox.sh          start (stays in the foreground)
#   local/netbox/run-netbox.sh manage … run a NetBox management command
set -euo pipefail
export USER="${USER:-$(id -un)}"
here="$(cd "$(dirname "$0")" && pwd)"
export LAB_NETBOX_DATA="${LAB_NETBOX_DATA:-$here/../../.lab-netbox}"
export LAB_REDIS_PORT="${LAB_REDIS_PORT:-6380}"
mkdir -p "$LAB_NETBOX_DATA"
export LAB_NETBOX_DATA="$(cd "$LAB_NETBOX_DATA" && pwd)"
export PYTHONPATH="$here"
export NETBOX_CONFIGURATION=configuration

if [ -z "${IN_LAB_NIX_SHELL:-}" ]; then
  export IN_LAB_NIX_SHELL=1
  exec nix --extra-experimental-features 'nix-command flakes' shell \
    nixpkgs#netbox nixpkgs#postgresql_16 nixpkgs#redis -c "$0" "$@"
fi

if [ "${1:-}" = manage ]; then
  shift
  exec netbox "$@"
fi

pg="$LAB_NETBOX_DATA/pg"
socket="$LAB_NETBOX_DATA/pg-socket"
mkdir -p "$socket"
if [ ! -d "$pg" ]; then
  initdb -D "$pg" -A trust -U "$USER" >/dev/null
fi
pg_ctl -D "$pg" -o "-k $socket -c listen_addresses=''" -l "$LAB_NETBOX_DATA/pg.log" -w start
redis-server --port "$LAB_REDIS_PORT" --bind 127.0.0.1 --save '' --daemonize yes \
  --logfile "$LAB_NETBOX_DATA/redis.log"
cleanup() {
  redis-cli -p "$LAB_REDIS_PORT" shutdown nosave >/dev/null 2>&1 || true
  pg_ctl -D "$pg" -m fast stop >/dev/null 2>&1 || true
}
trap cleanup EXIT

if ! psql -h "$socket" -d postgres -tAc "select 1 from pg_database where datname='netbox'" | grep -q 1; then
  createdb -h "$socket" netbox
fi
netbox migrate --no-input >"$LAB_NETBOX_DATA/migrate.log"
netbox collectstatic --no-input >/dev/null
if [ ! -s "$LAB_NETBOX_DATA/api-token" ]; then
  netbox shell -c "
from users.models import User, Token
user = User.objects.filter(username='admin').first() or User.objects.create_superuser('admin', 'admin@lab.local', 'admin')
token = Token(user=user, description='provisioner lab')
token.save()
print(f'nbt_{token.key}.{token.token}')
" | tail -1 >"$LAB_NETBOX_DATA/api-token"
fi
echo "NetBox on http://127.0.0.1:8001/ (admin/admin), API token: $(cat "$LAB_NETBOX_DATA/api-token")"
netbox runserver --insecure 127.0.0.1:8001
