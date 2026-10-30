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
|               | (mit `dhcp-client` auch ein IPv6-DHCP-Client mit Prefix       |
|               | Delegation, siehe Rezept unten); setzt `internet` voraus      |
| `internet`    | Interface zum Internet (Uplink, auch ohne `nat`)              |

An Prefixen markieren zwei Tags, woraus der Config-Helper Adressen vergibt
([Config-Helper](config-helper.md#prefixe)). Die Config ändern sie nicht; die Befunde melden
einen fehlenden oder fast vollen Pool (unter 10 % frei):

| Tag-Slug   | Bedeutung, wenn am Prefix gesetzt                                   |
|------------|---------------------------------------------------------------------|
| `loopback` | daraus kommen die Loopback-Adressen (je ein Prefix für IPv4 und IPv6) |
| `transfer` | daraus kommen die Transfernetze zwischen Routern (nur IPv4)          |

An einem Service (IPAM → Services) öffnet das Tag `forward` seine Ports vom Internet her
([Freigaben](#freigaben-vom-internet-services-mit-forward)); ein Service ohne es bleibt Doku.

An Journal-Einträgen erkennt der Provisioner seine eigenen am Tag `netbox-provisioner`
([Protokoll](protocol.md)); er setzt ihn selbst.

Alle neun Tags lassen sich mit [netbox-import/3-tags.yaml](netbox-import/3-tags.yaml) anlegen
(Customization > Tags > Import, YAML).

`internet` (und `nat`, das es voraussetzt; fehlt es, meldet das der Befund
`NatWithoutInternet`) nimmt das Interface in die Interface-Liste `uplink` auf, die die
Perimeter-Firewall schützt ([firewall.md](firewall.md)): vom Internet herein nur Antworten, ICMP,
SSH mit Bruteforce-Schutz und der Provisioner aus `api_networks`. Auf die Liste können sich auch
von Hand gebaute Firewall-Regeln beziehen (die IPv6-Firewall von router-faesigrund-keller); was
sie hereinlassen, kommt weiter herein, das Verwerfen des Provisioners steht hinter ihnen. Andere
Listen als `uplink` und `maintenance` entfernt ein Apply. `nat` erzeugt zudem `srcnat
action=masquerade out-interface=<Uplink>`.

### Freigaben vom Internet: Services mit `forward`

Ein Service (IPAM → Services) am Gerät, das den Dienst anbietet (ein Gerät ohne RouterOS, z.B.
der Server mit seinem statischen Lease), mit seinen Ports (`tcp/443`, `udp/1194`; NetBox kennt
TCP, UDP und SCTP), seinen IP-Adressen und dem Tag `forward`:

- **IPv4:** Der Router mit `nat` der Site leitet jeden Port vom Uplink an die Adresse weiter,
  aussen derselbe Port wie innen (Z3). Die Site ist die des Prefixes der Adresse (oder des
  nächsten darüber mit Site), sonst die des Geräts. Hat die Site keinen Router mit `nat` oder
  mehrere, ist das der Befund `ForwardWithoutRouter` (Fehler); jeder Port geht pro Router nur
  an einen Service, der mit der kleineren NetBox-ID bekommt ihn, der andere den Befund
  `ForwardPortTaken` (Fehler).
- **IPv6:** Jeder Router mit einem Interface zum Internet lässt die Ports der Adresse herein
  (die Adresse ist überall dieselbe, ein Prefix routet über jeden Uplink).
- Ein Service mit `forward`, aber ohne Adresse oder Port, öffnet nichts: Befund
  `ForwardIncomplete` (Warnung).

Was das auf den Geräten ergibt: [firewall.md](firewall.md#freigaben).

### Config Context `dns_servers`

```json
{"dns_servers": ["172.31.88.2", "2a02:16a:fe00:1a0::10:5", "172.16.1.4", "fd7e:907d:34ab::4"]}
```

Die DNS-Server der Client-Netze, der erste wird zuerst gefragt (z.B. Blocky mit Adfilter, dann
ein Fallback). Zuweisen wie jeder Config Context, z.B. pro Tenant oder Site. Der Provisioner
verteilt die IPv4-Server per DHCP und die IPv6-Server per Router Advertisement (RDNSS), letzteres
nur auf Geräten mit Client-Netz. Jedes Gerät fragt die Server auch selbst
(`/ip dns servers`), beantwortet aber keine DNS-Anfragen (`allow-remote-requests=no`). Steht
eine eigene Adresse des Geräts in der Liste (im Beispiel die Loopbacks von
router-faesigrund-keller), ist es selbst ein Resolver: Es fragt nur die übrigen Server und
beantwortet Anfragen; die Perimeter-Firewall hält das Internet davon fern.

### Config Context `ssh_stage_time`, `ssh_blacklist_time`

```json
{"ssh_stage_time": 60, "ssh_blacklist_time": 600}
```

Die Zeiten des SSH-Bruteforce-Schutzes der Perimeter-Firewall in Sekunden
([firewall.md](firewall.md#perimeter-firewall)): wie lange eine neue SSH-Verbindung ihre Quelle in
einer Stufe hält (ohne: 60) und wie lange eine erwischte Quelle gesperrt bleibt (ohne: 600,
10 Minuten). Länger ist riskant: Wer sich von aussen ein paarmal vertippt, sperrt sich selbst so
lange aus.

### Custom Fields

| Objekt-Typ          | Name                   | Typ                          | Zweck                                         |
|---------------------|------------------------|------------------------------|-----------------------------------------------|
| Tenant              | `mikrotik_credentials` | Text                         | Name eines Zugangsdaten-Eintrags im Provisioner |
| Device              | `wlan_group`           | Objekt → Wireless LAN Group  | Gerät ist Access Point dieser WLAN-Gruppe     |
| Device              | `propagate_default_route` | Boolean                   | OSPF gibt die Default-Route des Geräts weiter |
| Wireless LAN Group  | `controller`           | Objekt → Device              | CAPsMAN-Controller der Gruppe                 |
| Wireless LAN Group  | `wlan_mgmt`            | Objekt → VLAN                | Management-VLAN der Gruppe                    |
| VLAN Group          | `vxlan_vni`            | Integer                      | VLANs der Gruppe über VXLAN verbinden         |
| Prefix              | `dhcp_lease_time`      | Integer (Sekunden)           | Lease-Zeit des DHCP-Servers dieses Netzes     |
| Wireless Link       | `rf_country`           | Text                         | Land beider Enden einer Funkbrücke übersteuern |
| Wireless Link       | `rf_frequency_mode`    | Selection                    | Frequency Mode beider Enden übersteuern       |
| Journal Entry       | `provisioner_job`      | JSON                         | Stand eines Jobs, vom Provisioner geschrieben |

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
| Custom Field `propagate_default_route` | nur am Router mit dem Internet-Uplink: OSPF gibt seine Default-Route weiter; sonst keine (`originate-default=never`) |

Das **Board-Modell** kommt nicht aus NetBox (der Device Type wird nicht ausgewertet), sondern wird
live vom Gerät gelesen (`/system/routerboard`). Unterstützt sind nur die Modelle in
`backend/src/device/ros/hw_facts.rs`: `RB750Gr3`, `RB960PGS`, `RB962UiGS-5HacT2HnT`,
`RBwAPG-5HacD2HnD`, `RBwAPG-5HacT2HnD` (erster wAP ac, nur `wireless`), `RBcAP2n` (cAP 2n, nur `wireless`), `C52iG-5HaxD2HaxD`, `cAPGi-5HaxD2HaxD`, `CRS109-8G-1S-2HnD`, `CRS309-1G-8S+`, `CRS318-16P-2S+`,
`CRS326-24G-2S+`, `CRS354-48G-4S+2Q+`, `CCR1009-7G-1C-1S+`, `CCR1009-8G-1S-1S+`, `CCR2004-1G-12S+2XS`, dazu die Boards
der [Funkbrücken](#funkbrücke) und `RB912UAG-2HPnD`. Für jedes andere Modell bricht die
Generierung mit `NoPortsFound` ab.

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
| `wifi<N>`             | Radio von `/interface/wifi` (`wifi-qcom`, `wifi-qcom-ac`): auf einem AP Radio eines CAP (siehe [WLAN](#wlan)) |
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
| PoE Mode             | `PSE` → PoE-Ausgang `auto-on`, mit einem passiven **PoE Type** (`Passive 24V/48V …`) `forced-on`, sonst `off` (nur auf PoE-fähigen Boards). `auto-on` speist erst, wenn RouterOS eine Last erkennt, eine UISP Wave Nano erkennt es nicht; `forced-on` legt die Spannung ohne Prüfung an, deshalb nur, wenn das Gerät am anderen Ende passives PoE nimmt; das Gerät am anderen Kabelende gilt als von diesem Port gespeist. Ein Update startet das speisende Gerät erst neu, wenn kein gespeistes (auch weiter unten in der Kette) mehr ein Update macht. Würde Anwenden an einem Port mit eingeschaltetem PoE-Ausgang diesen ausschalten, warnt die Geräteseite davor. `PD` am gespeisten Port hält die Gegenseite fest (Helfer „PoE“), der Generator braucht es nicht. `PSE` geht auch ohne Kabel (PoE an, bevor das Gerät dort erfasst ist; „Neues Gerät“ gibt ihm dann `PD`). Die Live-Prüfung der Interfaces meldet `PSE` an einem Port ohne PoE-out oder verkabelt ohne Last, einen speisenden Port, dessen Gegenüber nicht `PD` ist („PoE vervollständigen“), und `PD` an einem Port, an dem das Gerät speist; „Kein PoE“ im Helfer entfernt einen Eintrag samt Gegenstück |
| MTU                  | L3-MTU eines Ethernet-Ports oder `wlan`-Radios, siehe [MTU](#mtu) |
| Speed, Duplex        | feste Geschwindigkeit eines Ethernet-Ports ohne Autonegotiation, siehe [Feste Geschwindigkeit](#feste-geschwindigkeit) |
| RF Role, RF Channel Frequency/Width, TX Power | Funk-Port einer [Funkbrücke](#funkbrücke) |
| Wireless Link        | SSID, PSK und Funkregeln einer [Funkbrücke](#funkbrücke)                   |
| Cable                | Namensgenerierung, wenn kein Label gesetzt ist; Weg für die Ersteinrichtung; der Typ (DAC, Glasfaser, Kupfer) für eine [feste Geschwindigkeit](#feste-geschwindigkeit) |

Der 802.1Q-Mode selbst (Access/Tagged) wird nicht ausgewertet. NetBox verlangt ihn aber, damit
man VLANs zuweisen kann.

### Feste Geschwindigkeit

Ohne `Speed` in NetBox handelt ein Port die Geschwindigkeit aus (`auto-negotiation=yes`). Mit
`Speed` läuft er fest damit, ohne Autonegotiation: Ein QSFP+-Breakout an einem DAC (klr-sw-1
`qsfpplus1-1`) kommt nur so hoch. RouterOS nennt die Geschwindigkeit mit dem Medium
(`10G-baseCR` Twinax, `10G-baseSR-LR` Optik, `10G-baseT` Kupfer); bei einem SFP-/QSFP-Käfig
ergibt es sich aus dem **Typ des Kabels** (`DAC (Passive/Active)`, `SMF…`/`MMF…`/`AOC`,
`CAT…`), ohne Typ schlägt die Generierung fehl. `Duplex` zählt nur bei Kupfer bis 1G
(`Half`, sonst voll).

- **Beide Enden brauchen dieselbe Speed** (wenn am anderen Ende RouterOS läuft), sonst schlägt
  die Generierung fehl: Fest gegen Autonegotiation gibt keinen Link.
- Eine Geschwindigkeit, die das Board am Port nicht anbietet (`advertise`), lehnt die
  Generierung ab.
- Zurück auf Autonegotiation (Speed gelöscht) bleibt die alte `speed` auf dem Gerät stehen:
  RouterOS lehnt sie leer ab und braucht sie dann nicht.

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

MAC-Telnet und Neighbor Discovery laufen auf einem konfigurierten Gerät nur an den Ports der
Interface-Liste `maintenance`: Ports mit Kabel zu einem Gerät mit RouterOS, Funkstrecken und
freie Ports (in NetBox ohne Kabel, Adresse, VLAN und Bridge, oder dort gar nicht vorhanden). An
Kunden-Ports kündigt sich der Router nicht an. Für ein neues Gerät muss der Port des Nachbarn
deshalb in dieser Liste sein: frei, oder mit dem Kabel in NetBox und danach beim Nachbarn
angewendet. Das gilt für MAC-Telnet schon länger, die Discovery folgt jetzt derselben Liste.

Ein Heim-AP (z.B. wAP ac) hat im Werkszustand ether1 als WAN: Firewall, und weder MAC-Telnet
noch Neighbor Discovery auf diesem Port. Der Nachbar sieht ihn dann nicht. So einen AP mit dem
Reset-Knopf in den **CAPs-Modus** zurücksetzen (halten, bis die LED blinkt, dann noch etwa 5 s,
bis sie dauerhaft leuchtet, und loslassen): ether1 ist dann ein normaler Management-Port. Den
CAP, den dieser Modus einschaltet, schaltet die Ersteinrichtung wieder aus (sonst lehnt RouterOS
das Umbenennen der Funkmodule ab); das Anwenden setzt ihn danach so, wie NetBox es vorgibt.

Antwortet das Gerät zwischendurch nicht (es bootet noch oder ist ausgelastet), bricht MAC-Telnet
die Sitzung ab oder meldet sogar ein falsches Passwort. Die Ersteinrichtung versucht es darum
bis zu zwei Minuten lang wieder; erst dann meldet sie „abgelehnt“ oder „keine Antwort“. Falsche
Zugangsdaten zeigen sich also erst nach dieser Zeit. Den Port aus der Werks-Bridge nimmt das
Gerät erst, wenn keine MAC-Telnet-Sitzung mehr offen ist: Eine eigene Sitzung zum neuen Gerät
während der Einrichtung schliessen.

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
- Wo der Switch-Chip VLAN-Filtering in Hardware kann (CRS3xx, RB5009, hEX u.a.), werden die
  Bridges VLANs einer einzigen Bridge, denn jeder Chip ausser CRS1xx/2xx übernimmt nur eine
  Bridge in Hardware; eine Bridge ohne VLAN bekommt eine lokale VLAN-ID ab 4094 abwärts
  ([l2-generation.md](l2-generation.md#nach-dem-switch-chip-l2setupfor_switch_chips)). In NetBox
  ändert sich dafür nichts.
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
- **Lease-Zeit:** Custom Field `dhcp_lease_time` am Prefix, in Sekunden; leer (oder 0) bleibt
  es bei RouterOS' 30 min. Eine kurze Lease braucht ein Netz, dessen Clients bei einem Ausfall
  schnell zu einem anderen Server wechseln sollen (Funkbrücke mit zwei DHCP-Seiten, der Helfer
  setzt dort 120 s).
- **Mehrere IPv4-Netze an einem Interface mit `dhcp`:** RouterOS nimmt nur einen DHCP-Server pro
  Interface. Das erste Netz mit freien Adressen legt ihn an, jedes weitere hängt seine Adressen an
  dessen Pool und bekommt einen eigenen Netz-Eintrag (Gateway, DNS). Welches Netz ein Client
  bekommt, lässt sich nicht steuern; die Lease-Zeit ist die kürzeste der Netze. Ein Netz ohne freie Adresse (ein Transfernetz, dessen
  Adressen alle vergeben sind) bekommt nichts.
- Ein IPv6-/64 an einem Interface mit `dhcp` wird per Router Advertisement angekündigt
  (`advertise=yes`, SLAAC). Andere IPv6-Adressen nicht.
- Gateway des DHCP-Netzes ist die eigene Adresse des Routers auf diesem Interface. DNS-Server
  sind die aus dem Config Context `dns_servers`. Ohne diesen ist es ebenfalls die eigene Adresse,
  und der Router beantwortet DNS-Anfragen (`allow-remote-requests=yes`). Hat er selbst keinen
  `dhcp-client`, lernt er keinen DNS-Server und fragt die Loopbacks der Router mit `nat`-Uplink.
- **Statische DHCP-Leases:** ein Gerät ohne RouterOS (Device Type beliebig, z.B. „Generic“,
  Rolle z.B. „Client“) mit einem Interface, das eine **primäre MAC-Adresse** und eine IPv4 im
  Prefix eines `dhcp`-Interfaces hat, ist ein statischer Lease dieses Servers: Adresse und MAC
  aus NetBox, der Gerätename als Kommentar, sonst RouterOS' Vorgaben. Die Adresse fällt aus dem
  automatischen Pool. Der Provisioner verwaltet die statischen Leases ganz: einer, den NetBox
  nicht kennt, wird beim Anwenden entfernt; dynamische bleiben. Die Live-Prüfung der Interfaces
  zeigt solche vorher, mit „Lease übernehmen“ ([Config-Helper](config-helper.md#lease-übernehmen)).
  MikroTik-Geräte selbst werden nie über eine MAC in NetBox erkannt.

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
   dahinter die eigenen Netze nicht kennt (keine Route zurück). Ist das der Uplink ins Internet,
   am Gerät `propagate_default_route` setzen, damit OSPF die Default-Route an die anderen Router
   weitergibt. Bei einem Uplink in ein Netz, das selbst eine Default-Route hat (z.B. ein Lab am
   produktiven Netz), nicht: die Default-Route ginge sonst in jenes Netz zurück.
   Mit `dhcp-client` und `nat` bekommt der Uplink auch einen IPv6-DHCP-Client, der eine Adresse
   und einen Prefix anfragt (`request=address,prefix`): Der Prefix kommt in den Pool
   `pd-<Interface>` (Stücke von /64), noch nimmt kein Netz daraus; die Default-Route wie beim
   IPv4-Client. Hat das Gerät dort schon einen Client, behält er seine übrigen Einstellungen und
   den Namen seines Pools. Clients an anderen Interfaces bleiben, wie sie sind.

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

Die WLANs einer WLAN-Gruppe können **verschiedene VLANs** haben, jedes WLAN seine eigene. Ein AP
mit 802.11ac-Radios (wAP ac) kann dann nicht mit `wifi-qcom-ac` CAP sein, diese Chips taggen
keine VLANs: Er bekommt seine Radios als `wlan<N>` (siehe [WLAN](#wlan)). Mit einer einzigen VLAN
in der Gruppe geht auch `wifi-qcom-ac`.

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
4. Management: `lo` mit Loopback als Primary IP und eine feste IPv4 an der Bridge im Netz der
   Router-Ports, mit Tag `ospf` an der Bridge: die Brücke ist dann ein OSPF-Router, der seine
   Loopback ankündigt, die Nutzdaten bleiben auf L2. Für IPv6 reichen die Link-Local-Adressen.
   Eine Loopback ohne `ospf` meldet `LoopbackWithoutOspf`. Ein Gerät ganz ohne IP-Adresse und
   ohne Tag `dhcp-client` bekommt stattdessen auf seiner Bridge einen DHCP-Client (siehe
   [IP-Adressen, Prefixe und DHCP](#ip-adressen-prefixe-und-dhcp)).
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

Der Helfer **Funkbrücke** (Karte „NetBox ergänzen“ auf der Seite einer Brücke) trägt das alles
ein, was fehlt: Kabel, Bridge, Namen, RF Role, Wireless Link mit PSK, Loopback, Adressen und
`ospf` ([config-helper.md](config-helper.md#funkbrücke)).

Was daraus generiert wird und worauf beim Ausrollen zu achten ist:
[radio-bridge-generation.md](radio-bridge-generation.md).

Brückengeräte ohne RouterOS (UISP Wave Nano) sind Geräte ohne Platform mit einem Ethernet- und
einem Funk-Interface (NetBox-Typ, Namen beliebig), Kabel zum Router-Port und Wireless Link. Die
Router-Ports haben ein gemeinsames `/31` und je ein eigenes Netz mit `dhcp` und kurzer
`dhcp_lease_time`; die Wave Nanos haben keine Adresse in NetBox. Der Helfer trägt auch das ein
([config-helper.md](config-helper.md#brückengeräte-ohne-routeros)).

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
   wie sie ist. Ein **wAP ac** läuft mit `wifi1`/`wifi2` auf `wifi-qcom-ac`, mit `wlan1`/`wlan2`
   (2,4 und 5 GHz) auf `wireless` als CAP des klassischen CAPsMAN; das braucht er für eine Gruppe
   mit mehreren VLANs. Nach dem Wechsel in NetBox tauscht „Pakete anpassen“ auf der Geräteseite
   die Pakete (ohne die RouterOS-Version zu ändern), danach bringt Anwenden die Konfiguration.
4. Für die Variante auf dem Controller: Die VLAN-Gruppe der WLAN-VLANs bekommt eine `vxlan_vni`
   (siehe [VXLAN](#vxlan)). Der AP bringt die Clients jedes WLANs in dessen VLAN.
5. Das `country` der WLANs kommt aus der Region des Standorts des Controllers.

## Was nicht ausgewertet wird

Diese Felder werden gelesen oder sind in NetBox vorhanden, beeinflussen die Config aber nicht. Es
reicht also nicht, sie zu setzen:

- **Interface `enabled`**: wird abgefragt, aber ignoriert. Ein in NetBox deaktiviertes Interface
  wird trotzdem konfiguriert.
- **Device Role** (`router`, `switch`, `wireless-bridge`): `wireless-bridge` macht ein Gerät zum
  Brückengerät (Interface-Ansicht, „Neues Gerät“ ohne „Verbindung“); die anderen werden gelesen,
  aber nirgends verwendet.
  Router und Switch unterscheiden sich nur durch ihre Interfaces.
- **Device Type**: wird gelesen (für den Config-Helper), das Board kommt aber live vom Gerät.
- **Prefixe**: Beschreibung und Scope (Site) werden gelesen, aber noch nicht ausgewertet; die
  Tags `loopback`/`transfer` nur für die Befunde.
- **802.1Q Mode**: siehe oben; nur die VLAN-Zuweisungen zählen.
- **Tags an Bridge-Ports**: bei Bridges zählen die Tags des Bridge-Interfaces und des
  VLAN-Interfaces, das die IP trägt, nicht die der Mitgliedsports.
- **VLANs ohne VLAN-Gruppe**, **WLAN-Gruppen ohne Controller**.
- **L2VPNs**: werden nicht gelesen, VXLANs kommen aus der VLAN-Gruppe.
