# Example RouterOS configurations

Reference configurations covering every technique the provisioner needs to support: CAPsMAN,
native `interface/wifi`, HW L3 routing, OSPF (internal), BGP (external only), VXLAN (end-user
transport), across the device roles `router` / `switch` / `wlan-bridge` / `wlan-ap` /
`wlan-controller`.

**These are not a spec to reproduce 1:1.** The generator's output does not need to match a
`.rsc` file here byte-for-byte — it needs to keep providing the *functionality* the example
demonstrates. Where possible, an example's `config.rsc` should be a real export (`/export`) from
a currently-running production device, lightly redacted (strip passwords/PSKs/serials, leave a
note of what was removed) rather than a hand-invented one — real configs surface real edge cases
hand-written examples would miss. Each scenario directory's `README.md` says whether that's done
yet.

These also double as the basis for the golden-file tests described in the completion plan
(`backend/src/device/ros/*`): once the generator can produce output for a scenario, its rendered
script is compared against the reference here for *behavioral* equivalence, not textual diff.

## Scenarios

| # | Scenario | Demonstrates |
|---|---|---|
| 01 | [core-router-ospf](01-core-router-ospf/) | Plain internal router, OSPF underlay only, no BGP/WLAN |
| 02 | [edge-router-bgp-vpn](02-edge-router-bgp-vpn/) | Router with an eBGP peer to a VPN gateway, redistributed into OSPF |
| 03 | [edge-router-bgp-cilium](03-edge-router-bgp-cilium/) | Router with an eBGP peer to a Cilium cluster |
| 04 | [switch-multi-vlan](04-switch-multi-vlan/) | Pure L2 switch, multiple household VLANs, still gets loopback+OSPF |
| 05 | [vxlan-household-transport](05-vxlan-household-transport/) | Household VLANs carried across sites via VXLAN (VTEPs) |
| 06 | [wlan-bridge-p2p](06-wlan-bridge-p2p/) | Point-to-point wireless inter-site link, transparently bridged |
| 07 | [wlan-ap-capsman-legacy](07-wlan-ap-capsman-legacy/) | Older AP managed via CAPsMAN |
| 08 | [wlan-ap-wifi-new](08-wlan-ap-wifi-new/) | Newer AP managed via native `interface/wifi` |
| 09 | [wlan-controller](09-wlan-controller/) | CAPsMAN + WiFi-CAPsMAN controller, combined with the router role |
| 10 | [dhcp-household](10-dhcp-household/) | DHCP server/pool for a household VLAN, auto-derived free-range |

Scenarios 02, 03 and 06 are the highest priority to fill with real exports first — they cover the
areas flagged as the biggest open risks in the completion plan (exact RouterOS BGP field names,
and the wireless point-to-point bridge config pattern, which has no existing code at all).
