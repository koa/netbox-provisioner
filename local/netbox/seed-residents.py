#!/usr/bin/env python3
"""Gives the lab NetBox two residents (docs/concepts/self-service.md, „Wer ist wer“).

The owner group „Anwohner“ with the owners „Familie Meier“ (NetBox user `meier`) and
„Huber AG“ (`huber`), named like the Keycloak users of local/keycloak/realm.json. Meier has
lab-wap, the port ether3 of lab-crs125 and the VLAN lab-wlan with its WLAN, Huber the port
ether4 of lab-crs125 and the VLAN lab-gast. Ownership changes no device's configuration.
Idempotent, like seed-lab.py, whose NetBox client it takes.

    local/netbox/seed-residents.py [--url http://127.0.0.1:8001] [--token-file …]
"""
import argparse
import importlib.util
import pathlib
import secrets

here = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("seed_lab", here / "seed-lab.py")
seed_lab = importlib.util.module_from_spec(spec)
spec.loader.exec_module(seed_lab)

RESIDENTS = {
    "Familie Meier": {
        "user": "meier",
        "devices": ["lab-wap"],
        "ports": [("lab-crs125", "ether3")],
        "vlans": ["lab-wlan"],
    },
    "Huber AG": {
        "user": "huber",
        "devices": [],
        "ports": [("lab-crs125", "ether4")],
        "vlans": ["lab-gast"],
    },
}


def one(nb, path, query):
    found = nb.call("GET", path, query=query)["results"]
    if len(found) != 1:
        raise SystemExit(f"{path} {query}: {len(found)} matches")
    return found[0]


def set_owner(nb, path, item, owner):
    if (item.get("owner") or {}).get("id") != owner:
        nb.call("PATCH", f"{path}/{item['id']}", {"owner": owner})


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", default="http://127.0.0.1:8001")
    parser.add_argument("--token-file", default=here.parent.parent / ".lab-netbox" / "api-token")
    args = parser.parse_args()
    nb = seed_lab.NetBox(args.url, pathlib.Path(args.token_file).read_text().strip())

    group = nb.ensure("users/owner-groups", {"name": "Anwohner"})["id"]
    for name, resident in RESIDENTS.items():
        users = nb.call("GET", "users/users", query={"username": resident["user"]})["results"]
        if users:
            user = users[0]["id"]
        else:
            # logs in to NetBox only over OIDC in production; the lab never logs in as it
            user = nb.call("POST", "users/users", {
                "username": resident["user"], "password": secrets.token_urlsafe(24),
            })["id"]
        owner = nb.ensure("users/owners", {"name": name}, {"group": group, "users": [user]})["id"]
        for device in resident["devices"]:
            set_owner(nb, "dcim/devices", one(nb, "dcim/devices", {"name": device}), owner)
        for device, port in resident["ports"]:
            interface = one(nb, "dcim/interfaces", {"device": device, "name": port})
            set_owner(nb, "dcim/interfaces", interface, owner)
        for vlan in resident["vlans"]:
            set_owner(nb, "ipam/vlans", one(nb, "ipam/vlans", {"name": vlan}), owner)
        print(f"{name}: owner {owner}, user {resident['user']}")


if __name__ == "__main__":
    main()
