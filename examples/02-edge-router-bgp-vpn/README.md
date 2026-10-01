# 02 — Edge router with eBGP to VPN gateway

**Device role:** `router`, with a `bgp-peer`-tagged interface facing a VPN gateway.
**Demonstrates:** everything in scenario 01, plus a `/routing/bgp/connection` eBGP session to an
external VPN peer, redistributing OSPF/connected routes out and the VPN-learned routes back into
OSPF (`RoutingRedistribute::Bgp`) so the rest of the network learns them without an iBGP mesh.

**Why it matters — highest priority.** The completion plan's exact RouterOS 7
`/routing/bgp/connection` field names are an unverified best-effort sketch (see plan §5, risk 1).
This example's real config is the primary way to validate those field names before finalizing the
`mikrotik-model-generator/ros_model/routing/Bgp/*.txt` entity definitions in Phase 1. The network
already runs a mix of 2-byte and 4-byte ASNs in production — capture whichever this device
actually uses.

**Status:** populated (`config.rsc`, real `/export` from `router-faesigrund-keller`). Turned out to
be far richer than "a VPN eBGP peer" — the device is simultaneously a CAPsMAN controller, hosts
several OpenVPN servers/clients, GRE/IPsec/6to4/IPIPv6 tunnels, full firewall/NAT, and **three
separate `/routing/bgp/instance` with three different local ASNs**, several `/routing/bgp/template`
entries, and `/routing/filter/rule` chains per peer. The completion plan was revised accordingly
(simplified v1 BGP data model, auto-derived instances, no templates, optional filter pass-through)
— see plan §1.2/§2/§2bis/§2ter. VPN tunnels, firewall/NAT and the operational bits (scheduler,
netwatch, logging, user groups) are explicitly out of scope for the generator (plan §2ter).
