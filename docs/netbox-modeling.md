# Geräte in NetBox modellieren

Diese Anleitung beschreibt, welche NetBox-Features der Provisioner auswertet und wie Router und
Switches gepflegt werden müssen, damit am Ende eine korrekte RouterOS-Konfiguration herauskommt.
Was der Provisioner aus den Daten macht, steht in den anderen Dateien dieses Ordners (siehe
[overview.md](overview.md)).

Grundlage ist ausschließlich das, was `backend/src/netbox/fetch_topology.graphql` abfragt und
`backend/src/topology/fetch.rs` auswertet. Alles, was dort nicht vorkommt, ignoriert der
Provisioner, auch wenn es in NetBox gepflegt ist (siehe [Was nicht ausgewertet wird](#was-nicht-ausgewertet-wird)).

## Einmalige Einrichtung in NetBox

Diese Objekte müssen in NetBox existieren, bevor irgendein Gerät provisioniert werden kann.

### Tags

Die Slugs müssen exakt so heißen:

| Tag-Slug      | Wirkung, wenn am Interface gesetzt                            |
|---------------|---------------------------------------------------------------|
| `ospf`        | OSPF (v2 und v3) auf diesem Interface aktivieren              |
| `dhcp`        | DHCP-**Server** auf diesem Interface (nur IPv4)               |
| `dhcp-client` | DHCP-**Client** auf diesem Interface, falls es keine IP hat   |
|               | (mit `ospf` ohne Default-Route: die kommt dann über OSPF)     |
| `nat`         | Uplink, dessen Netz nicht zurückroutet: ausgehend maskieren   |

`nat` erzeugt `srcnat action=masquerade out-interface=<Uplink>`. Beantwortet der Router selbst
DNS (kein Config Context `dns_servers`, siehe unten), sperrt es zudem DNS (TCP/UDP 53) vom Uplink
her im `input`.

### Config Context `dns_servers`

```json
{"dns_servers": ["172.31.88.2", "2a02:16a:fe00:1a0::10:5", "172.16.1.4", "fd7e:907d:34ab::4"]}
```

Die DNS-Server der Client-Netze, der erste wird zuerst gefragt (z.B. Blocky mit Adfilter, dann
ein Fallback). Zuweisen wie jeder Config Context, z.B. pro Tenant oder Site. Der Provisioner
verteilt die IPv4-Server per DHCP und die IPv6-Server per Router Advertisement (RDNSS), letzteres
nur auf Geräten mit Client-Netz. Jedes Gerät fragt die Server auch selbst
(`/ip dns servers`), beantwortet aber keine DNS-Anfragen (`allow-remote-requests=no`).

### Custom Fields

| Objekt-Typ          | Name                   | Typ                          | Zweck                                         |
|---------------------|------------------------|------------------------------|-----------------------------------------------|
| Tenant              | `mikrotik_credentials` | Text                         | Name eines Zugangsdaten-Eintrags im Provisioner |
| Device              | `wlan_group`           | Objekt → Wireless LAN Group  | Gerät ist Access Point dieser WLAN-Gruppe     |
| Wireless LAN Group  | `controller`           | Objekt → Device              | CAPsMAN-Controller der Gruppe                 |
| Wireless LAN Group  | `wlan_mgmt`            | Objekt → VLAN                | Management-VLAN der Gruppe                    |
| VLAN Group          | `vxlan_vni`            | Integer                      | VLANs der Gruppe über VXLAN verbinden         |
| Wireless Link       | `rf_country`           | Text                         | Land beider Enden einer Funkbrücke übersteuern |
| Wireless Link       | `rf_frequency_mode`    | Selection                    | Frequency Mode beider Enden übersteuern       |

Die Namen müssen exakt stimmen. Anlegen, Werte und Wirkung im Einzelnen:
[custom-fields.md](custom-fields.md).

### Weitere Stammdaten

- **Platform** mit Name `routeros`: markiert ein Gerät als RouterOS-Gerät. Die Geräteliste im
  Frontend filtert darauf (`has_routeros`).
- **IP-Range-Rolle** mit Slug `dhcp`: markiert eine IP-Range als DHCP-Pool.
- **VLAN-Gruppe(n)**: jedes VLAN muss in einer VLAN-Gruppe liegen (siehe [VLANs](#vlans)).
- **Regionen**: Die oberste Region einer Site ist das Land, mit dem ISO-3166-Code als Slug (z.B.
  Region „Schweiz“, Slug `ch`). Darunter dürfen beliebige weitere Regionen hängen; ausgewertet
  werden bis zu sechs Ebenen. Gebraucht wird das Land für die Funkregeln einer
  [Funkbrücke](#funkbrücke).

## Zugangsdaten

Der Provisioner speichert keine Passwörter in NetBox. Stattdessen:

1. In der Provisioner-Konfiguration (`config.yaml`, Abschnitt `settings`) einen benannten Eintrag
   anlegen:
   ```yaml
   settings:
     mikrotik_credentials:
       standort-berg:
         user: provisioner     # optional, Standard: admin
         password: "..."
   ```
2. Am **Tenant** das Custom Field `mikrotik_credentials` auf diesen Namen setzen
   (`standort-berg`).

Der Tenant wird in dieser Reihenfolge gesucht: Tenant des Geräts, dann Tenant der Location, dann
Tenant der Site. Der erste Tenant, der das Feld gesetzt hat, gewinnt. Findet sich keiner, ist das
Gerät nicht erreichbar. Im Frontend kann man dann noch Ad-hoc-Zugangsdaten eingeben.

## Gerät (Device)

| NetBox-Feld                  | Verwendung                                                              |
|------------------------------|-------------------------------------------------------------------------|
| Name                         | `/system identity`; außerdem Teil generierter Bridge-Namen              |
| Platform                     | `routeros` = RouterOS-Gerät                                             |
| Tenant / Location / Site     | Zugangsdaten (siehe oben)                                               |
| Primary IPv6 / Primary IPv4  | **Management-Adresse**: der Provisioner verbindet sich zur primären IPv6, falls gesetzt, sonst zur primären IPv4 |
| Primary IPv4                 | **OSPF-Router-ID** und **VXLAN-VTEP-Adresse**. Ohne primäre IPv4 wird **kein OSPF** generiert |
| Custom Field `wlan_group`    | nur für Access Points, siehe [WLAN](#wlan)                              |

Das **Board-Modell** kommt nicht aus NetBox (der Device Type wird nicht gelesen), sondern wird
live vom Gerät gelesen (`/system/routerboard`). Unterstützt sind nur die Modelle in
`backend/src/device/ros/hw_facts.rs`: `RB962UiGS-5HacT2HnT`, `RB750Gr3`, `CRS326-24G-2S+`,
`CRS318-16P-2S+`, `C52iG-5HaxD2HaxD`, `cAPGi-5HaxD2HaxD`, `CCR1009-7G-1C-1S+`,
`CRS354-48G-4S+2Q+`, `CRS109-8G-1S-2HnD`. Für jedes andere Modell bricht die Generierung mit
`NoPortsFound` ab.

**Empfehlung:** Die primäre IPv4 auf das Loopback-Interface legen (siehe unten). Dann ist die
Router-ID stabil, und die VTEP-Adresse hängt an keinem physischen Link.

## Interfaces

### Namen müssen den RouterOS-Standardnamen entsprechen

Der Provisioner erkennt ein Interface nur am **Namen** als physischen Port (`PhysicalPortId::from_str`):

| NetBox-Interface-Name | wird zu                                           |
|-----------------------|---------------------------------------------------|
| `ether<N>`            | Ethernet-Port N (`/interface ethernet`, `ether<N>`) |
| `sfp-sfpplus<N>`      | SFP+-Port N                                       |
| `sfp<N>`              | SFP-Port N (z.B. `RB962…`, `CRS109…`)             |
| `combo<N>`            | Combo-Port N (z.B. `CCR1009…`)                    |
| `qsfpplus<N>-<L>`     | Lane L des QSFP+-Ports N (z.B. `CRS354…`)         |
| `wlan<N>`             | Radio des `wireless`-Pakets (`/interface/wireless`): Funkbrücke (siehe [Funkbrücke](#funkbrücke)) oder, auf einem AP, Radio des klassischen CAPsMAN |
| `wlan60-<N>`          | 60-GHz-Funk-Port (`/interface/w60g`, siehe [Funkbrücke](#funkbrücke)) |
| `wifi<N>`             | WLAN-Interface (wird derzeit nicht konfiguriert)  |
| `lo`                  | Loopback                                          |
| alles andere          | **virtuelles** Interface (Bridge, VLAN-Interface …) |

Die NetBox-Namen bleiben also immer `ether1`, `ether2` usw. Den sprechenden Namen auf dem Gerät
(z.B. `e03-ap-keller`) erzeugt der Provisioner selbst aus Port und **Label** (siehe
[interface-naming.md](interface-naming.md)). Der menschenlesbare Teil gehört deshalb ins Feld
**Label**, nicht in den Namen.

Der Name muss **exakt** passen: `uplink-ether3` oder `ether3-alt` sind virtuelle Interfaces, kein
Port.

QSFP+-Ports heißen in RouterOS immer pro Lane, auch ohne Breakout-Kabel: `qsfpplus1-1` ist dann
der ganze 40G-Port. Bei einem Breakout-Kabel (ein QSFP+-Port auf vier SFP+-Gegenstellen) pro
genutzter Lane ein Interface `qsfpplus1-1`, `qsfpplus1-2`… anlegen, mit eigenem Kabel und eigener
IP. Ein NetBox-Interface `qsfpplus1` ohne Lane ist kein Port und wird ignoriert; es kann entfernt
werden.

### Felder pro Interface

| NetBox-Feld          | Verwendung                                                                 |
|----------------------|----------------------------------------------------------------------------|
| Name                 | Port-Erkennung (siehe oben)                                                |
| Label                | sprechender Name auf dem Gerät, z.B. Label `AP Keller` an `ether3` → `e03-ap-keller` |
| Type                 | nur relevant für `lo`: das Loopback muss Typ **Virtual** haben             |
| Bridge               | Mitglied welcher Bridge (siehe [Bridges](#bridges-und-vlans))              |
| 802.1Q: Untagged VLAN | Access-VLAN des Ports bzw. VLAN eines VLAN-Interfaces                     |
| 802.1Q: Tagged VLANs | getaggte VLANs (Trunk)                                                     |
| IP Addresses         | IP-Adressen auf dem Interface                                              |
| Tags                 | `ospf`, `dhcp`, `dhcp-client`, `nat` (siehe oben)                          |
| PoE Mode             | `PSE` → PoE-Ausgang `auto-on`, sonst `off` (nur auf PoE-fähigen Boards); das Gerät am anderen Kabelende gilt als von diesem Port gespeist. Ein Update startet das speisende Gerät erst neu, wenn kein gespeistes (auch weiter unten in der Kette) mehr ein Update macht. Würde Anwenden an einem Port mit eingeschaltetem PoE-Ausgang diesen ausschalten, warnt die Geräteseite davor |
| MTU                  | L3-MTU eines Ethernet-Ports oder `wlan`-Radios, siehe [MTU](#mtu) |
| RF Role, RF Channel Frequency/Width, TX Power | Funk-Port einer [Funkbrücke](#funkbrücke) |
| Wireless Link        | SSID, PSK und Funkregeln einer [Funkbrücke](#funkbrücke)                   |
| Cable                | Namensgenerierung, wenn kein Label gesetzt ist; Weg für die Ersteinrichtung |

Der 802.1Q-Mode selbst (Access/Tagged) wird nicht ausgewertet. NetBox verlangt ihn aber, damit
man VLANs zuweisen kann.

### MTU

Ohne MTU in NetBox behält ein Port die 1500 von RouterOS. Mit MTU bekommt ein Ethernet-Port oder
ein `wlan`-Radio diese L3-MTU (`mtu`) und eine L2-MTU (`l2mtu`), die sie samt VLAN-Tag trägt: die
des Boards, mindestens MTU + 4. Auf anderen Interfaces (Bridge, VLAN, `wifi`, `wlan60`) lehnt die
Generierung eine MTU ab; eine Bridge nimmt die kleinste MTU ihrer Ports.

- **Beide Enden eines Kabels brauchen dieselbe MTU** (ohne Angabe 1500), sonst schlägt die
  Generierung fehl: OSPF bildet nur zwischen gleichen MTUs eine Nachbarschaft, und RouterOS 7 hat
  kein `mtu-ignore`. Das gilt für die ganze Strecke: über eine Funkbrücke tragen auch die Ports
  und Radios der Brückengeräte die MTU.
- **Wofür:** VXLAN braucht für einen vollen, getaggten Client-Frame (1518 Byte) außen 1554 Byte
  (über IPv4) bzw. 1574 (über IPv6). Mit MTU 1500 auf den Transportstrecken wird er fragmentiert.
  Im Lab haben alle Strecken zwischen den Geräten **2000**: Das liegt unter dem kleinsten Maximum
  der Boards dort (2290 der `wireless`-Radios; ein hEX liegt vermutlich bei etwa 2026), Jumbo
  Frames gehen deshalb nicht. Clients bleiben bei 1500.
- **Umstellen:** Bis beide Enden einer Strecke umgestellt sind, ist sie für OSPF unterbrochen.
  Deshalb vom Einstieg des Provisioners nach außen anwenden: Jedes weitere Gerät erreicht der
  Provisioner dann über den Nachbarn am Kabel ([Ersteinrichtung](#ersteinrichtung-über-mac-telnet),
  den Weg über das Gerät des Management-Proxys zuerst), und sein Apply stellt die Strecke zum
  schon umgestellten Nachbarn wieder her.
- **Funk:** Eine neue MTU startet ein `wlan`-Radio neu. Auf einem DFS-Kanal hört es danach erst
  auf Radar, bevor es sendet; im Wetterradar-Bereich (5600–5650 MHz) dauert das nach ETSI
  10 Minuten (im Lab gemessen). So lange ist die Funkbrücke weg.

### Ersteinrichtung über MAC-Telnet

Ein neues Gerät (Werkseinstellung oder ohne Adresse) richtet der Provisioner per Knopf auf der
Geräteseite über einen erreichbaren Nachbarn ein: Der Nachbar öffnet an seinem Port MAC-Telnet
zum neuen Gerät und setzt dort Passwort, Adresse und API. Dafür braucht das neue Gerät in NetBox
einen Port mit:

- einer **IPv4-Adresse** (auf dem Port selbst oder auf der Bridge, zu der er gehört), und
- einem **Kabel** zu einem Port des Nachbarn, der eine IPv4-Adresse im selben Netz hat.

MAC-Adressen werden in NetBox nicht gepflegt. Welches physische Gerät die Konfiguration bekommt,
wählt man auf der Geräteseite aus: Dort steht, was der Nachbar an seinem Port per Neighbor
Discovery sieht (Identity, Board, Version, MAC), oder man gibt die MAC von Hand ein. Hinter einer
Funkbrücke sieht der Nachbar auch Geräte, die weiter weg sind.

Bis zum ersten Anwenden (das OSPF bringt) erreicht der Provisioner das neue Gerät über einen
SSH-Tunnel zum Nachbarn: Die Verbindungen kommen von der Adresse des Nachbarn auf dem Kabel, das
neue Gerät braucht also keine Route zurück. Dafür setzt die generierte Konfiguration auf jedem
Gerät `/ip ssh forwarding-enabled=local`; ein Nachbar muss also selbst schon einmal angewendet
worden sein.

### Loopback

Ein Interface mit Name `lo` und Typ **Virtual** anlegen und ihm eine `/32`- (bzw. `/128`-)Adresse
geben. Die erste IP darauf wird als Loopback-Adresse gesetzt. Diese Adresse in der Regel auch als
primäre IPv4 des Geräts markieren.

## Bridges und VLANs

### Bridges

Eine Bridge muss man nur anlegen, wenn man sie wirklich braucht. Jedes Interface, das eine VLAN
trägt und keine Bridge nennt, landet automatisch auf der **Standard-Bridge** des Geräts (`bridge`,
auf einem Access Point `bridge-caps`). Eine eigene Bridge braucht man nur für:

- eine **flache Port-Gruppe ohne VLAN** (z.B. ein Switch-LAN, dessen Ports keine VLAN tragen),
- ein bewusst **getrenntes L2-Segment** mit eigener Bridge,
- einen eigenen Bridge-Namen.

Eine Bridge wird in NetBox als **virtuelles Interface** am Gerät angelegt (Typ Bridge). Alle
Ports, VLAN-Interfaces und IPs, die dazugehören sollen, bekommen im Feld **Bridge** diese Bridge
eingetragen.

- Der Name der Bridge in NetBox wird **nicht** übernommen. Auf dem Gerät heißt sie
  `bridge-<name-des-bridge-interfaces>` (z.B. NetBox-Name `sw` → `bridge-sw`).
- Ob daraus eine einfache Bridge, eine VLAN-Filtering-Bridge oder gar keine Bridge wird, entscheidet
  der Provisioner selbst (siehe [l2-generation.md](l2-generation.md)). Ein Port ohne Bridge und
  ohne VLAN bleibt ein eigenständiger Port.
- Führt eine modellierte Bridge eine VLAN, kommen bridge-lose Ports und VLAN-Interfaces mit
  derselben VLAN in diese Bridge statt in die Standard-Bridge. Ein bridge-loser Trunk-Port darf
  dadurch nicht auf zwei Bridges verteilt werden (`PortInMultipleBridges`).
- Hat die Bridge ein **Untagged VLAN**, sind alle ihre Ports ohne eigenes VLAN in diesem VLAN. So
  lässt sich ein ganzes Switch-LAN in ein VLAN legen, ohne jeden Port einzeln zu setzen.
- **Tags an der Bridge gelten für alle VLANs dieser Bridge.** Die Tags `ospf`/`dhcp`/`dhcp-client`
  werden bei Bridges vom Bridge-Interface gelesen, nicht von den Bridge-Ports. `dhcp` an der Bridge
  aktiviert also einen DHCP-Server auf **jeder** VLAN der Bridge, die eine IP hat. Zusätzlich
  zählen die Tags des VLAN-Interfaces, das die IP einer VLAN trägt (siehe unten).

### VLANs

- Jedes VLAN muss in einer **VLAN-Gruppe** liegen. Der Provisioner liest VLANs nur über die
  VLAN-Gruppen. Ein VLAN ohne Gruppe ist für ihn unsichtbar: ein Port, der es referenziert, bekommt
  dann eine synthetische VLAN-ID ab 60000 statt der echten.
- Dieselbe VLAN-ID darf auf einem Gerät nicht in zwei modellierten Bridges vorkommen, und zwei
  verschiedene VLANs mit derselben VID dürfen nicht auf demselben Gerät landen. Sonst bricht die
  Generierung mit `AmbiguousVlanIds` ab.
- Der VLAN-Name fließt in den Interface-Namen ein (`vlan-<name>-<vid>`).

### L3 auf einem VLAN (Gateway-Adresse)

Für eine IP-Adresse auf einem VLAN ein **virtuelles Interface** anlegen (Typ Virtual) mit:
- **Untagged VLAN** = das VLAN,
- **IP Address** = die Gateway-Adresse,
- **Bridge** = die Bridge, falls das VLAN auf einer modellierten Bridge liegt. Ohne Bridge kommt
  das VLAN-Interface auf die Bridge, die dieses VLAN führt, sonst auf die Standard-Bridge.
- **Tags** `dhcp`/`dhcp-client`/`ospf` für dieses VLAN.

Der Provisioner legt dafür ein `/interface vlan` auf der Bridge an und setzt die IP dort. Die IP
nie direkt an einen Bridge-Port hängen; ein Bridge-Mitglied kann in RouterOS keine eigene IP
tragen.

## IP-Adressen, Prefixe und DHCP

- **Reines L2-Gerät:** Hat ein Gerät in NetBox keine einzige IP-Adresse und nirgends das Tag
  `dhcp-client` (z.B. eine Funkbrücke), bekommt jede seiner flachen Bridges einen DHCP-Client.
  Sonst wäre es nach dem Provisionieren nicht mehr erreichbar. Der Client hängt am generierten
  Bridge-Namen und zieht bei einer Umbenennung mit.
- Zu jeder IP-Adresse muss ein **Prefix** mit genau derselben Länge existieren (Adresse
  `192.168.50.1/24` braucht Prefix `192.168.50.0/24`). Für DHCP-Server ist das zwingend,
  sonst `MissingPrefixOnIpAddress`.
- **DHCP-Pool explizit:** im Prefix eine **IP-Range** mit Rolle `dhcp` anlegen. Start und Ende
  müssen im selben Prefix liegen.
- **DHCP-Pool automatisch:** ohne `dhcp`-Range nimmt der Provisioner den gesamten freien Rest des
  Prefixes, abzüglich aller dort vergebenen IP-Adressen, Kind-Prefixe und anderen Ranges.
- Ein IPv6-/64 an einem Interface mit `dhcp` wird per Router Advertisement angekündigt
  (`advertise=yes`, SLAAC). Andere IPv6-Adressen nicht.
- Gateway des DHCP-Netzes ist die eigene Adresse des Routers auf diesem Interface. DNS-Server
  sind die aus dem Config Context `dns_servers`. Ohne diesen ist es ebenfalls die eigene Adresse,
  und der Router beantwortet DNS-Anfragen (`allow-remote-requests=yes`). Hat er selbst keinen
  `dhcp-client`, lernt er keinen DNS-Server und fragt die Loopbacks der Router mit `nat`-Uplink.

## Rezept: Router

Beispiel: Core-Router mit Uplink, OSPF zu zwei Nachbarn und einem Kunden-Port.

1. **Gerät** anlegen: Name, Platform `routeros`, Tenant mit `mikrotik_credentials`.
2. **Loopback**: Interface `lo`, Typ Virtual, IP `172.16.1.1/32`. Diese IP als **Primary IPv4**
   markieren. Optional eine IPv6 als **Primary IPv6** (wird dann Management-Adresse).
3. **Transit-Links zu OSPF-Nachbarn**: auf `ether1`, `ether2` je eine Transfer-IP (z.B.
   `10.172.12.65/30`), Tag `ospf`, Label mit dem Ziel (z.B. `Kirche`). Ohne Bridge, ohne VLAN →
   eigenständige Ports.
4. **Kabel** zu den Nachbargeräten in NetBox eintragen. Das ist nicht nötig für die Config, hilft
   aber bei der Namensgebung, falls kein Label gesetzt ist.
5. **Kunden-/Haushalts-Port** mit DHCP: `ether5` mit Label `Bachmann Dachboden`, IP
   `192.168.30.1/24`, Tags `dhcp`. Prefix `192.168.30.0/24` muss existieren.
6. **Uplink ohne feste IP**: `ether8` mit Tag `dhcp-client` und ohne IP, dazu `nat`, wenn das Netz
   dahinter die eigenen Netze nicht kennt (keine Route zurück).

## Rezept: Switch mit Haushalts-VLANs

Beispiel: Switch mit getaggtem Uplink, zwei Haushalts-VLANs als Access-Ports, Management per OSPF.

1. **Gerät** wie oben (Platform, Tenant, Loopback, Primary IPv4).
2. **VLAN-Gruppe** mit den Haushalts-VLANs anlegen, z.B. `haushalt-10` (VID 10), `haushalt-20`
   (VID 20).
3. **Bridge**: virtuelles Interface `sw`, Typ Bridge. Tag `ospf` hier setzen, falls OSPF über
   die VLANs laufen soll (gilt dann für alle VLANs der Bridge).
4. **Uplink** `ether1`: Bridge = `sw`, 802.1Q Mode Tagged, Tagged VLANs = `haushalt-10`,
   `haushalt-20`, Label `Uplink`.
5. **Access-Ports** `ether2`, `ether3`: Bridge = `sw`, 802.1Q Mode Access, Untagged VLAN =
   `haushalt-10` bzw. `haushalt-20`, Label = Wohnung/Dose.
6. **Gateway auf einem VLAN** (falls der Switch dort L3 machen soll): virtuelles Interface
   `haushalt-10`, Bridge = `sw`, Untagged VLAN = `haushalt-10`, IP `192.168.10.1/24`.

Daraus entsteht eine VLAN-Filtering-Bridge `bridge-sw`. `ether1` wird als nur-getaggt konfiguriert,
`ether2`/`ether3` als ungetaggt mit passender PVID, und es gibt ein `/interface vlan` mit der
Gateway-IP.

Die Bridge `sw` (Schritt 3) ist hier optional. Ohne sie und ohne die Bridge-Felder entsteht dieselbe
Struktur auf der Standard-Bridge `bridge`; das Tag `ospf` kommt dann an das VLAN-Interface.

## VXLAN

Ein VXLAN gehört zu einer **VLAN-Gruppe**:

1. Custom Field **`vxlan_vni`** (Integer) auf VLAN-Gruppen anlegen und bei der Gruppe die VNI
   eintragen, z.B. `Berg-WLAN` = `1`.
2. Den Ports, Bridges und VLAN-Interfaces, die im Netz sein sollen, das VLAN zuweisen.

Kommt ein VLAN einer solchen Gruppe auf mehr als einem Gerät vor, wird es zwischen diesen Geräten
getunnelt. Ein WLAN-VLAN kommt dabei auf jedem AP und dem Controller der WLAN-Gruppe vor. Jedes
beteiligte Gerät braucht eine **Primary IPv4**, sie ist seine VTEP-Adresse.

- Ein VXLAN pro Gruppe, mit allen getunnelten VLANs der Gruppe getaggt darin. Name auf dem Gerät:
  `vxlan-{gruppenname}`.
- VLANs aus Gruppen ohne `vxlan_vni` bleiben lokal, auch auf mehreren Geräten.
- Ein **L2VPN** wird nicht mehr gelesen und kann gelöscht werden; ebenso VXLAN-Interfaces und
  `bridge-caps`, die als Interface modelliert sind.

Statt zu generieren meldet der Provisioner einen Fehler, wenn in einem VLAN über alle beteiligten
Geräte

- Adressen aus **mehr als einem Netz** liegen (z.B. `10.192.2.1/24` am Controller und
  `10.192.5.1/24` an einem Switch),
- **mehr als ein** Interface das Tag `dhcp` hat,
- ein getunneltes VLAN **zusätzlich über ein Kabel** zu einem anderen Gerät läuft, das es ebenfalls
  trägt (Schleife), oder
- zwei getunnelte VLANs derselben Gruppe auf einem Gerät in **verschiedenen Bridges** liegen.

Details: [vxlan-generation.md](vxlan-generation.md).

### Rezept: Netze am Controller

Alle WLANs einer WLAN-Gruppe mit VLAN müssen **dieselbe VLAN** haben: Der AP setzt sie pro Radio,
und auf einem wAP ac (`wifi-qcom-ac`) geht das nur als Access-Port der Bridge.

Jedes Netz, das der Controller für die WLANs bereitstellt, ist ein **virtuelles Interface** mit
Untagged VLAN = WLAN-VLAN, IP-Adresse und gegebenenfalls Tag `dhcp`. Keine Bridge pro Netz: das
VXLAN kann nur in einer Bridge sein, und alle Netze liegen auf der Standard-Bridge.

### Rezept: LAN-Ports eines Access Points im Haushalts-WLAN

Ziel: An einem AP (z.B. `ap-mimi-buero`) sollen `ether2`–`ether5` im selben Netz wie das WLAN
`wlan-mimi` sein.

1. Der AP ist Mitglied der WLAN-Gruppe (Custom Field `wlan_group`).
2. Die Ports bekommen **Untagged VLAN** = `wlan-mimi`. **Keine** Bridge.

Die Ports landen auf `bridge-caps`, zusammen mit dem VXLAN und dem CAP-Datapath.

### Rezept: Wohnungs-Ports eines Switches im Netz eines WLANs

Ziel: Ein Drucker an der LAN-Dose einer Wohnung soll im selben Netz wie das WLAN der Wohnung sein.

1. Die Ports der Wohnung auf dem Switch bekommen **Untagged VLAN** = das VLAN des WLANs (wie jeder
   Access-Port). Hängen sie an einer Bridge, reicht das VLAN an der Bridge.
2. Der Switch braucht eine **Primary IPv4**.

Alle anderen Teilnehmer tragen den Switch dann automatisch als Remote-VTEP ein. Der Switch darf
keine eigene IP oder keinen DHCP-Server in einem anderen Netz desselben VLANs haben.

## Funkbrücke

Eine Punkt-zu-Punkt-Funkstrecke (z.B. `hegglin-schopf` ↔ `schopf-hegglin`), die das L2 transparent
weiterreicht. Pro Seite:

1. Ein Gerät mit Platform `routeros`. Unterstützt sind die Boards `RBDynaDishG-5HacDr3` und
   `SXT G-5HPacD` (5 GHz) sowie `RBwAPG-60ad`, `RBwAPG-60ad-A` und `RBLHGG-60ad` (60 GHz); weitere
   müssen erst im Provisioner eingetragen werden.
2. Interfaces mit den RouterOS-Namen: `ether1` und der Funk-Port, **`wlan1`** bei 5 GHz,
   **`wlan60-1`** bei 60 GHz.
3. Eine Bridge (virtuelles Interface), bei `ether1` und dem Funk-Port als **Bridge** eingetragen.
4. Management: nichts zu tun. Ein Gerät ohne jede IP-Adresse und ohne Tag `dhcp-client` bekommt
   auf seiner Bridge automatisch einen DHCP-Client (siehe
   [IP-Adressen, Prefixe und DHCP](#ip-adressen-prefixe-und-dhcp)). Wer eine feste Adresse will,
   trägt sie an der Bridge ein.
5. Am Funk-Port die **RF Role**: `AP` auf der einen, `Station` auf der anderen Seite.
   **RF Channel Frequency** (MHz) und **RF Channel Width** sind optional; die Frequenz gilt nur
   am AP, ohne sie wählt er selbst (`auto`). **TX Power** (dBm, nur 5 GHz) ist ebenfalls
   optional: gesetzt sendet die Seite fest mit dieser Leistung, sonst mit der Standardleistung.
6. Die Site liegt in einer Region mit dem Land (siehe [Weitere Stammdaten](#weitere-stammdaten)).

Dazu einmal pro Strecke ein **Wireless Link** zwischen den beiden Funk-Ports: **SSID**,
Authentication **WPA Personal** und der **PSK**. Er ist der gemeinsame Schlüssel beider Seiten
(Nv2-Schlüssel bei 5 GHz, Passwort bei 60 GHz). Mit Authentication **Open** läuft die Strecke
unverschlüsselt. Bei WPA Personal ohne PSK bricht die Generierung ab.

Ein Funk-Port ohne RF Role wird nicht als Funk verwaltet: seine Konfiguration bleibt, wie sie
auf dem Gerät ist. Mit RF Role legt der Provisioner dagegen **alle** Einstellungen des Funk-Ports
fest; was von Hand gesetzt war, geht verloren.

**Land und Funkregeln.** Normalerweise gelten die Regeln des Landes der Site
(`frequency-mode=regulatory-domain`, `country` aus der Region). Beide Seiten müssen in derselben
Regelung laufen; jede Seite nimmt aber das Land ihrer eigenen Site. Liegt eine Strecke über eine
Landesgrenze, muss `rf_country` gesetzt sein. Das wird nicht geprüft. Abweichend davon lässt sich
am Wireless Link übersteuern:

- `rf_country`: der RouterOS-Wert direkt, bei 5 GHz für `country` (z.B. `etsi 5.5-5.7 outdoor`),
  bei 60 GHz für `region` (z.B. `eu`, `no-region-set`). Er wird nicht geprüft; ein Tippfehler
  fällt erst beim Anwenden auf.
- `rf_frequency_mode`: `regulatory-domain`, `manual-txpower` oder `superchannel` (nur 5 GHz).

Ein Übersteuern verlässt in der Regel die zulässigen Grenzen. Es ist für Ausnahmen gedacht, nicht
für den Normalfall.

Was daraus generiert wird und worauf beim Ausrollen zu achten ist:
[radio-bridge-generation.md](radio-bridge-generation.md).

## WLAN

Generiert wird der WiFi-Stack (`/interface/wifi`, RouterOS 7) mit CAPsMAN auf dem Controller und
lokalem Forwarding auf den APs (siehe [wlan-generation.md](wlan-generation.md)). Der AP ist wie
jedes Gerät geroutet angebunden (Loopback, eigenes Transfernetz mit `ospf`). Für das Netz des WLANs
gibt es zwei Varianten:

- **Lokal auf dem AP**: eine Bridge mit IP und `dhcp`-Tag auf dem AP, in der die WiFi-Interfaces
  (`wifi1`, `wifi2`) und allenfalls Kundenports Mitglied sind. Das WLAN hat keine VLAN.
- **Auf dem Controller** (oder einem anderen Gerät): Das WLAN hat eine VLAN aus einer VLAN-Gruppe
  mit `vxlan_vni`, das Netz ist dort ein virtuelles Interface mit dieser VLAN, IP und `dhcp` (siehe
  [Rezept: Netze am Controller](#rezept-netze-am-controller)). Die WiFi-Interfaces des AP bekommen
  keine Bridge; Kundenports des AP bekommen die VLAN als Untagged VLAN (siehe
  [Rezept](#rezept-lan-ports-eines-access-points-im-haushalts-wlan)).

1. **Wireless LAN Group** anlegen, Custom Field `controller` = Controller-Gerät (Pflicht; eine
   Gruppe ohne Controller wird komplett ignoriert), `wlan_mgmt` = Management-VLAN. Pro Controller
   nur **eine** Gruppe; ist ein Gerät Controller mehrerer Gruppen, gewinnt eine davon.
2. **Wireless LANs** in der Gruppe anlegen: SSID, VLAN und Authentifizierung. Ausgewertet werden
   nur **WPA Personal** (mit PSK) und **Open** (wird als OWE umgesetzt). WLANs mit anderer
   Authentifizierung werden übersprungen.
3. Jeden **Access Point**: Custom Field `wlan_group` = die Gruppe. Nur echte APs: ein Gerät mit
   `wlan_group` bekommt die CAP-Konfiguration. Ein Switch, der nur Ports im WLAN-Netz hat, braucht
   das Feld nicht, ihm reicht das VLAN an den Ports. Ein alter AP mit dem `wireless`-Paket (z.B.
   SXT, RB912) bekommt seine Radios als `wlan<N>`-Interfaces (Typ IEEE 802.11a/g/n/ac). Er wird
   dann CAP des klassischen CAPsMAN auf dem Controller und bringt die Clients selbst in die VLAN
   ihres WLANs, wie ein CAP von `/interface/wifi` (Variante auf dem Controller, das VXLAN trägt sie
   dorthin). Ohne `wlan<N>`-Interfaces in NetBox behält ein solcher AP seine CAP-Konfiguration,
   wie sie ist.
4. Für die Variante auf dem Controller: Die VLAN-Gruppe der WLAN-VLANs bekommt eine `vxlan_vni`
   (siehe [VXLAN](#vxlan)). Der AP bringt die Clients jedes WLANs in dessen VLAN.
5. Das `country` der WLANs kommt aus der Region des Standorts des Controllers.

## Was nicht ausgewertet wird

Diese Felder werden gelesen oder sind in NetBox vorhanden, beeinflussen die Config aber nicht. Es
reicht also nicht, sie zu setzen:

- **Interface `enabled`**: wird abgefragt, aber ignoriert. Ein in NetBox deaktiviertes Interface
  wird trotzdem konfiguriert.
- **Device Role** (`router`, `switch`, `wlan-bridge`): wird gelesen, aber nirgends verwendet.
  Router und Switch unterscheiden sich nur durch ihre Interfaces.
- **Device Type**: wird nicht gelesen, das Board kommt live vom Gerät.
- **802.1Q Mode**: siehe oben; nur die VLAN-Zuweisungen zählen.
- **Tags an Bridge-Ports**: bei Bridges zählen die Tags des Bridge-Interfaces und des
  VLAN-Interfaces, das die IP trägt, nicht die der Mitgliedsports.
- **VLANs ohne VLAN-Gruppe**, **WLAN-Gruppen ohne Controller**.
- **L2VPNs**: werden nicht gelesen, VXLANs kommen aus der VLAN-Gruppe.
