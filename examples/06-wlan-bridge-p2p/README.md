# 06 — Point-to-point WLAN bridge

**Device role:** `wlan-bridge`
**Demonstrates:** a transparently-bridged point-to-point wireless link between two sites (up to
~2km), carrying the site's full L2 fabric (household VLANs, VXLAN traffic) across the air gap
rather than being a managed CAPsMAN client.

**Why it matters — highest priority, highest risk.** Per the completion plan there was **no
existing code at all** for this device role. These two examples turned out to reveal two
structurally different RouterOS interface types depending on radio technology, neither previously
modeled in `mikrotik-model`: legacy `/interface/wireless` with MikroTik's proprietary Nv2/NSTREME
protocol for the 5GHz dish link, and the dedicated `/interface/w60g` stanza for the 60GHz link —
see plan Phase 6 for the field-level breakdown.

**Status:** populated — `config-5ghz.rsc` (`bachmann-hegglin-2`, RBDynaDishG-5HacDr3,
`mode=station-bridge`, `wireless-protocol=nv2-nstreme-802.11`) and `config-60ghz.rsc`
(`schopf-hegglin`, RBwAPG-60ad-A, `/interface/w60g`, `mode=station-bridge`). Both confirm the
transparent-bridge design (simple non-VLAN-filtering bridge, radio port + one ethernet port) and
both use `/routing/bfd/configuration` for fast link-failure detection. Each file currently shows
only one end of its link — the other end of each pair would help confirm how the two sides'
SSID/security settings must match (open question, plan §5 risk 2).
