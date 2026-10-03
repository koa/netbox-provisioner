# L2-Generierung (Bridges, VLANs, Ports)

Code: `backend/src/device/ros/l2/mod.rs` (`L2Setup::new`, `L2Setup::logical_interfaces`),
`backend/src/device/ros/mod.rs::setup_l2` (Realisierung als RouterOS-Config).

Das ist der komplexeste und zentralste Teil der Generierungs-Pipeline — fast alles andere
(IP-Adressen, DHCP, OSPF, VXLAN/WLAN-Anbindung) baut auf dem Ergebnis von `L2Setup::new` auf.

## Schritt 1: "Ebenen" (`planes`) aus der Topologie sammeln

Für jedes Interface eines Geräts wird bestimmt, zu welcher **Root** (dem physischen Gerät, dessen
Bridge-Mitglied es ist — bzw. es selbst, falls es kein Bridge-Member ist) und welcher **VLAN** (oder
"kein VLAN") es gehört. Diese `(root, vlan)`-Paare heißen hier "Ebenen" (`planes:
BTreeMap<(Option<InterfaceId>, Option<VlanId>), BTreeSet<L2Port>>`).

Pro physischem Interface mit externem Port:
- Jede **getaggte VLAN** (`tagged_vlans()`) erzeugt einen `L2Port::TaggedEthernet`-Eintrag auf der
  Ebene `(root, Some(vlan))`.
- Eine **ungetaggte VLAN** (`untagged_vlan()`) erzeugt einen `L2Port::UntaggedEthernet`-Eintrag,
  ebenso auf `(root, Some(vlan))`.
- Hat das Interface **weder** getaggte noch ungetaggte VLAN, landet es als
  `L2Port::UntaggedEthernet` auf der "kein VLAN"-Ebene `(root, None)`.

Eigene IP-Adressen eines Interfaces (`interface.ips()`) werden als `L2Port::Ip(...)` auf **derselben
VLAN-Ebene** eingeordnet, zu der das Interface selbst über seine eigene `untagged_vlan()` gehört —
nicht pauschal auf die "kein VLAN"-Ebene. Das ist, was eine rein virtuelle "VLAN-Schnittstelle" in
NetBox (ein Interface ohne externen Port, nur mit `vlan` + `ips`) erlaubt, ihre L3-Adresse korrekt
auf dieser VLAN-Ebene zu tragen.

Ist ein Interface direkt an einem VXLAN terminiert (Port-Gruppe, siehe
[vxlan-generation.md](vxlan-generation.md#pfad-2-port-gruppen-auf-switches-interface-terminierung)),
kommt für jede VLAN, die es trägt, ein `L2Port::VxLan` auf **seine eigene** Ebene `(root, vlan)`.

VXLAN- und WLAN-Anbindungen aus dem WLAN-Kontext haben dagegen **kein** physisches Root-Device und
werden daher auf root-losen Ebenen `(None, Some(vlan))` einsortiert (`L2Port::VxLan`,
`L2Port::CapsMan`, `L2Port::Cap`, `L2Port::CapsManMgmt`, `L2Port::CapMgmt` — siehe
[vxlan-generation.md](vxlan-generation.md) und [wlan-generation.md](wlan-generation.md)).

## Schritt 2: Mehrdeutigkeits-Check

Bevor weitergemacht wird, werden alle Ebenen nach ihrer **echten numerischen** NetBox-VLAN-ID
gruppiert. Taucht dieselbe numerische VLAN-ID in zwei unabhängigen `(root, vlan)`-Schlüsseln auf
(z.B. weil zwei verschiedene NetBox-VLAN-Objekte zufällig dieselbe Nummer tragen, oder dieselbe
VLAN auf zwei unterschiedlichen Root-Gruppen referenziert wird, ohne dass das beabsichtigt wäre),
bricht die Generierung mit `SetupError::AmbiguousVlanIds` ab, statt stillschweigend eine der beiden
Bedeutungen zu verlieren.

## Schritt 3: Nach Root gruppieren, virtuelle Ebenen zusammenführen

Alle Ebenen werden nach Root umgruppiert (`by_root: Root -> VLAN -> Ports`). Root-lose Ebenen
(VXLAN/WLAN ohne physische Uplink-Ports) werden anschließend **eingemischt**: Hat eine physische
Root-Gruppe bereits dieselbe VLAN, werden die virtuellen Ports (VXLAN/CAPsMan/...) dort
hinzugefügt. Gibt es keine passende physische Gruppe, bleibt die Ebene root-los und wird trotzdem
später als eigene `TaggedBridge` realisiert — einfach eine ohne physisches Mitglied (z.B. eine VLAN,
die nur per VXLAN erreichbar ist).

## Schritt 4: Klassifizierung pro Root — welche der drei L2-Domänen-Arten?

Für jede Root-Gruppe wird genau einer von drei Fällen gewählt:

1. **`single_ports`** (ein `SingleInterfaceEntry`, keine Bridge in RouterOS):
   Bedingung: die Gruppe hat ein physisches Root-Device, besteht aus **genau einer** Ebene (der
   "kein VLAN"-Ebene), **keine** VXLAN/CAPsMan/Cap-Anbindung, und enthält **genau einen**
   `UntaggedEthernet`-Port. Das deckt den Standardfall "ein einzelner, nicht gebridgeter
   Access-Port" ab, ohne unnötig eine RouterOS-Bridge-Schnittstelle anzulegen.
2. **`simple_bridges`** (eine `/interface/bridge` **ohne** VLAN-Filtering):
   Gleiche Bedingung wie oben, aber **mehr als ein** physischer Port auf der einzigen Ebene (reines
   "mehrere Ports, eine flache L2-Domäne"-Bridging, kein VLAN-Tagging nötig).
3. **`tagged_bridges`** (eine `/interface/bridge` **mit** VLAN-Filtering, `protocol_mode = Mstp`):
   In allen übrigen Fällen — insbesondere sobald **mehr als eine** VLAN-Ebene vorkommt, oder
   **irgendeine** VXLAN-/CAPsMan-/Cap-Anbindung im Spiel ist, selbst wenn es nur eine VLAN-Ebene
   wäre. Diese Variante ist die allgemeinste und teuerste (RouterOS-seitig mehr Konfiguration), wird
   also bewusst nur verwendet, wenn die beiden einfacheren Fälle nicht ausreichen.

Jede Ebene innerhalb einer `TaggedBridge` bekommt dabei ihre eigene **numerische VLAN-ID**
zugewiesen — entweder die echte NetBox-ID, oder, falls keine vorhanden (z.B. die "kein
VLAN"-Ebene einer sonst getaggten Bridge), eine **synthetische** ID ab `60000` aufwärts
(`next_synthetic_vlan_id`, kollisionsfrei gegenüber allen in Schritt 2 gesammelten echten IDs).

Der Interface-Name einer VLAN-Ebene innerhalb einer getaggten Bridge ist:
- der **Bridge-Name selbst**, wenn es die "kein VLAN"/native Ebene ist (keine eigene
  `/interface/vlan`-Schnittstelle nötig — IP-Adressen auf der nativen VLAN sitzen direkt auf dem
  Bridge-Interface),
- sonst ein **dediziertes `/interface/vlan`**-Sub-Interface, benannt über
  `NameGenerator::generate_vlan_name` (siehe [interface-naming.md](interface-naming.md)).

Das ist bewusst so, weil ein Bridge-Mitgliedsport in RouterOS nie selbst eine IP tragen kann, sobald
er versklavt ist — die L3-Präsenz einer VLAN muss daher immer entweder auf der Bridge selbst (native
VLAN) oder auf einer eigenen VLAN-Sub-Schnittstelle sitzen, nie auf dem Namen eines einzelnen
Mitgliedsports, egal wie viele physische Ports diese VLAN tragen.

## Schritt 5: Realisierung als RouterOS-Config (`BaseDeviceDataTarget::setup_l2`)

Erst **nachdem** `L2Setup::new` die komplette Klassifizierung oben berechnet hat, übersetzt
`setup_l2` das Ergebnis in tatsächliche RouterOS-Mutationen:

- `single_ports`: benennt nur den physischen Port um, legt keine Bridge an.
- `simple_bridges`: legt eine Bridge mit `vlan_filtering = false`, `ingress_filtering = false`,
  `protocol_mode = Rstp` an; jeder Mitgliedsport bekommt
  `frame_types = AdmitOnlyUntaggedAndPriorityTagged`.
- `tagged_bridges`: legt eine Bridge mit `vlan_filtering = true`, `protocol_mode = Mstp` an. Für
  jeden physischen Port wird aus der Menge seiner getaggten/ungetaggten VLAN-Zugehörigkeiten (über
  alle VLAN-Ebenen dieser Bridge hinweg gesammelt) die passende `frame_types`/`pvid`/
  `ingress_filtering`-Kombination abgeleitet:
  - nur ungetaggt auf einer VLAN → `AdmitOnlyUntaggedAndPriorityTagged`, `pvid` = diese VLAN.
  - ungetaggt auf einer VLAN **und** zusätzlich getaggt auf anderen → `AdmitAll`, `pvid` = die
    ungetaggte VLAN.
  - nur getaggt (keine ungetaggte VLAN) → `AdmitOnlyVlanTagged`, kein `pvid`.
  Zusätzlich wird pro VLAN-ID ein `/interface/bridge/vlan`-Eintrag erzeugt, der die Liste der
  getaggten bzw. ungetaggten Mitgliedsports dieser VLAN zusammenfasst. Hat die VLAN ein eigenes
  `/interface/vlan`, ist die **Bridge selbst** getaggtes Mitglied; mit VLAN-Filtering sieht das
  VLAN-Interface sonst keinen Verkehr (und z.B. ein DHCP-Server darauf keine Anfragen).
  Ein `L2Port::VxLan` wird dabei wie ein nur-getaggter Port behandelt: das VXLAN-Interface wird
  angelegt (inkl. VTEPs) und getaggtes Mitglied der VLAN. Ausnahme: VXLANs, die `setup_wlan_ap`
  schon an `bridge-caps` gehängt hat. Die übrigen virtuellen Varianten (`CapsMan`/`Cap`/`*Mgmt`/`Ip`)
  erzeugen keinen Bridge-Port.

## `logical_interfaces()`: die vereinheitlichte Sicht für alles Nachgelagerte

`L2Setup::logical_interfaces()` liefert für **alle drei** Domänen-Arten eine gemeinsame
`LogicalInterface`-Sicht (Interface-Name, IP-Adressen, `use_ospf`, `enable_dhcp_client`,
`enable_dhcp_server`). IP-/DHCP-/OSPF-Setup (siehe [ospf-generation.md](ospf-generation.md)) iteriert
ausschließlich über diese Sicht und muss daher selbst nie unterscheiden, ob eine logische
Schnittstelle ein Einzelport, eine flache Bridge oder eine VLAN-Ebene einer getaggten Bridge ist.
Für eine Bridge (flach oder getaggt) werden `use_ospf`/`enable_dhcp_client`/`enable_dhcp_server`
dabei als "mindestens einer der beteiligten physischen Root-Ports hat dieses Flag" berechnet (ein
NetBox-Flag pro Mitgliedsport, zusammengeführt auf Bridge-/VLAN-Ebene).
