#!/usr/bin/env python3
"""Models the lab (CCR2004 + 2 netPower + 2 SXT + wAP + 2 old APs) in the local NetBox.

Idempotent: every object is looked up by its natural key and created or updated to the values
below, so the script can be rerun after a change. Needs a running local NetBox
(local/netbox/run-netbox.sh) and the custom fields from docs/netbox-import/.

    local/netbox/seed-lab.py [--url http://127.0.0.1:8001] [--token-file .lab-netbox/api-token]
"""
import argparse
import json
import pathlib
import sys
import urllib.error
import urllib.parse
import urllib.request

V4_LOOPBACK = "192.168.100.{}/32"
V6 = "2a02:16a:fe00:{}"


class NetBox:
    def __init__(self, url, token):
        self.url = url.rstrip("/")
        self.token = token

    def call(self, method, path, body=None, query=None):
        url = f"{self.url}/api/{path}/"
        if query:
            url += "?" + urllib.parse.urlencode(query)
        request = urllib.request.Request(
            url,
            method=method,
            data=None if body is None else json.dumps(body).encode(),
            headers={
                "Authorization": f"Bearer {self.token}",
                "Content-Type": "application/json",
                "Accept": "application/json",
            },
        )
        try:
            with urllib.request.urlopen(request) as response:
                return json.load(response) if response.status != 204 else None
        except urllib.error.HTTPError as error:
            sys.exit(f"{method} {url} {json.dumps(body)}: {error.code} {error.read().decode()}")

    def ensure(self, path, key, values=None):
        """The object at `path` matching the filter `key`, created or updated to `key | values`.

        A key ending in `_id` is a filter on a related object; it is sent without the suffix.
        """
        values = {**{k.removesuffix("_id"): v for k, v in key.items()}, **(values or {})}
        found = self.call("GET", path, query=key)["results"]
        if len(found) > 1:
            sys.exit(f"{path} {key}: {len(found)} matches")
        if not found:
            return self.call("POST", path, values)
        current = found[0]
        changes = {k: v for k, v in values.items() if not same(current.get(k), v)}
        return self.call("PATCH", f"{path}/{current['id']}", changes) if changes else current


def same(current, wanted):
    """Compares a value as NetBox returns it with the value we would send."""
    if isinstance(current, dict):
        if "id" in current and isinstance(wanted, int):
            return current["id"] == wanted
        if "value" in current:
            return current["value"] == wanted
        if isinstance(wanted, dict):
            return all(same(current.get(k), v) for k, v in wanted.items())
    if isinstance(current, list) and isinstance(wanted, list):
        ids = [c["id"] if isinstance(c, dict) and "id" in c else c for c in current]
        names = [c.get("slug", c.get("name")) if isinstance(c, dict) else c for c in current]
        wanted_ids = [w["id"] if isinstance(w, dict) and "id" in w else w for w in wanted]
        wanted_names = [w.get("slug", w.get("name")) if isinstance(w, dict) else w for w in wanted]
        return sorted(map(str, ids)) == sorted(map(str, wanted_ids)) or sorted(
            map(str, names)
        ) == sorted(map(str, wanted_names))
    return current == wanted


# Devices: name -> (device type, loopback index, interfaces). Interface: name -> fields, where
# "ip" are addresses, "tags" tag slugs, "bridge" the name of a bridge interface on the same
# device, "mtu" the L3 MTU. The links between the devices are modeled as in production: no MTU
# (RouterOS's 1500), IPv4 only, IPv6 link-local. MAC addresses are not kept in NetBox: at the
# first setup the device is picked from what its neighbor discovers.
DEVICES = {
    "lab-core": ("CCR2004-1G-12S+2XS", 1, {
        "ether1": {"label": "Uplink", "tags": ["dhcp-client", "ospf"]},
        "sfp-sfpplus1": {"label": "NP1", "ip": ["192.168.101.1/30"],
                         "tags": ["ospf"]},
        "sfp-sfpplus2": {"label": "NP2", "ip": ["192.168.101.5/30"],
                         "tags": ["ospf"]},
        # the networks of the WLANs, which the CAPs tunnel here (VXLAN of the VLAN group)
        "wlan": {"type": "virtual", "vlan": "lab-wlan",
                 "ip": ["192.168.104.1/24", V6.format("9001::1/64")], "tags": ["dhcp"]},
        "gast": {"type": "virtual", "vlan": "lab-gast",
                 "ip": ["192.168.105.1/24", V6.format("9002::1/64")], "tags": ["dhcp"]},
    }),
    "lab-np1": ("CRS318-16P-2S+", 2, {
        "sfp-sfpplus1": {"label": "Core", "ip": ["192.168.101.2/30"],
                         "tags": ["ospf"]},
        "ether1": {"label": "SXT A", "ip": ["192.168.101.9/29"],
                   "tags": ["ospf"], "poe_mode": "pse"},
        "ether2": {"label": "AP1", "ip": ["192.168.101.21/30"],
                   "tags": ["ospf"], "poe_mode": "pse"},
    }),
    "lab-np2": ("CRS318-16P-2S+", 3, {
        "sfp-sfpplus1": {"label": "Core", "ip": ["192.168.101.6/30"],
                         "tags": ["ospf"]},
        "ether1": {"label": "SXT B", "ip": ["192.168.101.10/29"],
                   "tags": ["ospf"], "poe_mode": "pse"},
        "lan": {"type": "bridge", "ip": ["192.168.103.1/24", V6.format("9000::1/64")],
                "tags": ["dhcp"]},
        "ether2": {"label": "wAP", "ip": ["192.168.101.17/30"],
                   "tags": ["ospf"], "poe_mode": "pse"},
        "ether3": {"label": "AP2", "ip": ["192.168.101.25/30"],
                   "tags": ["ospf"], "poe_mode": "pse"},
        "ether4": {"label": "Kunde", "bridge": "lan", "poe_mode": "pse"},
    }),
    "lab-sxt-a": ("RBSXTG-5HPacD", 4, {
        "link": {"type": "bridge", "ip": ["192.168.101.11/29"],
                 "tags": ["ospf"]},
        "ether1": {"label": "NP1", "bridge": "link"},
        "wlan1": {"type": "ieee802.11ac", "bridge": "link", "rf_role": "ap"},
    }),
    "lab-sxt-b": ("RBSXTG-5HPacD", 5, {
        "link": {"type": "bridge", "ip": ["192.168.101.12/29"],
                 "tags": ["ospf"]},
        "ether1": {"label": "NP2", "bridge": "link"},
        "wlan1": {"type": "ieee802.11ac", "bridge": "link", "rf_role": "station"},
    }),
    # a CAP of lab-core's CAPsMAN (WLAN_GROUP), routed like every other device; the customer
    # port is in the first WLAN's VLAN, which goes to lab-core over VXLAN like the WLANs. Its
    # 802.11ac radios can't tag the WLANs' VLANs with wifi-qcom-ac, so they run `wireless`
    # (`wlan<N>`), as a CAP of the legacy CAPsMAN
    "lab-wap": ("RBwAPG-5HacD2HnD", 6, {
        "ether1": {"label": "NP2", "ip": ["192.168.101.18/30"],
                   "tags": ["ospf"]},
        "ether2": {"label": "Kunde", "vlan": "lab-wlan"},
        "wlan1": {"type": "ieee802.11n"},
        "wlan2": {"type": "ieee802.11ac"},
    }),
    # older APs of the WLAN (RouterOS `wireless` package): CAPs of the legacy CAPsMAN on
    # lab-core; they forward their clients themselves, into the VXLAN to lab-core
    "lab-ap1": ("RBSXTG-5HPacD", 7, {
        "ether1": {"label": "NP1", "ip": ["192.168.101.22/30"],
                   "tags": ["ospf"]},
        "wlan1": {"type": "ieee802.11ac"},
    }),
    "lab-ap2": ("RB912UAG-2HPnD", 8, {
        "ether1": {"label": "NP2", "ip": ["192.168.101.26/30"],
                   "tags": ["ospf"]},
        "wlan1": {"type": "ieee802.11n"},
        # 5 GHz card in the miniPCIe slot
        "wlan2": {"type": "ieee802.11a"},
    }),
}
# interfaces of an earlier lab layout, deleted
REMOVED_INTERFACES = [("lab-wap", "lan"), ("lab-wap", "wifi1"), ("lab-wap", "wifi2")]
# VLAN groups (with their VXLAN VNI) and their VLANs: name -> vid
VLAN_GROUPS = {"lab-wlan": {"vni": 1, "vlans": {"lab-wlan": 104, "lab-gast": 105}}}
# Wireless LAN Group: controller, its APs (device custom field wlan_group) and WLANs
WLAN_GROUP = {"name": "lab-wlan", "controller": "lab-core", "aps": ["lab-wap", "lab-ap1", "lab-ap2"],
              "wlans": [{"ssid": "lab-wlan", "psk": "lab-wlan-preshared-key", "vlan": "lab-wlan"},
                        {"ssid": "lab-gast", "psk": "lab-gast-preshared-key", "vlan": "lab-gast"}]}
CABLES = [
    (("lab-core", "sfp-sfpplus1"), ("lab-np1", "sfp-sfpplus1")),
    (("lab-core", "sfp-sfpplus2"), ("lab-np2", "sfp-sfpplus1")),
    (("lab-np1", "ether1"), ("lab-sxt-a", "ether1")),
    (("lab-np2", "ether1"), ("lab-sxt-b", "ether1")),
    (("lab-np2", "ether2"), ("lab-wap", "ether1")),
    (("lab-np1", "ether2"), ("lab-ap1", "ether1")),
    (("lab-np2", "ether3"), ("lab-ap2", "ether1")),
]
PREFIXES = [
    "192.168.100.0/26", "192.168.101.0/30", "192.168.101.4/30", "192.168.101.8/29",
    "192.168.103.0/24", "192.168.101.16/30", "192.168.101.20/30", "192.168.101.24/30",
    "192.168.104.0/24", "192.168.105.0/24", V6.format("8000::/64"), V6.format("9000::/64"),
    V6.format("9001::/64"), V6.format("9002::/64"),
]


def interface_type(name, fields):
    if "type" in fields:
        return fields["type"]
    if name == "lo":
        return "virtual"
    if name.startswith("sfp28-"):
        return "25gbase-x-sfp28"
    if name.startswith("sfp-sfpplus"):
        return "10gbase-x-sfpp"
    return "1000base-t"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", default="http://127.0.0.1:8001")
    here = pathlib.Path(__file__).resolve().parent
    parser.add_argument("--token-file", default=here.parent.parent / ".lab-netbox" / "api-token")
    args = parser.parse_args()
    nb = NetBox(args.url, pathlib.Path(args.token_file).read_text().strip())

    tags = {slug: nb.ensure("extras/tags", {"slug": slug}, {"name": slug})["id"]
            for slug in ["ospf", "dhcp", "dhcp-client", "nat"]}
    nb.ensure("ipam/roles", {"slug": "dhcp"}, {"name": "dhcp"})
    platform = nb.ensure("dcim/platforms", {"slug": "routeros"}, {"name": "routeros"})["id"]
    region = nb.ensure("dcim/regions", {"slug": "ch"}, {"name": "Schweiz"})["id"]
    tenant = nb.ensure("tenancy/tenants", {"slug": "lab"},
                       {"name": "Lab", "custom_fields": {"mikrotik_credentials": "lab"}})["id"]
    # the DNS servers of the lab's client networks: the gateway router (Blocky,
    # 172.31.88.2/2a02:16a:fe00:1a0::10:5, isn't reachable from the lab for now)
    nb.ensure("extras/config-contexts", {"name": "lab-dns"},
              {"weight": 1000, "tenants": [tenant],
               "data": {"dns_servers": ["172.16.1.4", "fd7e:907d:34ab::4"]}})
    site = nb.ensure("dcim/sites", {"slug": "lab"},
                     {"name": "Lab", "region": region, "tenant": tenant, "status": "active"})["id"]
    manufacturer = nb.ensure("dcim/manufacturers", {"slug": "mikrotik"}, {"name": "MikroTik"})["id"]
    role = nb.ensure("dcim/device-roles", {"slug": "router"}, {"name": "Router", "color": "2196f3"})["id"]
    for prefix in PREFIXES:
        nb.ensure("ipam/prefixes", {"prefix": prefix}, {"tenant": tenant, "status": "active"})

    vlans = {}
    for group_name, group in VLAN_GROUPS.items():
        vlan_group = nb.ensure("ipam/vlan-groups", {"slug": group_name},
                               {"name": group_name, "custom_fields": {"vxlan_vni": group["vni"]}})
        for vlan_name, vid in group["vlans"].items():
            vlans[vlan_name] = nb.ensure("ipam/vlans", {"group_id": vlan_group["id"], "vid": vid},
                                         {"name": vlan_name, "tenant": tenant,
                                          "status": "active"})["id"]

    for device_name, name in REMOVED_INTERFACES:
        found = nb.call("GET", "dcim/interfaces", query={"device": device_name, "name": name})
        for interface in found["results"]:
            nb.call("DELETE", f"dcim/interfaces/{interface['id']}")

    interfaces = {}
    for device_name, (model, loopback, ports) in DEVICES.items():
        device_type = nb.ensure("dcim/device-types", {"slug": model.lower().replace("+", "-plus")},
                                {"model": model, "manufacturer": manufacturer})["id"]
        device = nb.ensure("dcim/devices", {"name": device_name},
                           {"device_type": device_type, "role": role, "site": site,
                            "tenant": tenant, "platform": platform, "status": "active"})["id"]
        ports = {"lo": {"ip": [V4_LOOPBACK.format(loopback), V6.format(f"8000::{loopback}/128")]},
                 **ports}
        # bridges first, so that their members can refer to them
        for name, fields in sorted(ports.items(), key=lambda p: p[1].get("type") != "bridge"):
            values = {"type": interface_type(name, fields), "label": fields.get("label", ""),
                      "tags": [tags[t] for t in fields.get("tags", [])]}
            # also clears a bridge the interface is no longer in
            values["bridge"] = (interfaces[(device_name, fields["bridge"])]
                                if "bridge" in fields else None)
            # also clears a VLAN the interface is no longer in
            values["mode"] = "access" if "vlan" in fields else None
            values["mtu"] = fields.get("mtu")
            values["untagged_vlan"] = vlans[fields["vlan"]] if "vlan" in fields else None
            for key in ["poe_mode", "rf_role"]:
                if key in fields:
                    values[key] = fields[key]
            interface = nb.ensure("dcim/interfaces", {"device_id": device, "name": name}, values)
            interfaces[(device_name, name)] = interface["id"]
            for address in fields.get("ip", []):
                ip = nb.ensure("ipam/ip-addresses", {"address": address},
                               {"tenant": tenant, "status": "active",
                                "assigned_object_type": "dcim.interface",
                                "assigned_object_id": interface["id"]})
                if name == "lo":
                    family = "primary_ip4" if ip["family"]["value"] == 4 else "primary_ip6"
                    nb.ensure("dcim/devices", {"name": device_name}, {family: ip["id"]})

    for (a_device, a_port), (b_device, b_port) in CABLES:
        a = interfaces[(a_device, a_port)]
        b = interfaces[(b_device, b_port)]
        existing = nb.call("GET", "dcim/cables", query={"interface_id": a})["results"]
        if not existing:
            nb.call("POST", "dcim/cables", {
                "a_terminations": [{"object_type": "dcim.interface", "object_id": a}],
                "b_terminations": [{"object_type": "dcim.interface", "object_id": b}],
                "status": "connected",
            })

    psk = "lab-link-preshared-key"
    link = nb.call("GET", "wireless/wireless-links",
                   query={"interface_a_id": interfaces[("lab-sxt-a", "wlan1")]})["results"]
    values = {"interface_a": interfaces[("lab-sxt-a", "wlan1")],
              "interface_b": interfaces[("lab-sxt-b", "wlan1")], "ssid": "lab-link",
              "status": "connected", "auth_type": "wpa-personal", "auth_psk": psk,
              "tenant": tenant}
    if link:
        nb.call("PATCH", f"wireless/wireless-links/{link[0]['id']}", values)
    else:
        nb.call("POST", "wireless/wireless-links", values)
    devices = {d["name"]: d["id"] for d in nb.call("GET", "dcim/devices",
                                                   query={"limit": 1000})["results"]}
    group = nb.ensure("wireless/wireless-lan-groups", {"slug": WLAN_GROUP["name"]},
                      {"name": WLAN_GROUP["name"],
                       "custom_fields": {"controller": devices[WLAN_GROUP["controller"]]}})
    for ap in WLAN_GROUP["aps"]:
        nb.ensure("dcim/devices", {"name": ap}, {"custom_fields": {"wlan_group": group["id"]}})
    for wlan in WLAN_GROUP["wlans"]:
        nb.ensure("wireless/wireless-lans", {"ssid": wlan["ssid"]},
                  {"group": group["id"], "status": "active", "tenant": tenant,
                   "auth_type": "wpa-personal", "auth_psk": wlan["psk"],
                   "vlan": vlans[wlan["vlan"]] if "vlan" in wlan else None})
    print("lab modeled:", ", ".join(DEVICES))


if __name__ == "__main__":
    main()
