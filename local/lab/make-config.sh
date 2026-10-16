#!/usr/bin/env bash
# Writes the provisioner config for the lab to .lab-netbox/provisioner/config.yaml: the local
# NetBox (local/netbox/run-netbox.sh) with its API token, the local Keycloak and the lab
# credentials. Run the provisioner (or a backend example) with that directory as CWD.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
out="$root/.lab-netbox/provisioner"
mkdir -p "$out"
if [ ! -s "$root/.lab-netbox/socks-password" ]; then
  head -c 18 /dev/urandom | base64 | tr -d "/+=" >"$root/.lab-netbox/socks-password"
fi
socks_password="$(cat "$root/.lab-netbox/socks-password")"
# the admin password of the lab devices stays out of the repository
if [ ! -s "$root/.lab-netbox/device-password" ]; then
  echo "put the lab devices' admin password into .lab-netbox/device-password" >&2
  exit 1
fi
device_password="$(cat "$root/.lab-netbox/device-password")"
cat >"$out/config.yaml" <<YAML
settings:
  auth_client_id: netbox-provisioner
  auth_issuer: http://localhost:8180/realms/lab
  auth_users: [lab-admin]
  server_port: 8082
  netbox_url: http://127.0.0.1:8001/graphql/
  netbox_token: "$(cat "$root/.lab-netbox/api-token")"
  # the helpers write to the lab NetBox with the same (admin) token
  allow_netbox_write: true
  netbox_write_token: "$(cat "$root/.lab-netbox/api-token")"
  # the lab's entry point: everything behind lab-core is reached through its /ip socks
  management_proxy:
    device: lab-core
    address: 172.17.1.82:1080
    user: provisioner
    password: "$socks_password"
  # the lab devices only carry what NetBox says, so the full reconciliation is wanted
  allow_apply: true
  # the channel the update preselects
  routeros_channel: stable
  mikrotik_credentials:
    lab:
      user: admin
      password: "$device_password"
YAML
echo "$out/config.yaml"
