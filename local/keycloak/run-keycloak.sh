#!/usr/bin/env bash
# Local Keycloak for the lab login, from nixpkgs (dev mode, in-memory database, so every start
# imports realm.json afresh; no containers):
# realm "lab" from realm.json with the public client "netbox-provisioner" and the users
# admin/admin and reader/reader (subjects "lab-admin", "lab-reader").
# Issuer http://localhost:8180/realms/lab, admin console admin/admin. Stays in the foreground.
#
# Keycloak rebuilds itself into its own lib/ on the first start, so it runs from a writable copy
# of the nix package in $LAB_KEYCLOAK_DATA (default ../../.lab-netbox/keycloak).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
data="${LAB_KEYCLOAK_DATA:-$here/../../.lab-netbox/keycloak}"
nix=(nix --extra-experimental-features 'nix-command flakes')
package="$("${nix[@]}" build --no-link --print-out-paths nixpkgs#keycloak)"
java="$("${nix[@]}" build --no-link --print-out-paths nixpkgs#openjdk21_headless)"
home="$data/$(basename "$package")"
if [ ! -d "$home" ]; then
  mkdir -p "$data"
  cp -r "$package" "$home.tmp"
  chmod -R u+w "$home.tmp"
  mv "$home.tmp" "$home"
fi
mkdir -p "$home/data/import"
cp "$here/realm.json" "$home/data/import/realm.json"
export JAVA_HOME="$java" PATH="$java/bin:$PATH" KC_HOME_DIR="$home"
export KC_BOOTSTRAP_ADMIN_USERNAME=admin KC_BOOTSTRAP_ADMIN_PASSWORD=admin
exec "$home/bin/.kc.sh-wrapped" start-dev --http-port 8180 --http-host 127.0.0.1 \
  --hostname http://localhost:8180 --import-realm --db dev-mem
