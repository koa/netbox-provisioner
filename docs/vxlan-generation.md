# VXLAN-Generierung

Code: `backend/src/device/ros/mod.rs` (`setup_wlan_ap`, `setup_vxlan`, `setup_l2`),
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

Es gibt zwei Codepfade, die VXLAN-Config erzeugen: den WLAN-AP-Pfad und den Port-Gruppen-Pfad.

## Pfad 1: WLAN-AP (`setup_wlan_ap`)

Wird nur aktiv, wenn `device.wlan_ap_of()` etwas liefert (das Gerät ist als Access Point einer
`WlanGroupAccess` eingetragen) **und** das Gerät eine primäre IPv4-Adresse hat (die als eigene
VTEP-Adresse dient). Dann:

1. Sammelt alle relevanten VLANs: die Management-VLAN der WLAN-Gruppe (`wlan_group.mgmt_vlan()`)
   plus die VLAN jedes einzelnen WLANs der Gruppe (`wlan.vlan()`), die eines haben.
2. Filtert daraus die VLANs, die überhaupt ein VXLAN zugeordnet haben (`vlan.vxlan()`) — eine VLAN
   ohne VXLAN-Zuordnung in NetBox bleibt rein lokal, bekommt keinen VXLAN-Transport.
3. Ruft `setup_vxlan` für jedes so gefundene, eindeutige VXLAN auf.

## `setup_vxlan`: was pro VXLAN generiert wird

Vorbedingung: Das VXLAN braucht sowohl einen **Namen** (aus `generate_vxlan_name`, siehe
[interface-naming.md](interface-naming.md)) als auch eine **VNI** (`vxlan.vni()`) — fehlt eines von
beiden, wird für dieses VXLAN nichts generiert (stiller Skip, kein Fehler).

Dann:

- Ein `/interface/vxlan`-Interface mit der gegebenen `vni`.
- Ein `/interface/bridge/port`-Eintrag, der dieses VXLAN-Interface an die feste
  Infrastruktur-Bridge `bridge-caps` hängt (siehe `CAPS_BRIDGE_NAME` in
  [interface-naming.md](interface-naming.md) — bewusst kein über `NameGenerator` erzeugter Name).
- Ein `/interface/bridge/vlan`-Eintrag auf `bridge-caps`, der das VXLAN-Interface als **getaggtes**
  Mitglied über den **gesamten** VLAN-Bereich `1..4094` einträgt — nicht nur die eine VLAN, die den
  Trigger auslöste. Grund: ein einzelnes VXLAN-Interface ist der gemeinsame Transport für
  potenziell mehrere VLANs derselben WLAN-Gruppe (Management- und mehrere Client-VLANs können
  dasselbe VXLAN teilen, siehe `generate_vxlan_name`s "genau eine WLAN-Gruppe"-Fall), daher muss es
  grundsätzlich alle VLAN-Tags durchlassen.
- VTEP-Einträge (`/interface/vxlan/vteps`) für jede **andere** IP aus `vxlan.vteps()` — die eigene
  IP wird herausgefiltert (`filter(|ip| ip != my_ip)`), ein Gerät trägt sich nicht selbst als sein
  eigenes Remote-VTEP ein.

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
    admit-only-vlan-tagged`. Das passt zur AP-/Controller-Seite, wo dasselbe VXLAN getaggt für
    `1-4094` auf `bridge-caps` hängt.
- Der Name kommt aus `NameGenerator::generate_vxlan_name` (produktiv: `vxlan-wlan-{gruppe}`, wenn
  alle WLANs des VXLAN zu einer WLAN-Gruppe gehören). Namen sind gerätelokal, für die
  Interoperabilität zählt nur die VNI.

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

**Abgrenzung zum AP-Pfad**: Hat `setup_wlan_ap` auf diesem Gerät für dieselbe VNI schon ein VXLAN
an `bridge-caps` gehängt, überspringt `setup_l2` dieses VXLAN, weil ein Interface nur Mitglied einer
Bridge sein kann. **Bekannte Lücke**: Ein AP, der zusätzlich einen eigenen LAN-Port für die Wohnung
an dasselbe VXLAN hängen will (z.B. `ether2` an einem cAP), wird dadurch nicht korrekt abgebildet.
Der Port landet ohne VXLAN in seiner Bridge. Das lässt sich erst sauber lösen, wenn die
WLAN-Generierung (siehe [wlan-generation.md](wlan-generation.md)) fertig ist und klar ist, ob
`bridge-caps` und die Haushalts-Bridge zusammengelegt werden.

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
