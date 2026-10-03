# IP-Adressen, DHCP und OSPF-Generierung

Code: `backend/src/device/ros/mod.rs` (`setup_ip_addresses`, `setup_ospf`, `GapFinder`).

Beide Schritte laufen über `L2Setup::logical_interfaces()` (siehe
[l2-generation.md](l2-generation.md)) und sind dadurch komplett unabhängig davon, ob eine logische
Schnittstelle ein Einzelport, eine flache Bridge oder eine VLAN-Ebene einer getaggten Bridge ist.

## IP-Adressen (`setup_ip_addresses`)

Für jede logische Schnittstelle:

- **Keine IP-Adressen** in NetBox hinterlegt, aber `enable_dhcp_client` gesetzt → ein
  `/ip/dhcp-client`-Eintrag wird angelegt (`add_default_route = yes`,
  `default_route_distance = 10`, `use_peer_dns = true`, `use_peer_ntp = false`).
- **Eine oder mehrere IP-Adressen** hinterlegt → jede wird per `/ip/address` bzw.
  `/ipv6/address` gesetzt. Ist zusätzlich `enable_dhcp_server` auf dieser Schnittstelle gesetzt,
  wird zusätzlich pro **IPv4**-Adresse (nicht IPv6 — DHCPv6-Server-Generierung ist nicht
  implementiert) ein DHCP-Server-Setup berechnet (siehe unten).

### DHCP-Server-Range-Berechnung (`GapFinder`)

Ein DHCP-Server braucht einen Adressbereich, der nicht mit bereits anderweitig vergebenen Adressen
kollidiert. `GapFinder<V: Ord + Copy>` ist ein generischer Helfer, der "reservierte" Intervalle
sammelt (`reserve_ipv4_range`/`reserve_ipv4_net`/`reserve_ipv4`) und anschließend die **Lücken**
innerhalb eines gegebenen Gesamtbereichs als `Range<V>`-Iterator zurückgibt
(`find_gaps_ipv4`/`gaps`) — Implementierung über eine sortierte Map von ±1-Zählern an den
Intervallgrenzen (klassischer Sweep-Line-Ansatz), nicht über explizite Bitmaps.

Die Quelle der Range ist:
- Hat das zugehörige NetBox-Prefix **explizit als DHCP markierte Ranges**
  (`prefix.ranges().filter(is_dhcp)`), werden genau die verwendet.
- Sonst wird die **gesamte restliche Lücke** im Prefix berechnet: alle Kind-Prefixe, alle bereits
  vergebenen IPs im Prefix und alle explizit nicht-DHCP-Ranges werden reserviert, der Rest des
  Adressraums (abzüglich Netz-/Broadcast-Adresse) wird als DHCP-Pool verwendet.

Der Server-Name ist `dhcp-{if_name}` für die erste IP auf einer Schnittstelle, `dhcp-{if_name}-{n}`
ab der zweiten (`ip_idx > 0`) — relevant, falls eine Schnittstelle mehrere IPv4-Adressen (und damit
potenziell mehrere DHCP-Server) trägt. Gateway und DNS-Server der erzeugten
`/ip/dhcp-server/network`werden beide auf die eigene Router-IP gesetzt.

## OSPF (`setup_ospf`)

Voraussetzung: Das Gerät hat eine `primary_ip_v4` (wird als `router-id` verwendet). Ohne das wird
gar kein OSPF generiert.

1. Alle logischen Schnittstellen mit `use_ospf == true` werden gesammelt (`BTreeSet`, dedupliziert
   und sortiert nach Namen — Determinismus der generierten Config).
2. Ist diese Menge **leer**, wird überhaupt kein OSPF-Config-Objekt angelegt.
3. Sonst werden **immer beide** Instanzen erzeugt, unabhängig davon, ob tatsächlich IPv6 im Spiel
   ist:
   - `default-v2` (`RoutingOspfInstanceVersion::_2`) mit Area `backbone-v2`.
   - `default-v3` (`RoutingOspfInstanceVersion::_3`) mit Area `backbone-v3`.
   Beide Instanzen bekommen `router_id` = die eigene primäre IPv4-Adresse (auch die v3/OSPFv3-
   Instanz — RouterOS verlangt für OSPFv3 ebenfalls eine IPv4-formatierte Router-ID) und
   `redistribute = {Connected, Static}`.
4. Beide Areas bekommen ein `routing/ospf/interface-template`, das **dieselbe** Menge von
   Schnittstellen auf beiden Instanzen aktiviert (`use_bfd = false` auf beiden). Es gibt keine
   Unterscheidung, welche Schnittstellen tatsächlich IPv6-fähig sind — `default-v3` wird also auch
   auf rein-IPv4-Netzen mit aktiviert.

**Praktische Konsequenz** (aus der Multi-CHR-Testharness-Erfahrung, siehe
`backend/tests/chr_network/wait.rs`): Ein reines IPv4-Testszenario muss beim Prüfen auf
OSPF-Nachbarschaft explizit auf die `default-v2`-Instanz warten, nicht auf "irgendeine" — die
`default-v3`-Nachbarschaft kann unter bestimmten Umständen (z.B. IPv6-Link-Local-Timing in
virtualisierten Testumgebungen) in `Init` hängen bleiben, ohne dass das etwas mit dem eigentlichen
IPv4-Szenario zu tun hat.
