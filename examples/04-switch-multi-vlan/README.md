# 04 — Pure switch, multiple household VLANs

**Device role:** `switch`
**Demonstrates:** L2-only device trunking several household VLANs across multiple ports (a mix of
tagged and untagged), bridge/VLAN filtering — but per the plan's §1.1 decision, switches still get
a loopback IP and OSPF participation for their own management reachability, even though they do
no L3 routing for end-user traffic.

**Why it matters:** the main real-world exercise for `L2Setup`'s `TaggedBridge`/`SimpleBridge`
grouping logic (Phase 2) — a switch with several VLANs spread across ports is exactly the case the
`todo!()` in `L2Setup::new()` needs to handle correctly.

**Status:** TODO — needs a real `/export` from a production access/distribution switch carrying
multiple household VLANs.

`config.rsc` — not yet populated.
