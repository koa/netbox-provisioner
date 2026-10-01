# 08 — Newer AP under native WiFi/CAPsMAN

**Device role:** `wlan-ap` (newer hardware, `wifi-qcom` package), managed by a `wlan-controller`
device via the `wlan_group`/`controller` NetBox custom fields (not via the `role` field — see plan
§1.1).
**Demonstrates:** a CAP under the native `interface/wifi` stack (`interface/wifi/cap`,
`datapath`/`configuration`/`provisioning`), the modern equivalent of scenario 07.

**Why it matters:** `WifiDeviceData` is already modeled and has a live-ish code path
(`WifiDeviceDataTarget::generate_from`, currently still behind a `todo!()` for the mgmt-interface
lookup) — this example validates that path once Phase 2/3 unblock it.

**Status:** TODO — needs a real `/export` from a production newer-generation AP currently managed
via native WiFi/CAPsMAN.

`config.rsc` — not yet populated.
