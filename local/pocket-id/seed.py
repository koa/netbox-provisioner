#!/usr/bin/env python3
"""Sets up the lab Pocket-ID (local/pocket-id/run-pocket-id.sh) like local/keycloak/realm.json.

The users admin (group `admin`), meier and huber (the NetBox owners' users of
local/netbox/seed-residents.py) and weber, whom NetBox doesn't know before its first login
(docs/login.md, „NetBox über OIDC“); the public client `netbox-provisioner` (the lab UI on
:8082, with `**`: `*` is one path segment) and the confidential client `netbox` for the lab
NetBox, whose secret it writes to netbox-client-secret next to the API key. Every login asks no
consent. Idempotent.

    local/pocket-id/seed.py [--data .lab-netbox/pocket-id]
"""
import argparse
import json
import pathlib
import urllib.request

URL = "http://localhost:8190/api"
USERS = {
    "admin": {"firstName": "Lab", "lastName": "Admin", "groups": ["admin"]},
    "meier": {"firstName": "Familie", "lastName": "Meier", "groups": []},
    "huber": {"firstName": "Huber", "lastName": "AG", "groups": []},
    "weber": {"firstName": "Weber", "lastName": "", "groups": []},
}
CLIENTS = {
    "netbox-provisioner": {
        "callbackURLs": ["http://localhost:8082/**"],
        "isPublic": True,
        "pkceEnabled": True,
    },
    "netbox": {
        "callbackURLs": ["http://127.0.0.1:8001/oauth/complete/oidc/"],
        "isPublic": False,
        "pkceEnabled": False,
    },
}


class Api:
    def __init__(self, key: str):
        self.key = key

    def call(self, method: str, path: str, body=None):
        request = urllib.request.Request(
            URL + path,
            method=method,
            data=None if body is None else json.dumps(body).encode(),
            headers={"X-API-KEY": self.key, "Content-Type": "application/json"},
        )
        with urllib.request.urlopen(request) as response:
            text = response.read()
        return json.loads(text) if text else None

    def all(self, path: str):
        return self.call("GET", f"{path}?pagination[limit]=100")["data"]


def main():
    parser = argparse.ArgumentParser()
    here = pathlib.Path(__file__).resolve().parent
    parser.add_argument("--data", default=here / "../../.lab-netbox/pocket-id", type=pathlib.Path)
    args = parser.parse_args()
    api = Api((args.data / "api-key").read_text().strip())

    users = {user["username"]: user for user in api.all("/users")}
    for name, user in USERS.items():
        if name not in users:
            users[name] = api.call("POST", "/users", {
                "username": name,
                "email": f"{name}@lab.local",
                "emailVerified": True,
                "firstName": user["firstName"],
                "lastName": user["lastName"],
                "isAdmin": False,
            })
            print(f"user {name}")
    groups = {group["name"]: group for group in api.all("/user-groups")}
    for group in sorted({group for user in USERS.values() for group in user["groups"]}):
        if group not in groups:
            groups[group] = api.call("POST", "/user-groups", {"name": group, "friendlyName": group})
            print(f"group {group}")
        members = [users[name]["id"] for name, user in USERS.items() if group in user["groups"]]
        api.call("PUT", f"/user-groups/{groups[group]['id']}/users", {"userIds": members})

    clients = {client["id"] for client in api.all("/oidc/clients")}
    for id, client in CLIENTS.items():
        body = {"id": id, "name": id, "logoutCallbackURLs": [], "skipConsent": True, **client}
        if id in clients:
            api.call("PUT", f"/oidc/clients/{id}", body)
        else:
            api.call("POST", "/oidc/clients", body)
            print(f"client {id}")
    secret = args.data / "netbox-client-secret"
    if not secret.exists() or not secret.read_text().strip():
        # only the one written stays
        for old in api.call("GET", "/oidc/clients/netbox")["credentials"]["secrets"]:
            api.call("DELETE", f"/oidc/clients/netbox/secrets/{old['id']}")
        secret.write_text(api.call("POST", "/oidc/clients/netbox/secrets", {})["secret"])
        print("secret of netbox written")


if __name__ == "__main__":
    main()
