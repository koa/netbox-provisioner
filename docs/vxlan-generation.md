# VXLAN-Generierung

Code: `backend/src/topology/fetch.rs` (VXLAN aus VLAN-Gruppe), `backend/src/topology/access/vlan.rs`
(`VlanAccess::devices`/`tunnel`), `backend/src/topology/access/vxlan.rs` (`VxlanAccess::vteps`),
`backend/src/device/ros/l2/mod.rs` (`L2Setup::new`, `check_l2_domain`),
`backend/src/device/ros/mod.rs` (`setup_l2`).

## NetBox-Modell

Ein VXLAN gehört zu einer **VLAN-Gruppe**: Hat sie im Custom Field `vxlan_vni` eine VNI, ergibt sie
genau ein VXLAN. Dessen Name ist der Gruppenname, und es ist ein **VLAN-Trunk**: Die VLANs der Gruppe
laufen darin **getaggt**, und das VLAN-Tag wählt das Netz aus. Ein Beispiel ist die Gruppe
`Berg-WLAN` mit VNI 1 und allen Haushalts-VLANs plus der Management-VLAN 99, siehe
`examples/09-wlan-controller`.

L2VPN-Objekte werden nicht mehr gelesen.

## Wann ein VLAN getunnelt wird

Ein VLAN wird getunnelt, wenn

1. seine VLAN-Gruppe eine VNI hat, und
2. es auf **mehr als einem Gerät** benutzt wird (`VlanAccess::tunnel`).

Ein Gerät benutzt ein VLAN (`VlanAccess::devices`), wenn

- eines seiner Interfaces es ungetaggt oder getaggt trägt (Port, Bridge, virtuelles Interface), oder
- es AP oder Controller einer WLAN-Gruppe ist, deren WLAN- oder Management-VLAN es ist.

Ein VLAN, das nur auf einem Gerät vorkommt, bleibt lokal, auch in einer Gruppe mit VNI. Ein VLAN
aus einer Gruppe ohne VNI bleibt immer lokal, auch auf mehreren Geräten. So lässt sich pro Gruppe
entscheiden, ob getunnelt wird.

Damit hängt ein Switch-Port im VLAN `wlan-mimi` automatisch im selben Netz wie das WLAN `wlan-mimi`:
Port und WLAN benutzen dasselbe VLAN-Objekt.

## Generierung

Für jedes getunnelte VLAN eines Geräts kommt ein `L2Port::VxLan` auf die root-lose Ebene. Von dort
landet es wie die CAP-/CAPsMAN-Einträge entweder in der modellierten Bridge, die dieses VLAN trägt,
oder auf der [Standard-Bridge](l2-generation.md#standard-bridge) (auf einem AP `bridge-caps`). Das
VXLAN wird nur Mitglied der VLANs, die das Gerät selbst benutzt.

`setup_l2` realisiert das:

- `/interface/vxlan` mit `vni` und `local-address` = Primary IPv4 des Geräts. Ohne Primary IPv4 gibt
  es `SetupError::MissingVtepAddress`, denn die Gegenstellen kennen das Gerät nur unter dieser
  Adresse.
- `/interface/vxlan/vteps` für jede Adresse aus `vxlan.vteps()` außer der eigenen.
- Das VXLAN wird **getaggtes** Mitglied jedes getunnelten VLANs (`/interface/bridge/vlan
  tagged=<vxlan>`), sein Bridge-Port bekommt `frame-types=admit-only-vlan-tagged`.
- Der Name kommt aus `NameGenerator::generate_vxlan_name` (produktiv `vxlan-{gruppenname}`, z.B.
  `vxlan-berg-wlan`). Namen sind gerätelokal, für die Interoperabilität zählt nur die VNI.

Das VXLAN wird bewusst pro VLAN eingetragen und nicht pauschal als Bereich `1-4094`: RouterOS lehnt
neben einem solchen Bereich jeden weiteren Bridge-VLAN-Eintrag derselben VLAN-ID ab (`vlan already
added`, geprüft gegen CHR 7.24.5). Lokale Access-Ports in einem getunnelten VLAN wären dann unmöglich.

Beispiel (Test `test_scenario_switch_vxlan_port_group`, `KeepNameGenerator`):

```
/interface/vxlan
add ... local-address="172.16.1.70" name=client-berg port=8472 vni=1 ...
/interface/vxlan/vteps
add interface=client-berg remote-ip="172.16.1.61"
/interface/bridge/vlan
add bridge=bridge tagged=client-berg untagged=ether2,ether3 vlan-ids=1
/interface/bridge/port
add interface=client-berg bridge=bridge frame-types=admit-only-vlan-tagged ingress-filtering=yes ...
add interface=ether2 bridge=bridge frame-types=admit-only-untagged-and-priority-tagged pvid=1 ...
```

## VTEP-Liste (`VxlanAccess::vteps`)

Die Primary IPv4 jedes Geräts, das eines der getunnelten VLANs der Gruppe benutzt. Jeder Teilnehmer
kennt also jeden anderen (Full Mesh mit Head-End-Replication), ohne dass das irgendwo gepflegt
werden muss.

Broadcasts eines VLANs gehen damit an alle VTEPs der Gruppe, auch an solche, die das VLAN nicht
benutzen; deren Bridge verwirft sie. Wird das zu viel, teilt man die Gruppe auf.

## Was abgelehnt wird

Diese Fälle würden nach dem Ausrollen nur als Ausfall auffallen. Deshalb wird für das Gerät
**nichts** generiert, sondern ein Fehler gemeldet (`check_l2_domain`, `L2Setup::new`). Die L2-Domäne
eines getunnelten VLANs sind alle Geräte, die es benutzen, die eines lokalen VLANs nur das Gerät
selbst. Ein Konflikt blockiert deshalb jedes Gerät der Domäne, auch APs, die das VLAN nur als WLAN
führen.

- **Kabel und Tunnel** (`VlanOnCableAndTunnel`): Ein getunneltes VLAN läuft zusätzlich über ein Kabel
  zu einem anderen Gerät, das es ebenfalls trägt. Das ergäbe eine L2-Schleife. Entweder das VLAN in
  eine Gruppe ohne VNI legen oder das Kabel ohne dieses VLAN modellieren.
- **Mehrere Netze** (`MultipleNetworksOnVlan`): Die Interfaces mit diesem Untagged VLAN haben über
  die ganze Domäne Adressen aus mehr als einem IPv4- bzw. IPv6-Netz. Mehrere Adressen desselben
  Netzes (Gateway plus Adresse eines Switches) sind in Ordnung.
- **Mehrere DHCP-Server** (`MultipleDhcpServersOnVlan`): Mehr als ein Interface mit diesem Untagged
  VLAN hat das Tag `dhcp`.
- **VXLAN in zwei Bridges** (`PortInMultipleBridges`): Zwei getunnelte VLANs derselben Gruppe liegen
  auf einem Gerät in verschiedenen Bridges. Das VXLAN kann nur Mitglied einer Bridge sein.
