#!/usr/bin/env bash
# Builds frontend and binary and runs the provisioner against the lab on
# http://127.0.0.1:8082/ (stays in the foreground). Needs the local NetBox
# (local/netbox/run-netbox.sh) and Keycloak (local/keycloak/run-keycloak.sh) running.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
"$root/local/lab/make-config.sh" >/dev/null
nix=(nix --extra-experimental-features 'nix-command flakes')
if [ ! -d "$root/frontend/node/node_modules" ]; then
  "${nix[@]}" shell nixpkgs#nodejs -c sh -c "cd '$root/frontend/node' && npm install"
fi
# One build at a time, from an empty dist: a start right after a stopped one sometimes failed in
# wasm-bindgen with "error writing JS loader file to stage dir: No such file or directory" and
# went through the second time. The cause isn't known (not reproduced on purpose); a stale or
# shared dist would fit. The second try stays, said in the log.
exec 9>"$root/target/run-provisioner.lock"
flock 9
"${nix[@]}" shell nixpkgs#trunk nixpkgs#dart-sass -c sh -c "
  cd '$root/frontend' && trunk clean &&
  { trunk build --release || { echo 'trunk failed, trying once more' >&2; trunk build --release; }; } &&
  cd '$root' && cargo build -p binary"
# the binary keeps running; the lock is for the build only
exec 9>&-
cd "$root/.lab-netbox/provisioner"
LOG_LEVEL="${LOG_LEVEL:-info}" exec "$root/target/debug/binary"
