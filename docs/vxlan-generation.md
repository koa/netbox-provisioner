# VXLAN-Generierung

Code: `backend/src/device/ros/mod.rs` (`setup_l2`),
`backend/src/device/ros/l2/mod.rs` (`L2Setup::new`), `backend/src/topology/access/vxlan.rs`
(`VxlanAccess`), `backend/src/topology/access/interface.rs` (`vxlan_terminations`).

## NetBox-Modell

Ein VXLAN ist in NetBox ein **L2VPN vom Typ `vxlan`**; dessen `identifier` ist die VNI. Ein L2VPN
kann zwei Arten von Terminierungen haben, und beide werden ausgewertet:

- **VLAN-Terminierung**: "diese VLAN wird über dieses VXLAN transportiert". In diesem Netz trägt ein
  VXLAN typischerweise **mehrere** VLANs gleichzeitig (z.B. `vxlan-client-berg`, VNI 1, mit allen
  Haushalts-VLANs 1–100 und der Management-VLAN 99, siehe `examples/09-wlan-controller`). Das VXLAN
  ist also ein **VLAN-Trunk**: die Frames laufen darin **getaggt**, und das VLAN-Tag wählt den
  Haushalt aus.
- **Interface-Terminierung**: "dieser Port nimmt an diesem VXLAN teil". Das ist die explizite
  Zuweisung von Switch-Ports (siehe unten).

Beide Fälle laufen durch denselben Mechanismus: `L2Setup::new` plant ein `L2Port::VxLan` pro
VLAN, und `setup_l2` realisiert es (siehe unten). Unterschiedlich ist nur, **wie** ein Gerät zu
einem VXLAN kommt: als WLAN-Access-Point oder über eine Interface-Terminierung.

## Pfad 1: WLAN-AP

Ist das Gerät Access Point einer WLAN-Gruppe (`device.wlan_ap_of()`), nimmt es an jedem VXLAN teil,
auf dem die Management-VLAN der Gruppe oder die VLAN eines ihrer WLANs liegt (`vlan.vxlan()`). Eine
VLAN ohne VXLAN-Zuordnung bleibt rein lokal.

Für jedes so gefundene VXLAN kommt in **jede** seiner VLAN-Terminierungen ein `L2Port::VxLan` auf
die root-lose Ebene, also auf die [Standard-Bridge](l2-generation.md#standard-bridge). Auf einem AP
heißt die `bridge-caps`, dieselbe Bridge, die der CAP-Datapath verwendet. Das VXLAN ist damit
getaggtes Mitglied jeder transportierten VLAN, wie im Referenz-Export
`examples/08-wlan-ap-wifi-new` (`tagged=vxlan-client-berg vlan-ids=1-20,99`).

Ein LAN-Port des APs, der die Haushalts-VLAN ungetaggt trägt und keine Bridge nennt (z.B.
`ap-mimi-buero` `ether2`–`ether5`), landet auf derselben Standard-Bridge. Er liegt damit ohne
weitere Modellierung im selben L2 wie das WLAN und das VXLAN.

Das VXLAN wird bewusst pro VLAN eingetragen und nicht pauschal als Bereich `1-4094`: RouterOS lehnt
neben einem solchen Bereich jeden weiteren Bridge-VLAN-Eintrag derselben VLAN-ID ab (`vlan already
added`, geprüft gegen CHR 7.24.5). Lokale Access-Ports in einer Haushalts-VLAN wären dann unmöglich.

## Pfad 2: Port-Gruppen auf Switches (Interface-Terminierung)

Anwendungsfall: Eine Wohnung hat ein WLAN, dessen Haushalts-VLAN per VXLAN transportiert wird. Ein
Bewohner steckt einen Drucker an eine LAN-Dose, die auf einem Switch im Keller endet, und der
Drucker soll trotzdem im selben L2-Netz wie sein WLAN liegen.

**Modellierung in NetBox**: die Ports der Wohnung (eine "Port-Gruppe") werden

1. wie jeder Access-Port mit ihrer **ungetaggten VLAN** = Haushalts-VLAN versehen (das wählt den
   Haushalt innerhalb des VXLAN-Trunks aus), und
2. zusätzlich als **Interface-Terminierung** an das L2VPN/VXLAN gehängt.

Statt einzelner Ports kann man auch ein VLAN-Interface (ein virtuelles Interface ohne externen
Port, nur mit `vlan`) terminieren. Dann nimmt diese VLAN auf diesem Gerät als Ganzes am VXLAN teil.
Ein terminiertes Interface **ohne** VLAN ist ein Fehler (`SetupError::VxlanTerminationWithoutVlan`),
weil sich ohne VLAN nicht bestimmen lässt, welcher Haushalt innerhalb des Trunks gemeint ist.

Die explizite Interface-Terminierung (statt "jedes Gerät, das die VLAN irgendwo trägt") ist eine
bewusste Entscheidung. Sonst würde auch jeder Transit-Switch, der die Haushalts-VLAN nur physisch
weiterreicht, zum VTEP, und damit entstünde eine L2-Schleife über VXLAN und das physische Kabel
zugleich.

**Generierung**:

- `L2Setup::new` hängt für jedes terminierte Interface einen `L2Port::VxLan` auf **dieselbe
  Ebene wie das Interface selbst**, also `(Root des Interfaces, VLAN)` für jede VLAN, die es trägt
  (ungetaggt und getaggt), nicht auf eine root-lose Ebene. Dadurch landet das VXLAN in derselben
  Bridge wie die Ports, und diese Bridge wird zwingend eine `TaggedBridge` (siehe
  [l2-generation.md](l2-generation.md)).
- `setup_l2` realisiert das:
  - `/interface/vxlan` mit `vni` und `local-address` = primäre IPv4 des Geräts. Ohne primäre IPv4
    gibt es `SetupError::MissingVtepAddress`, denn die Gegenstellen kennen das Gerät nur unter
    dieser Adresse als VTEP.
  - `/interface/vxlan/vteps` für jede Adresse aus `vxlan.vteps()` außer der eigenen.
  - Das VXLAN-Interface wird **getaggtes** Mitglied der VLAN (`/interface/bridge/vlan
    tagged=<vxlan> untagged=<ports>`), und sein Bridge-Port bekommt `frame-types =
    admit-only-vlan-tagged`. Das passt zur AP-/Controller-Seite, wo dasselbe VXLAN ebenfalls
    getaggt in den Haushalts-VLANs hängt.
- Der Name kommt aus `NameGenerator::generate_vxlan_name` (produktiv: `vxlan-{l2vpn-name}`, z.B.
  `vxlan-client-berg`). Namen sind gerätelokal, für die Interoperabilität zählt nur die VNI.

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

Auf einem AP braucht es für LAN-Ports keine Interface-Terminierung: Pfad 1 hängt das VXLAN bereits
an alle WLAN-VLANs der Standard-Bridge, es reicht die VLAN am Port.

## Wo die VTEP-Adressliste herkommt (`VxlanAccess::vteps`)

Nicht direkt aus einer NetBox-Liste, sondern abgeleitet: die Vereinigung aus

1. den primären IPv4-Adressen aller Geräte, deren Interface direkt am VXLAN terminiert ist
   (`interface_terminations()` → `InterfaceAccess::device()`), und
2. den primären IPv4-Adressen aller **Controller und Access Points** jeder WLAN-Gruppe, deren WLANs
   über eine der VXLAN-VLAN-Terminierungen laufen (`vlan_terminations()` → `wlan()` →
   `wlan_group()` → `aps()`/`controller()`).

Das bedeutet: Ein Access Point muss nicht selbst explizit als VXLAN-Interface-Terminierung in NetBox
eingetragen sein — es reicht, dass er Mitglied einer WLAN-Gruppe ist, deren WLAN/Management-VLAN auf
dieses VXLAN zeigt. Die komplette Full-Mesh-VTEP-Liste (jeder AP kennt jeden anderen AP plus den
Controller) wird daraus implizit abgeleitet.

Ein Switch mit terminierter Port-Gruppe (Pfad 2) ist über Punkt 1 automatisch in dieser Liste. Die
APs und alle anderen Switches derselben VXLAN tragen ihn also ohne weitere Pflege als Remote-VTEP
ein, und der Switch umgekehrt alle anderen.
