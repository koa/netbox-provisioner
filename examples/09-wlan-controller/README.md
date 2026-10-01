# 09 — WLAN controller (combined with router role)

**Device role:** `router` that is also a WLAN controller (`controller` NetBox custom field on the
relevant Wireless LAN Group points at this device) — matches how it's typically deployed in the
live network (a controller is usually also a regular router, not a dedicated appliance).
**Demonstrates:** `/caps-man/manager` + `/caps-man/configuration`/`/datapath` (legacy) and
`/interface/wifi/capsman` + its `/configuration`/`/datapath` (native) side by side on one device
that's simultaneously doing normal router duties (OSPF, VLANs, etc.) for itself.

**Why it matters:** verifies the controller-side config generation composes correctly with the
rest of a router's config rather than only being tested in isolation.

**Status:** TODO — needs a real `/export` from a production device acting as a CAPsMAN/WiFi
controller.

`config.rsc` — not yet populated.
