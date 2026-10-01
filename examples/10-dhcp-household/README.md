# 10 — DHCP server for a household VLAN

**Device role:** `router` or `switch` hosting a DHCP server for one household's VLAN.
**Demonstrates:** `/ip/dhcp-server`, `/ip/dhcp-server/network`, `/ip/pool` with a pool range
automatically derived around already-reserved/static addresses within the household's prefix —
exercising the already-implemented `GapFinder<Ipv4Addr>` (`backend/src/device/ros/mod.rs`) against
real `IpPrefixData`/`IpRangeData`.

**Why it matters:** lower risk than 02/03/06 (the gap-finding logic is already written and
reasoned about), but still valuable as a concrete regression reference once Phase 3
(`setup_ip_addresses`) is unblocked.

**Status:** TODO — needs a real `/export` from a production device running DHCP for a household
VLAN, plus a note of which addresses in that prefix are statically reserved (so the expected pool
gaps are understandable from the example alone).

`config.rsc` — not yet populated.
