# L2-Generierung (Bridges, VLANs, Ports)

Code: `backend/src/device/ros/l2/mod.rs` (`L2Setup::new`, `L2Setup::logical_interfaces`),
`backend/src/device/ros/mod.rs::setup_l2` (Realisierung als RouterOS-Config).

Das ist der komplexeste und zentralste Teil der Generierungs-Pipeline — fast alles andere
(IP-Adressen, DHCP, OSPF, VXLAN/WLAN-Anbindung) baut auf dem Ergebnis von `L2Setup::new` auf.

## Schritt 1: "Ebenen" (`planes`) aus der Topologie sammeln

Für jedes Interface eines Geräts wird bestimmt, zu welcher **Root** und welcher **VLAN** (oder
"kein VLAN") es gehört. Diese `(root, vlan)`-Paare heißen hier "Ebenen" (`planes:
BTreeMap<(Option<InterfaceId>, Option<VlanId>), BTreeSet<L2Port>>`). Die Root ist:

- die in NetBox eingetragene **Bridge** (`bridge()`), wenn das Interface eine nennt;
- sonst, wenn das Interface eine VLAN trägt (ungetaggt oder getaggt) und nicht selbst eine Bridge
  ist: **keine** Root (`None`), also die [Standard-Bridge](#standard-bridge) des Geräts;
- sonst das Interface **selbst** (ein einzelner Port ohne VLAN, ein Loopback, eine Bridge).

### Standard-Bridge

Ein Interface, das eine VLAN trägt, aber keine Bridge nennt, landet in der root-losen Gruppe. In
diese Gruppe sortiert der Code auch alle VXLAN-/CAPsMAN-/CAP-Anbindungen ohne physische Root ein.
Diese Gruppe wird zur **Standard-Bridge** des Geräts (`bridge`, auf einem Access Point
`bridge-caps`, siehe [interface-naming.md](interface-naming.md)). Dadurch muss man in NetBox nur
dann eine Bridge anlegen, wenn man sie wirklich braucht:

- für eine flache Port-Gruppe ohne VLAN (z.B. ein Switch-LAN ohne VLAN-Tagging),
- für ein bewusst getrenntes L2-Segment, oder
- für einen eigenen Bridge-Namen.

Für alles andere reicht es, die VLAN an Ports und VLAN-Interfaces einzutragen. Typischer Fall: die
LAN-Ports eines Access Points, die in einem per VXLAN transportierten Haushalts-VLAN hängen (z.B.
`ap-mimi-buero` `ether2`–`ether5`). Sie bilden zusammen mit dem VXLAN und dem CAP-Datapath ein L2.

Zwei Interfaces ohne Bridge mit derselben VLAN auf demselben Gerät sind damit dasselbe L2-Segment,
so wie NetBox eine VLAN ohnehin versteht.

Pro physischem Interface mit externem Port:
- Jede **getaggte VLAN** (`tagged_vlans()`) erzeugt einen `L2Port::TaggedEthernet`-Eintrag auf der
  Ebene `(root, Some(vlan))`.
- Eine **ungetaggte VLAN** (`untagged_vlan()`) erzeugt einen `L2Port::UntaggedEthernet`-Eintrag,
  ebenso auf `(root, Some(vlan))`.
- Hat das Interface **weder** getaggte noch ungetaggte VLAN, landet es als
  `L2Port::UntaggedEthernet` auf der "kein VLAN"-Ebene `(root, None)`.

Eigene IP-Adressen eines Interfaces (`interface.ips()`) werden als `L2Port::Ip { ip, owner }` auf **derselben
VLAN-Ebene** eingeordnet, zu der das Interface selbst über seine eigene `untagged_vlan()` gehört —
nicht pauschal auf die "kein VLAN"-Ebene. Das ist, was eine rein virtuelle "VLAN-Schnittstelle" in
NetBox (ein Interface ohne externen Port, nur mit `vlan` + `ips`) erlaubt, ihre L3-Adresse korrekt
auf dieser VLAN-Ebene zu tragen.

VXLAN- und WLAN-Anbindungen haben **kein** physisches Root-Device und werden daher auf root-losen
Ebenen `(None, Some(vlan))` einsortiert (`L2Port::VxLan`,
`L2Port::CapsMan`, `L2Port::Cap`, `L2Port::CapsManMgmt`, `L2Port::CapMgmt` — siehe
[vxlan-generation.md](vxlan-generation.md) und [wlan-generation.md](wlan-generation.md)).

## Schritt 2: Nach Root gruppieren, root-lose Ebenen zusammenführen

Alle Ebenen werden nach Root umgruppiert (`by_root: Root -> VLAN -> Ports`). Die root-lose Gruppe
(Standard-Bridge) wird anschließend **pro VLAN eingemischt**: Führt eine in NetBox modellierte
Bridge bereits dieselbe VLAN, kommen deren Einträge (VXLAN/CAPsMan/..., bridge-lose Ports und
VLAN-Interfaces) dorthin. Sonst bleiben sie in der Standard-Bridge. Die "kein VLAN"-Ebene der
root-losen Gruppe wird nie eingemischt, weil sie keine VLAN-Identität hat, die zu einer anderen
Bridge passen könnte.

## Schritt 3: Konsistenz-Checks

Erst **nach** dem Zusammenführen wird geprüft (vorher würde z.B. ein AP-Port in einer modellierten
Bridge plus der CAP-Eintrag derselben VLAN fälschlich als Konflikt gelten):

- **Mehrdeutige VLAN-IDs**: Alle Ebenen werden nach ihrer **echten numerischen** NetBox-VLAN-ID
  gruppiert. Taucht dieselbe Nummer in mehr als einer Ebene auf (dieselbe VLAN auf zwei modellierten
  Bridges, oder zwei verschiedene NetBox-VLANs mit derselben Nummer auf einem Gerät), bricht die
  Generierung mit `SetupError::AmbiguousVlanIds` ab, statt stillschweigend eine Bedeutung zu
  verlieren.
- **Port in mehreren Bridges**: Weil pro VLAN zusammengeführt wird, könnte ein bridge-loser
  Trunk-Port mit einer VLAN in einer modellierten Bridge und einer anderen in der Standard-Bridge
  landen. Ein Port kann aber nur Mitglied einer Bridge sein, daher `SetupError::PortInMultipleBridges`.
  Dasselbe gilt für das VXLAN einer VLAN-Gruppe, das jedem getunnelten VLAN beitritt.
- **L2-Domäne**: Schon in Schritt 1 prüft `check_l2_domain` jedes VLAN über alle Geräte, auf die es
  getunnelt wird: Kabel parallel zum Tunnel, mehrere IP-Netze, mehrere DHCP-Server (siehe
  [vxlan-generation.md](vxlan-generation.md#was-abgelehnt-wird)).

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
(`next_synthetic_vlan_id`, kollisionsfrei gegenüber allen in Schritt 3 gesammelten echten IDs).

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
  angelegt (inkl. VTEPs) und getaggtes Mitglied der VLAN. Die übrigen virtuellen Varianten
  (`CapsMan`/`Cap`/`*Mgmt`/`Ip`) erzeugen keinen Bridge-Port.
  Pro VLAN gibt es genau einen `/interface/bridge/vlan`-Eintrag; RouterOS lehnt überlappende
  Einträge derselben VLAN-ID (z.B. ein `1-4094`-Bereich neben einem Eintrag für VLAN 4) mit
  `vlan already added` ab (geprüft gegen CHR 7.24.5).

## `logical_interfaces()`: die vereinheitlichte Sicht für alles Nachgelagerte

`L2Setup::logical_interfaces()` liefert für **alle drei** Domänen-Arten eine gemeinsame
`LogicalInterface`-Sicht (Interface-Name, IP-Adressen, `use_ospf`, `enable_dhcp_client`,
`enable_dhcp_server`). IP-/DHCP-/OSPF-Setup (siehe [ospf-generation.md](ospf-generation.md)) iteriert
ausschließlich über diese Sicht und muss daher selbst nie unterscheiden, ob eine logische
Schnittstelle ein Einzelport, eine flache Bridge oder eine VLAN-Ebene einer getaggten Bridge ist.
Für eine Bridge (flach oder getaggt) werden `use_ospf`/`enable_dhcp_client`/`enable_dhcp_server`
dabei als "mindestens eine beteiligte Root hat dieses Flag" berechnet. Die Tags eines
Bridge-Mitglieds selbst zählen also nicht, nur die der Bridge. Bei einer VLAN-Ebene einer getaggten
Bridge zählen zusätzlich die Tags des Interfaces, dem eine IP-Adresse auf dieser VLAN gehört
(`L2Port::Ip`'s `owner`, typischerweise das VLAN-Interface). Auf der Standard-Bridge gibt es kein
NetBox-Interface für die Bridge, dort ist das VLAN-Interface der einzige Ort für diese Tags. Bei
bridge-losen Ports der Standard-Bridge ist die Root der Port selbst, dessen Tags zählen deshalb mit.
