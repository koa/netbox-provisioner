# 07 — Legacy AP under CAPsMAN

**Device role:** `wlan-ap` (older hardware), managed by a `wlan-controller` device via the
`wlan_group`/`controller` NetBox custom fields (not via the `role` field — see plan §1.1).
**Demonstrates:** a CAP (`interface/wireless/cap`) connecting to a CAPsMAN manager, picking up its
SSID/VLAN/security config from the controller rather than local config.

**Why it matters:** `WirelessDeviceData`/`setup_wlan_ap` already has a live code path for this
(driven by `device.wlan_ap_of()`) — this example is mainly a regression/verification reference,
lower risk than 02/03/06.

**Status:** TODO — needs a real `/export` from a production legacy AP currently managed by
CAPsMAN.

`config.rsc` — not yet populated.
