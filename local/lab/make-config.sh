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
# the secret the lab NetBox signs its webhook with (automatic rollout)
if [ ! -s "$root/.lab-netbox/webhook-secret" ]; then
  head -c 18 /dev/urandom | base64 | tr -d "/+=" >"$root/.lab-netbox/webhook-secret"
fi
webhook_secret="$(cat "$root/.lab-netbox/webhook-secret")"
# the admin password of the lab devices stays out of the repository
if [ ! -s "$root/.lab-netbox/device-password" ]; then
  echo "put the lab devices' admin password into .lab-netbox/device-password" >&2
  exit 1
fi
device_password="$(cat "$root/.lab-netbox/device-password")"
# the address lab-np2 sees this machine at; the devices behind it see lab-np2's
own_address="$(python3 -c 'import socket; s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.connect(("172.17.1.82", 9)); print(s.getsockname()[0])')"
# the CAs of WebFig over HTTPS, made once: a root (www-root.pem, the browser's, from
# /www-ca.crt) and the intermediate CA under it issuing the devices' certificates (www-ca.pem),
# which the devices send along
lab="$root/.lab-netbox"
if [ ! -s "$lab/www-root.key" ]; then
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 3650 \
    -subj "/CN=netbox-provisioner lab root CA" \
    -keyout "$lab/www-root.key" -out "$lab/www-root.pem" 2>/dev/null
  openssl req -new -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes \
    -subj "/CN=netbox-provisioner lab CA" -keyout "$lab/www-ca.key" -out "$lab/www-ca.csr" 2>/dev/null
  printf '%s\n' basicConstraints=critical,CA:TRUE keyUsage=critical,keyCertSign,cRLSign \
    subjectKeyIdentifier=hash authorityKeyIdentifier=keyid >"$lab/www-ca.ext"
  openssl x509 -req -in "$lab/www-ca.csr" -CA "$lab/www-root.pem" -CAkey "$lab/www-root.key" \
    -CAcreateserial -days 3650 -extfile "$lab/www-ca.ext" -out "$lab/www-ca.pem" 2>/dev/null
  rm "$lab/www-ca.csr" "$lab/www-ca.ext"
fi
pem() { sed 's/^/      /' "$1"; }
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
  netbox_webhook_secret: "$webhook_secret"
  # the lab's entry point: everything behind lab-np2 is reached through its /ip socks
  management_proxy:
    device: lab-np2
    address: 172.17.1.82:1080
    user: provisioner
    password: "$socks_password"
  # the lab devices only carry what NetBox says, so the full reconciliation is wanted
  allow_apply: true
  # the channel the update preselects, as in production
  routeros_channel: long-term
  # the plain API answers only the provisioner: directly at lab-np2, through its SOCKS behind it
  # (over IPv6 if the device has an address there)
  api_networks: [192.168.0.0/16, "2a02:16a:fe00:8000::/50", "$own_address/32"]
  www_ca:
    certificate: |
$(pem "$root/.lab-netbox/www-ca.pem")
    key: |
$(pem "$root/.lab-netbox/www-ca.key")
    root: |
$(pem "$root/.lab-netbox/www-root.pem")
  mikrotik_credentials:
    lab:
      user: admin
      password: "$device_password"
YAML
echo "$out/config.yaml"
