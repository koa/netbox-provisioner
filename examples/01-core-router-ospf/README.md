# 01 — Core router, OSPF only

**Device role:** `router`
**Demonstrates:** plain internal L3 router — ethernet interfaces, bridge/VLAN for locally
terminated household segments, loopback IP, OSPF participation (area `0.0.0.0`, router-id from
loopback) for internal reachability. No BGP, no CAPsMAN/WiFi, no VXLAN.

**Why it matters:** this is the baseline case every other scenario adds on top of — Phase 2/3 of
the completion plan (`L2Setup` + `setup_ospf`) should produce correct output for this scenario
before anything more advanced.

**Status:** TODO — needs a real `/export` from a production core router with no BGP/WLAN role.
Redact passwords, WPA PSKs and serial numbers; note in this file what was stripped.

`config.rsc` — not yet populated.
