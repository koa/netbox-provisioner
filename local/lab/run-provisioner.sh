#!/usr/bin/env bash
# Builds frontend and binary and runs the provisioner against the lab on
# http://127.0.0.1:8082/ (stays in the foreground). Needs the local NetBox
# (local/netbox/run-netbox.sh) and Keycloak (local/keycloak/run-keycloak.sh) running.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
"$root/local/lab/make-config.sh" >/dev/null
nix=(nix --extra-experimental-features 'nix-command flakes')
"${nix[@]}" shell nixpkgs#trunk nixpkgs#dart-sass -c sh -c "
  cd '$root/frontend' && trunk build --release &&
  touch '$root/binary/src/main.rs' && cd '$root' && cargo build -p binary"
cd "$root/.lab-netbox/provisioner"
LOG_LEVEL="${LOG_LEVEL:-info}" exec "$root/target/debug/binary"
