# 05 — VXLAN household transport

**Device role:** `router` (or `switch`) terminating VXLAN VTEPs.
**Demonstrates:** a household VLAN extended across sites via VXLAN — bridge-port/bridge-vlan
entries for the VXLAN interface, static VTEP peer(s) (per the plan's decision: static VTEPs, no
EVPN control plane needed at this scale), VNI assignment.

**Why it matters:** the live code already has a working `setup_vxlan` path (driven today via the
WLAN-management VXLAN use case) — this example validates the same mechanism generalizes correctly
to plain household-VLAN transport, not just WLAN mgmt traffic.

**Status:** TODO — needs a real `/export` from a production device with an active VXLAN
household-VLAN extension (`/interface/vxlan`, `/interface/vxlan/vteps`, relevant bridge-vlan
entries).

`config.rsc` — not yet populated.
