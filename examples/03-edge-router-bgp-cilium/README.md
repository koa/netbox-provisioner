# 03 — Edge router with eBGP to Cilium

**Device role:** `router`, with a `bgp-peer`-tagged interface facing a Cilium Kubernetes cluster's
BGP control plane.
**Demonstrates:** same eBGP/OSPF-redistribution pattern as scenario 02, but peering with Cilium
instead of a VPN gateway — worth keeping as a separate example since Cilium's BGP Control Plane
may expect different session parameters (e.g. multihop, specific timers) than a VPN gateway does.

**Why it matters:** second confirmation point (alongside scenario 02) for the real
`/routing/bgp/connection` field names, and revealed that this peer actually uses **iBGP**
(`local.role=ibgp`, same ASN both sides), not eBGP as originally assumed — corrected in the
completion plan (§2).

**Status:** populated (`config.rsc`, real `/export` from `klr-sw-3`). Notably this "edge router" is
actually a pure L2/L3-hw-offload **switch** (CRS309) that also speaks iBGP to three Kubernetes
nodes (`sr02`/`sr03`/`sr04`, presumably Cilium's BGP speakers) over both IPv4 and IPv6, with
`multihop=yes`. Good evidence that BGP capability should be gated purely by the presence of a
`bgp-peer`-tagged interface, not by device role (`router` vs `switch`) — matches the plan's design.
