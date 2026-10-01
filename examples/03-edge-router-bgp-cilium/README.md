# 03 — Edge router with eBGP to Cilium

**Device role:** `router`, with a `bgp-peer`-tagged interface facing a Cilium Kubernetes cluster's
BGP control plane.
**Demonstrates:** same eBGP/OSPF-redistribution pattern as scenario 02, but peering with Cilium
instead of a VPN gateway — worth keeping as a separate example since Cilium's BGP Control Plane
may expect different session parameters (e.g. multihop, specific timers) than a VPN gateway does.

**Why it matters:** second confirmation point (alongside scenario 02) for the unverified
`/routing/bgp/connection` field names in the completion plan, and for whether `bgp_multihop`
needs to be true for this particular peer.

**Status:** TODO — needs a real `/export` (or `/routing/bgp/connection print detail`) from a
production device peering with the Cilium cluster, redacted as needed.

`config.rsc` — not yet populated.
