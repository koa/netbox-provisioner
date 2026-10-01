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

**Status:** TODO — needs a real `/export` (or at minimum `/routing/bgp/connection print detail`
plus the relevant `/routing/ospf/instance print detail`) from a production device that currently
peers with a VPN gateway. Redact the peer address/AS if sensitive, but keep the field *names* and
structure intact — that's the point of this example.

`config.rsc` — not yet populated.
