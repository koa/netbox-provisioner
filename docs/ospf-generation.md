# IP-Adressen, DHCP und OSPF-Generierung

Code: `backend/src/device/ros/mod.rs` (`setup_ip_addresses`, `setup_ospf`), `device/ros/dhcp.rs`
(`GapFinder`).

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

Der Server-Name ist `dhcp-{if_name}` für die erste IPv4-Adresse auf einer Schnittstelle,
`dhcp-{if_name}-{n}` ab der zweiten — relevant, falls eine Schnittstelle mehrere IPv4-Adressen (und
damit potenziell mehrere DHCP-Server) trägt. Gezählt wird nur unter den IPv4-Adressen: eine
IPv6-Adresse, die in NetBox davor steht (etwa nach einem Rückgängig neu angelegt), benennt den
Server nicht um. Gateway und DNS-Server der erzeugten
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
   `redistribute = {Connected, Static, Bgp}`: BGP ist von Hand konfiguriert (klr-sw-3,
   `examples/` 01 bis 03), seine Routen erreichen das übrige Netz nur so; ohne BGP bringt es
   nichts. Eine Default-Route geht über `redistribute` nicht hinaus (CHR 7.24.5: eine statische
   `0.0.0.0/0` wird mit `redistribute=static` keine externe LSA), nur über `originate_default`.
   `originate_default` ist `IfInstalled` nur an Geräten
   mit dem Custom Field `propagate_default_route`, sonst `Never` (auch die Default-Route vom
   DHCP-Client eines Uplinks oder eine statische geht dann nicht hinaus; andere statische Routen
   schon). RouterOS' eigener Standard wäre `if-installed`: ein Lab mit Uplink ins produktive Netz
   hat so dessen Default-Route wieder dorthin exportiert.
4. Beide Areas bekommen ein `routing/ospf/interface-template`, das **dieselbe** Menge von
   Schnittstellen auf beiden Instanzen aktiviert (`use_bfd = false` auf beiden). Es gibt keine
   Unterscheidung, welche Schnittstellen tatsächlich IPv6-fähig sind — `default-v3` wird also auch
   auf rein-IPv4-Netzen mit aktiviert.
5. IPv6 ist auf jedem Gerät eingeschaltet (`/ipv6/settings disable-ipv6=no`, der Werkszustand).
   Mit abgeschaltetem IPv6 sind die IPv6-Adressen `invalid` und `default-v3` findet keinen
   Nachbarn, obwohl Adressen und Instanz angewendet aussehen (so auf lab-ap1).

**Praktische Konsequenz** (aus der Multi-CHR-Testharness-Erfahrung, siehe
`backend/tests/chr_network/wait.rs`): Ein reines IPv4-Testszenario muss beim Prüfen auf
OSPF-Nachbarschaft explizit auf die `default-v2`-Instanz warten, nicht auf "irgendeine" — die
`default-v3`-Nachbarschaft kann unter bestimmten Umständen (z.B. IPv6-Link-Local-Timing in
virtualisierten Testumgebungen) in `Init` hängen bleiben, ohne dass das etwas mit dem eigentlichen
IPv4-Szenario zu tun hat.

### MTU

RouterOS 7 bildet eine Nachbarschaft nur zwischen gleichen Interface-MTUs (bei 1500 gegen 2000
bleibt sie in ExStart/Exchange, auf einer CHR 7.24.5 nachgestellt) und kennt kein `mtu-ignore`.
Die Generierung verlangt deshalb an beiden Enden eines Kabels dieselbe MTU aus NetBox
(`port_mtu`, `SetupError::MtuMismatch`), siehe [netbox-modeling.md](netbox-modeling.md#mtu).
