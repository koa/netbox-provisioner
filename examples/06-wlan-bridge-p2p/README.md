# 06 — Point-to-point WLAN bridge

**Device role:** `wlan-bridge`
**Demonstrates:** a transparently-bridged point-to-point wireless link between two sites (up to
~2km), carrying the site's full L2 fabric (household VLANs, VXLAN traffic) across the air gap
rather than being a managed CAPsMAN client.

**Why it matters — highest priority, highest risk.** Per the completion plan (§5, risk 2; Phase 6)
there is **no existing code at all** for this device role — no station/AP-bridge-mode config, no
security-profile pairing, nothing. This is the single most important example to fill in, since a
real, currently-working bridge pair is the only reliable source of truth for what RouterOS config
pattern actually achieves transparent L2 bridging over a wireless P2P link in this network
(`interface/wireless` vs. native `interface/wifi` with `WifiMode::StationBridge`, channel/frequency
locking, security profile matching between both ends, `datapath.bridge` wiring).

**Status:** TODO — needs a real `/export` from **both ends** of a currently-working production
P2P wireless bridge pair (the two ends' configs need to be read together to understand how they
associate). Redact WPA keys/serials, keep everything else intact.

`config.rsc` — not yet populated (should eventually hold both `a-end.rsc` and `b-end.rsc`, or
equivalent).
