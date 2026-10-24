# Konzept: Interfaces nach Aufgabe

Status: Schritte 1 bis 3 umgesetzt (Zuordnung und Ansicht, `topology/interfaces.rs`,
`components/interfaces/`; die Helfer im Modal, `components/helper_flow.rs`; die
Live-Prüfungen, `device/interface_live.rs`), der Rest offen. Ersetzt auf der [Geräteseite](device-page.md) die Karte
„NetBox ergänzen“ mit ihrer Auswahl einzelner Helfer ([Config-Helper](config-helper.md)). Die
Helfer selbst (Plan, Vorschau, Schreiben, Rückgängig) bleiben, sie werden nur anders aufgerufen.

## Warum

Heute wählt man zuerst einen Helfer und dann ein Interface. Was an einem Interface fehlt, sieht
man erst im Plan, und ob es danach stimmt, erst in den Befunden der Site. Gewünscht ist die
umgekehrte Sicht: Das Gerät zeigt seine Interfaces, sortiert nach ihrer Aufgabe. Jedes sagt, was
geprüft ist und was nicht passt. Von dort geht es mit einem Klick zum passenden Lösungsvorschlag.

## Aufgaben

Der Server ordnet jedes Interface genau einer Aufgabe zu, in dieser Reihenfolge (die erste, die
passt, gilt):

| Aufgabe | erkannt an |
|---|---|
| Loopback | Name `lo` |
| Uplink | Tag `dhcp-client` oder `nat` (später `internet`) |
| Richtfunk-Verbindung | das Interface oder ein Port seiner Bridge ist ein Ende eines Wireless Links |
| Kunden-Anschluss | untagged VLAN am Interface, oder Tag `dhcp` |
| Direktverbindung | Kabel (auch über Patch-Panels) zu einem Port eines anderen RouterOS-Geräts (Router, AP, Switch), das kein Brückengerät ist |
| Richtfunk-Verbindung | Kabel zu einem Brückengerät |
| Kunden-Anschluss | Adresse in einem Prefix, das weder selbst noch über ein Eltern-Prefix im Pool `transfer`/`loopback` liegt |
| Unbekannt | alles andere, auch freie Ports |

Ein Brückengerät hat die Rolle `wlan-bridge` oder genau einen Funk-Port mit RF-Rolle oder
Wireless Link. Die Rolle allein reicht nicht: Die Brückengeräte im Lab haben die Rolle `router`,
im gespeicherten Export haben viele Link-Enden keine RF-Rolle (das ist dann der Befund
`RadioWithoutRole`, die Zeile bleibt Richtfunk).

Die Reihenfolge weicht vom ersten Entwurf ab (Direkt vor Kunden), wegen des gespeicherten
Exports: Eine Kunden-Bridge eines Switches hat an ihren Ports andere RouterOS-Geräte (`1og-sw1`
`switch-lan`, VLAN 1), darum zählen VLAN und `dhcp` vor den Kabeln. Die Links dort liegen dagegen
oft in Prefixes ohne Pool (`router-hegglin` `172.17.0.16/29`), darum zählt das Prefix erst nach
den Kabeln. Was der Export ergibt, schreibt der Test `interfaces_of_netbox_example` nach
`target/netbox-example/interfaces.txt`.

- **Bridges:** Gezählt wird das L3-Interface. Eine Bridge bekommt die Aufgabe über ihre Ports
  und Adressen; ihre Ports stehen als Unterzeilen in derselben Zeile, nicht als eigene Zeilen.
- **Brückengeräte:** Auf der Seite eines Brückengeräts zeigt der Funk-Port die Strecke von
  dort aus. Der Port zum Router zählt zur Richtfunk-Verbindung. Geprüft wird dasselbe wie vom
  Router aus.
- **Eigene APs und Switches** hängen über eine Direktverbindung (L3, `ospf`) am Router, nicht
  über einen Trunk. Ihre Kunden-VLANs bekommen sie über VXLAN.

Die Zuordnung rechnet der Server aus der Topologie, wie die Befunde (`topology/findings.rs`).
Das Frontend rechnet nichts selbst.

## Was geprüft wird

Jede Prüfung hat einen von vier Zuständen: erfüllt, Warnung, Fehler, nicht geprüft (Gerät nicht
erreichbar oder noch am Lesen). Zu jeder gibt es einen Satz, was erwartet wird und was gefunden
wurde: „Gegenstelle laut NetBox `lab-np2 ether3`, das Gerät sieht dort `lab-wap ether1`“. Die
Prüfungen aus NetBox sind dieselben wie die Befunde. Die Ansicht gruppiert die Befunde pro
Interface und rechnet sie nicht ein zweites Mal. Neue Prüfungen kommen deshalb als neue Befunde
dazu und zeigen sich dann auch auf der Site-Seite.

### Loopback

- `lo` vorhanden.
- IPv4 `/32` und IPv6 `/128` darauf, je als Primary IP des Geräts.
- `ospf` an mindestens einem Interface (sonst kündigt niemand die Loopback an,
  `LoopbackWithoutOspf`).

Fehlt etwas: **„Loopback ergänzen“** öffnet den Helfer „Loopback“.

### Uplink

Uplinks werden vollständig in NetBox modelliert. Der Provisioner erkennt und zeigt sie, ändert
sie aber nicht: keine Lösungsvorschläge, kein Entfernen, nur „In NetBox öffnen“.

- Angezeigt: Port, Adresse bzw. `dhcp-client`, `nat`, live die Adresse und das Gateway vom
  Gerät (`/ip/dhcp-client`, Default-Route).
- Geprüft wie die übrigen Befunde, später mit denen der [Firewall](firewall.md) (`nat` ohne
  `internet`). Für einen Befund verweist die Zeile auf NetBox statt auf einen Helfer.

### Direktverbindung

- Netz: an beiden Enden je eine IPv4 im selben Netz, das Netz ist ein `/31` (ein `/30` gilt,
  mit Hinweis) und als Prefix im Transfer-Pool (`NoCommonSubnet`, `AddressWithoutPrefix`).
- `ospf` an beiden Enden (`OspfOnOneEnd`), gleiche MTU (`MtuMismatch`).
- Label je der Name der Gegenseite (Hinweis).
- Live: Das Gerät sieht über den Port den Nachbarn, den NetBox als Gegenstelle hat
  (`/ip/neighbor`, wie „Nachbarn lesen“).
- Live: OSPF-Nachbar `Full` auf dem Port.

Aktionen:

- **„Lösungsvorschlag“** öffnet den Helfer „Verbindung“ mit beiden Enden vorbelegt. Er ergänzt
  nur, was fehlt.
- Sieht das Gerät einen anderen Nachbarn: **„Kabel zu `lab-wap ether1` übernehmen“**. Das ist
  „Verbindung abräumen“, danach „Verbindung“ mit dem gesehenen Nachbarn, in einem Plan.
- **„Verbindung entfernen“** öffnet den Helfer „Verbindung abräumen“.

### Richtfunk-Verbindung

Die Strecke vom Router-Port aus:

```
lab-np1 e01 ── lab-sxt-a ether1 ┄┄ lab-link-3b ┄┄ lab-sxt-b ether1 ── lab-np2 e01
```

- Kette vollständig: Kabel zum Brückengerät, dort Port und Funk-Port in einer Bridge, ein
  Wireless Link zum zweiten Brückengerät, von dort ein Kabel zu einem Router-Port.
- Funk: RF-Rolle an beiden Enden, je ein AP und eine Station (`RadioWithoutRole`), SSID,
  WPA mit PSK.
- Netz: ein Netz (`/29`) mit je einer Adresse an allen vier L3-Enden (zwei Router-Ports, zwei
  Bridges) und `ospf` an allen vieren. Das Prefix liegt im Transfer-Pool.
- Brückengeräte: Loopback als Primary IP, wie bei „Loopback“.
- MTU gleich an beiden Kabeln.
- Live: Der Router sieht über den Port den Router der Gegenseite (über eine Brücke sieht er
  nicht das Brückengerät, siehe [Config-Helper](config-helper.md#netbox-gegen-gerät)).
- Live: OSPF-Nachbar `Full` zum Router der Gegenseite, beide Brückengeräte antworten auf Ping
  an ihre Loopback.

Aktionen:

- **„Lösungsvorschlag“** öffnet den Helfer „Funkbrücke“. Heute startet man ihn auf der Seite
  eines Brückengeräts. Neu kommt der Aufruf vom Router-Port her dazu: Das verkabelte
  Brückengerät und die Gegenseite des Wireless Links sind dann vorbelegt.
- **„Verbindung entfernen“** (neuer Helfer „Richtfunk abräumen“): die Adressen im `/29`, das
  Prefix, `ospf` und die Labels an den vier Enden. Kabel, Brückengeräte und Wireless Link
  bleiben. Den Wireless Link nimmt der Helfer nur auf Wunsch mit weg (Schalter im Plan).

### Kunden-Anschluss

- L2: Mode access mit einem untagged VLAN am Port bzw. an seiner Bridge; das VLAN in einer
  VLAN-Gruppe (`VlanWithoutGroup`). **Tagged VLANs an physischen Ports sind nicht vorgesehen:**
  Muss ein VLAN auf L2 zu einem anderen Gerät, geht es über VXLAN (`vxlan_vni` an der
  VLAN-Gruppe). Ein Port mit tagged VLANs ist ein Befund (Warnung), mit Verweis auf VXLAN.
- VLAN ist wahlfrei: Ein Port ohne VLAN mit Adresse ist ein Kunden-Anschluss ohne VLAN.
- L3 auf diesem Gerät, wenn es eine Adresse hat:
  - IPv4: Prefix mit Scope der Site, die erste Adresse als Gateway, `dhcp`, Pool nicht leer
    (`DhcpPoolEmpty`);
  - IPv6: `/64` mit `::1`, angekündigt über `dhcp`;
  - jede Familie ist wahlfrei, eine genügt.
- L3 anderswo: Hat das Gerät für das VLAN keine Adresse, zeigt die Zeile, wo der L3-Übergang
  liegt („L3 auf `lab-np1`“, mit Link zu dessen Geräteseite).
- **Genau ein L3-Übergang pro VLAN im Netz:** über alle Geräte, auf denen das VLAN liegt (mit
  `vxlan_vni` die ganze VLAN-Gruppe). Zwei Geräte mit einer Adresse in dem VLAN sind ein Fehler,
  keins eine Warnung („VLAN ohne Gateway“). Das gilt auch für Switches: Ein Switch, der ein VLAN
  nur auf L2 bereitstellt, hat dort keine Adresse und ist aus dem VLAN nicht direkt erreichbar,
  nur über den Router des VLANs (seine Loopback). Heute lehnt der Generator nur zwei *verschiedene*
  Netze und zwei DHCP-Server in einem VLAN ab (`MultipleNetworksOnVlan`,
  `MultipleDhcpServersOnVlan`). Zwei Geräte mit Adressen im selben Netz lässt er durch. Die neue
  Regel ist strenger; sie kommt als Befund dazu, nicht als Generatorfehler.

Aktionen:

- **„Lösungsvorschlag“** öffnet den Helfer „Kunden-Netz“ mit Port bzw. VLAN vorbelegt. Fehlt nur
  L2 (VLAN am Port), schlägt er „VLAN am Port setzen“ vor (neu, wie `setPortVlan` im
  [Self-Service](self-service.md)).
- **„Verbindung entfernen“** (neuer Helfer „Kunden-Anschluss abräumen“): Adressen, das Prefix,
  wenn danach nichts mehr darin ist, `dhcp`, das virtuelle VLAN-Interface, wenn es sonst nichts
  trägt, und die VLANs am Port. Das VLAN-Objekt selbst bleibt.

### Unbekannt

- Die Zeile zeigt, was NetBox am Interface hat (Kabel, Adressen, Tags, VLANs), und live, wen das
  Gerät dort sieht.
- **„Rolle zuweisen“** bietet Kunden-Netz, Direktverbindung oder Richtfunk an. Die gewählte
  Rolle öffnet den passenden Helfer mit dem Interface vorbelegt.
- Passt der gesehene Nachbar zu einer Rolle, ist sie vorgewählt: ein Router mit dem Port in
  NetBox ergibt Direktverbindung, ein Brückengerät ergibt Richtfunk. Das entspricht dem heutigen
  „Als Verbindung übernehmen“.
- **Freie Ports**, ohne Kabel und ohne Daten in NetBox, stehen zusammengefasst unter einer
  aufklappbaren Zeile („12 freie Ports“), sonst gehen die Interfaces mit Daten zwischen ihnen
  unter. Aufgeklappt hat jeder seine eigene Zeile mit denselben Aktionen wie eine unbekannte.

### Aktionen pro Port

Alles geht vom Port aus (2026-10-24, statt des Menüs „Weitere Helfer“): jede Zeile hat genau
die Aktionen, die auf diesem Port möglich sind, und der Helfer startet mit ihm.

| Port | Aktionen |
|---|---|
| frei oder unbekannt (Router) | Als Kunden-Anschluss, Als Direktverbindung; an einem Ethernet-Port Als VLAN am Port, ohne Kabel zu einem Router Als Richtfunk, ohne Kabel oder offen an einem Patchpanel Neues Gerät anschliessen; PoE, Flags |
| frei oder unbekannt (Brückengerät) | Als Richtfunk; PoE, Flags |
| Direktverbindung | Lösungsvorschlag (mit Befund), Verbindung entfernen; PoE, Flags |
| Richtfunk | Lösungsvorschlag (unvollständig oder mit Befund), Verbindung entfernen; PoE, Flags |
| Kunden-Anschluss | VLAN am Port (`VlanWithoutPort`), Kunden-Netz, Verbindung entfernen; PoE, Flags |
| Port einer Bridge | PoE; in einem Kunden-Anschluss auch Aus Bridge lösen |
| aus der Live-Prüfung | Kabel übernehmen, Verbindung mit dem gesehenen Router, Neues Gerät anschliessen für ein unbekanntes MikroTik an einem freien Port, Adresse übernehmen für eine, die nur auf dem Gerät steht ([Config-Helper](config-helper.md#adresse-übernehmen)), Lease übernehmen für einen statischen DHCP-Lease, den nur der Router hat ([Config-Helper](config-helper.md#lease-übernehmen)) |
| Uplink | keine (Z25) |
| Loopback | Loopback, nur wo sie fehlt |

- **Ports einer Bridge** stehen eingerückt unter ihr (`InterfaceRow.portRows`, je mit Kabel und
  Aktionen). Ihre Tags zählen nicht, die der Bridge schon: darum dort keine Flags. Den Port
  eines Kunden-Anschlusses nimmt „Aus Bridge lösen“ allein heraus (siehe
  [Config-Helper](config-helper.md#aus-bridge-lösen)); die Ports der Bridge eines Brückengeräts
  bleiben, ohne sie gibt es keinen Richtfunk.
- **PoE** gibt es an jedem Ethernet-Port, mit dem Port vorbelegt und der Gegenseite aus dem
  Kabel. Ob das Board dort PoE-out hat, weiss NetBox nicht (siehe
  [Config-Helper](config-helper.md#poe)).
- **Flags** zeigt nur die vier Schalter dieses Interfaces, die Befunde danach wie bisher.
- **Als Richtfunk** von einem freien Router-Port fragt zuerst nach dem Brückengerät (genau ein
  Radio, kein Kabel zu einem Router) und bettet dann dessen Helfer ein, mit Router und
  Router-Port vorbelegt (`HelperPreset::RadioChoose`, danach `RadioFrom`). Der Plan läuft wie
  vom verkabelten Router-Port im Job des Brückengeräts.
- Die andere Brücke und ihr Router werden weiter im Dialog gewählt: sie liegen auf anderen
  Geräten.
- **Die Dialoge** fragen den Port, von dem sie starten, nicht mehr (2026-10-24): er steht im
  Titel. Übrig bleibt bei Verbindung die Gegenseite und ihr Port mit der MTU, bei Verbindung
  entfernen nichts, bei Kunden-Netz Prefixe, Länge und DHCP, bei VLAN am Port Gruppe und VLAN,
  bei PoE Richtung und Gegenseite („Vom Gerät ablesen“ zeigt nur diesen Port), bei der
  Funkbrücke vom Router-Port her alles ausser Router und Router-Port der eigenen Seite. Von der
  Seite eines Brückengeräts und für VLAN am Port von einem VLAN-Interface her bleibt die
  Auswahl, dort ist kein Port gegeben (`HelperPreset::fixes_port`).

## Oberfläche (PatternFly, patternfly-yew 0.8)

### Platz auf der Geräteseite

Karte **„Interfaces“** über die volle Breite, an der Stelle von „NetBox ergänzen“. PoE und
Flags öffnen vom Port aus, je für diesen einen Port (bis 2026-10-24 aus dem Menü „Weitere
Helfer“ im Kartenkopf, das es nicht mehr gibt).

```
┌ Interfaces ─────────────────────────────────────────────────────────── [Live prüfen ⟳] ┐
│ [Alle 15] [Loopback 1] [Uplink 1] [Direkt 2] [Richtfunk 1] [Kunden 3] [Unbekannt 7]   │
│ ☐ nur Befunde                                                                         │
│                                                                                       │
│ Loopback                                                                              │
│   lo   IPv4 192.168.100.3/32 ✓ Primary   IPv6 fd00::3/128 ✓ Primary   ● in Ordnung    │
│                                                                                       │
│ Direktverbindungen (2)                                                                │
│ ▸ s01-np2   lab-np2 s01    192.168.101.0/31   MTU 1600   ▲ 1 Warnung          ⋮      │
│ ▾ e03-wap   lab-wap ether1 192.168.101.2/31   MTU 1500   ✖ 1 Fehler           ⋮      │
│     ✓ beide Enden im selben /31, Prefix im Transfer-Pool                              │
│     ✓ ospf an beiden Enden · ✓ MTU gleich                                             │
│     ✖ Gegenstelle: NetBox lab-wap ether1, Gerät sieht lab-ap1 ether1                  │
│     ○ OSPF-Nachbar: nicht geprüft (kein Nachbar)                                      │
│     [Kabel zu lab-ap1 ether1 übernehmen]  [Verbindung entfernen]                      │
│                                                                                       │
│ Richtfunk (1)                                                                         │
│ ▸ e01-sxt-a  lab-sxt-a ┄ lab-link-3b ┄ lab-sxt-b → lab-np2 e01  192.168.101.8/29  ● ok │
│ …                                                                                     │
│ Unbekannt (7)                                                                         │
│ ▸ ether7     kein Kabel · sieht lab-np3 ether2         ○ ohne Rolle                ⋮  │
│ ▾ 5 freie Ports                                                                       │
│ ▸ ether8     kein Kabel · sieht niemanden              ● in Ordnung                ⋮  │
└───────────────────────────────────────────────────────────────────────────────────────┘
```

- **Filter:** `ToggleGroup` mit einem Eintrag pro Aufgabe und der Anzahl, dazu `Switch` „nur
  mit Befund“. Zuerst steht eine Aufgabe mit Fehlern. Die Wahl merkt sich der Browser
  (`localStorage`, nur Komfort).
- **Pro Aufgabe** ein `Title` (h3) mit `Badge` und darunter eine Tabelle (`Table`,
  `TableMode::CompactExpandable`, auf schmalen Bildschirmen `grid`). Die Loopback ist keine
  Tabelle, sondern eine Liste ihrer Angaben (wie in der aufgeklappten Zeile; die
  `DescriptionList` von PatternFly liess zu viel Platz). Fehlt `lo`, steht dort ein `EmptyState`
  mit dem Knopf „Loopback anlegen“. Leere Aufgaben sind ausgeblendet, ausser der Loopback.
- **Spalten** je Aufgabe:

  | Aufgabe | Spalten |
  |---|---|
  | Uplink | Port · Adresse / DHCP-Client · NAT · Gateway (live) · Zustand · In NetBox öffnen |
  | Direkt | Port · Gegenstelle (Link zur Geräteseite) · Netz · MTU · Zustand · Aktionen |
  | Richtfunk | Port · Strecke · Netz · Zustand · Aktionen |
  | Kunden | Port/VLAN-Interface · VLAN (untagged) · IPv4 · IPv6 · DHCP · Zustand · Aktionen |
  | Unbekannt | Port · NetBox hat · Gerät sieht · Rolle zuweisen |

- **Zustand:** ein `Label` mit Farbe und Icon, nicht nur Farbe:

  | Zustand | Label |
  |---|---|
  | in Ordnung | grün, `check-circle` |
  | Warnungen | orange, `exclamation-triangle`, „2 Warnungen“ |
  | Fehler | rot, `exclamation-circle`, „1 Fehler“ |
  | in NetBox geändert, noch nicht angewendet | blau, `info-circle` |
  | nur aus NetBox geprüft (live fehlt) | grau, mit Zusatz „ohne Gerät“ |

  Auf breiten Bildschirmen zeigt ein `Popover` am Label die fehlgeschlagenen Prüfungen. Die
  vollständige Liste steht in der aufgeklappten Zeile: kein Hover, damit es auf dem Handy auch
  geht.
- **Aufgeklappte Zeile:** die Prüfungen als `List` (plain) mit Icon, Satz „erwartet / gefunden“
  und, wo es eins gibt, einem Link nach NetBox. Darunter die Knöpfe der Lösungsvorschläge
  (`Button` primary für den ersten, secondary für die übrigen). „Verbindung entfernen“ ist ein
  Button vom Typ danger, `ButtonVariant::Danger`.
- **Aktionen-Spalte:** ein Kebab-Menü (`MenuToggle` plain mit `Menu`) mit
  „Lösungsvorschlag“, „Verbindung entfernen“, „In NetBox öffnen“. So bleibt die Zeile schmal; die
  Knöpfe in der aufgeklappten Zeile sind dieselben.
- **Strecke** (Richtfunk) als Folge von kleinen `Label`s (Gerät) mit Kabel-/Funk-Zeichen
  dazwischen, auf dem Handy umbrechend.

### Vom Lösungsvorschlag zum Gerät

Ein Klick auf einen Lösungsvorschlag öffnet ein `Modal` (`ModalVariant::Large`, auf dem Handy volle
Breite) mit einem `ProgressStepper` oben: **Plan → Vorschau → In NetBox schreiben → Anwenden**.

1. **Plan:** das Formular des Helfers, vorbelegt aus der Zeile. Bei einem reinen Vorschlag gibt
   es nichts einzugeben, der Plan steht sofort da. Darunter die Liste der NetBox-Änderungen
   (anlegen / ändern / löschen, alt → neu).
2. **Vorschau:** die Prüfungen dieser Zeile, wie sie nach dem Plan wären
   (`topology.findings(after:)`, wie heute bei den Flags), und das RouterOS-Skript der
   betroffenen Geräte aufklappbar (`ExpandableSection` mit `CodeBlock`). Grün vor dem Schreiben
   ist die eigentliche Rückmeldung, die heute fehlt.
3. **Schreiben:** der Job mit seinen Events wie heute. Schlägt er fehl, zeigt das Modal, was
   geschrieben ist, und „Rückgängig“.
4. **Anwenden:** „Jetzt anwenden“ springt zur Karte „Änderungen“ (bei einer Strecke
   „Gemeinsam anwenden“). „Später“ schliesst das Modal.

Nach dem Schreiben lädt die Seite die Topologie neu. Die Zeile zeigt das blaue Label „in NetBox
geändert, noch nicht angewendet“ und darin einen `Alert` (inline, info) mit „Anwenden“ und
„Rückgängig“. Das blaue Label bleibt, bis die Vorschau des Geräts keine Änderung mehr hat. Danach
zeigt die Zeile wieder ihre Prüfungen.

### Live-Prüfungen

Die Karte zeigt zuerst, was aus NetBox kommt; das ist sofort da. Die Live-Prüfungen
(Nachbarn, OSPF, Ping der Brückengeräte) lädt sie danach in einer eigenen Abfrage. Bis die
Antwort da ist, zeigt die Spalte „Gerät sieht“ bzw. die Prüfung einen `Spinner` (klein). „Live
prüfen“ fragt neu. Ist das Gerät nicht erreichbar, stehen die Live-Prüfungen auf „nicht geprüft“
(grau). Sie zählen dann nicht als Fehler.

## Technik

- **Eine Abfrage für die Ansicht**, `device.interfaces(deviceId)` (GraphQL, nur NetBox):
  - pro Interface die Aufgabe und die Gegenstelle bzw. Strecke;
  - die Prüfungen als `{ id, state, expected, actual, netboxUrl }`;
  - die Lösungsvorschläge als fertige Helfer-Eingaben (`HelperChoice`) und die Entfernen-Aktion.

  Das Frontend schickt eine Eingabe unverändert an `plan` weiter. So bleibt die Entscheidung,
  was ein Vorschlag tut, im Backend.
- **Live getrennt**, `device.interfaceLive(deviceId)`: Nachbarn und OSPF-Nachbarn pro Interface,
  aus derselben Lesung wie „Nachbarn lesen“. Das Frontend fügt beide über die Interface-ID
  zusammen.
- **Prüfungen = Befunde:** Die Ansicht ruft dieselben Prüfungen auf wie `findings.rs`, gefiltert
  auf das Gerät und gruppiert nach `InterfaceRef`. Neu dazu kommen die Befunde „ein L3-Übergang
  pro VLAN“, „tagged VLAN an einem physischen Port“, „Label nennt die Gegenseite nicht“,
  „Strecke unvollständig“ und „Brückengerät ohne Loopback“. Sie erscheinen dann auch auf der Site-Seite.
- **Neue Helfer** (keine für Uplinks): „Richtfunk abräumen“, „Kunden-Anschluss abräumen“,
  „VLAN am Port setzen“, der Aufruf von „Funkbrücke“ vom Router-Port her, und „Kabel übernehmen“
  als ein Plan aus Abräumen und Verbindung.
- **Frontend:** `netbox_helper.rs` (über 2000 Zeilen) wird aufgeteilt (erledigt 2026-10-24,
  `components/netbox_helper/`, ein Modul pro Helfer mit Formular): die Ansicht
  (`components/interfaces/`: `rows.rs` das Modell, `table.rs` die Tabelle mit den Spalten pro
  Aufgabe, `loopback.rs`; eine Datei pro Aufgabe lohnte sich nicht, die Aufgaben unterscheiden
  sich nur in den Spalten) und das Modal mit dem Ablauf (`components/helper_flow.rs`). Die heutigen Formulare der Helfer ziehen in das Modal um.

## Reihenfolge

1. Zuordnung und Ansicht nur lesend: Aufgaben (Uplinks eingeschlossen), Prüfungen aus den
   vorhandenen Befunden, ohne Live-Prüfungen und ohne Knöpfe. Danach Screenshots schmal und breit im Lab.
   Erledigt 2026-10-23.
2. Lösungsvorschläge über die vorhandenen Helfer (Loopback, Verbindung, Abräumen,
   Kunden-Netz, Funkbrücke) im Modal mit Stepper; die Karte „NetBox ergänzen“ fällt weg.
   Erledigt 2026-10-23, so:
   - Das Backend gibt jeder Zeile ihre Aktionen (`InterfaceRow.actions`, die der Loopback in
     `InterfaceView.loopbackActions`, auch ohne `lo`): Art (Lösungsvorschlag, Verbindung
     entfernen, Rolle zuweisen), Helfer und Vorbelegung (Port, Gegenstelle, VLAN). Direkt: ein
     Vorschlag „Verbindung“ mit beiden Enden, nur mit Befund, und „Verbindung entfernen“.
     Kunden: „Kunden-Netz“ auf VLAN bzw. Port, nur ohne L3 anderswo und solange eine Familie
     fehlt. Richtfunk: „Funkbrücke“ nur auf dem Brückengerät, unvollständig oder mit Befund;
     vom Router-Port her und das Abräumen kommen mit Schritt 5. Unbekannt (nicht frei): „Als
     Kunden-Anschluss“, „Als Direktverbindung“, auf einem Brückengerät „Als Richtfunk“.
     Uplinks: keine.
   - Das Modal bettet den bisherigen Helfer ein (`NetboxHelper` mit `embedded`, ohne Karte
     und Auswahl, `preset`), statt seine Formulare umzuziehen: Plan, Vorschau, Schreiben und
     Rückgängig bleiben an einer Stelle; die Phase meldet er dem Stepper (`on_phase`). Die
     Vorschau ist die bisherige (Befunde vorher/nachher, Skript), noch nicht die Prüfungen
     der Zeile allein.
   - „Weitere Helfer“ im Kartenkopf öffnete jeden der sieben Helfer, nicht nur PoE und Flags:
     so ging ohne die alte Karte nichts verloren. Seit 2026-10-24 geht alles vom Port aus
     (siehe [Aktionen pro Port](#aktionen-pro-port)), das Menü ist weg.
   - Das blaue Label bleibt, bis ein Anwenden des Geräts fertig ist (nicht, bis die Vorschau
     leer ist), und nur im Browser, bis die Seite neu lädt. „Rückgängig“ in der Zeile öffnet
     den Helfer wieder, der die letzte Schreibung des Geräts zurücknimmt.
   - Die Zeilen von „Nachbarn lesen“ öffnen den Helfer weiter als Karte darunter.
3. Live-Prüfungen (Nachbarn, OSPF, Ping der Brückengeräte). Erledigt 2026-10-23, so:
   - `AccessibleDevice.interfaceLive` liest Nachbarn (wie „Nachbarn lesen“, dieselben
     Befunde), `/routing/ospf/neighbor`, `/ip/address` und pingt die Brückengeräte einer
     Strecke an ihrer Primary IPv4. Gruppiert wird im Backend auf die Zeilen der Ansicht
     (`DeviceAccess::interface_rows`); das Frontend fügt über das Interface der Zeile zusammen.
   - Ein OSPF-Nachbar gehört zur Zeile seines Interfaces. RouterOS nennt Ports nach Kurzname
     und Label (`e01-sxt-a`), Bridges und VLAN-Interfaces so, wie L2 sie erzeugt
     (`bridge-link` für NetBox' `link`): darum ausser dem Portnamen über die Adressen, die
     `/ip/address` dort hat. Sein Gerät ist das mit der Router-ID als Primary IPv4.
   - Erwartet werden auf einer Zeile mit `ospf` die RouterOS-Geräte an den Kabeln (Direkt)
     bzw. alle Geräte der Strecke mit `ospf` (Richtfunk, die Brückengeräte sind OSPF-Router
     über ihre Bridge). Benachbart heisst `Full` oder `TwoWay` zwischen zwei DROthers, wie
     nach einem Update (`update_fallback::is_adjacent`). Fehlt einer, ist das ein Fehler,
     ebenso ein Brückengerät, das nicht antwortet.
   - „Gerät sieht“ steht als Spalte nur bei „Unbekannt“, sonst in der aufgeklappten Zeile.
     Nicht erreichbar: ein Hinweis über den Aufgaben mit dem Grund, die Zeilen grau „ohne
     Gerät“.
   - Im Lab zeigte es gleich einen echten Zustand: lab-sxt-a `e01-lab-np1` läuft nicht
     (`running=false`), lab-np1 hat auf `e01-sxt-a` keinen OSPF-Nachbarn; die Brückengeräte
     antworten über den 60-GHz-Weg.
4. Die neuen Befunde (ein L3-Übergang pro VLAN, tagged VLAN an einem Port, Strecke
   unvollständig, …). Erledigt 2026-10-23, so:
   - Ein VLAN ist ein Netz pro Bereich (`topology/vlan_domain.rs`): mit `vxlan_vni` alle
     RouterOS-Geräte mit dem VLAN, ohne die Geräte, die Kabel mit dem VLAN an beiden Enden
     verbinden. Dasselbe VLAN lokal auf zwei Geräten sind so zwei Netze, je mit eigenem
     Gateway. Zwei Geräte mit Adresse in einem Bereich sind ein Fehler
     (`VlanWithSeveralGateways`), keins eine Warnung (`VlanWithoutGateway`). Ausgenommen ist das
     Management-VLAN einer WLAN-Gruppe (`wlan_mgmt`): CAPsMAN findet seine CAPs darin auf L2.
     „L3 auf …“ der Kunden-Zeile zählt ebenso pro Bereich.
   - `TaggedVlanOnPort`: tagged VLANs an einem Ethernet-Port, eigene oder (ohne eigenes VLAN)
     die seiner Bridge, Warnung mit Verweis auf VXLAN.
   - `RadioPathIncomplete` (Warnung) aus dem `RadioPath` der Richtfunk-Zeile eines
     Brückengeräts: es fehlt das Kabel zum Router, der Wireless Link, seine SSID oder das Kabel
     auf der Gegenseite; einmal pro Strecke.
   - Brückengerät ohne Loopback: der bestehende `NoLoopback`, auf einem Brückengerät ein
     Fehler, es ist nur über die Loopback erreichbar.
   - `LabelNotFarEnd` (Hinweis): ein Port mit Kabel zu einem RouterOS-Gerät, dessen Label das
     Gerät nicht nennt (ohne Gross/Klein und Satzzeichen, eins im anderen enthalten). Im
     gespeicherten Export sind das 58, meist Ports ohne Label.
   - Im Export kam keiner der beiden VLAN-Fehler und kein tagged Port vor; dazu drei Strecken
     ohne SSID und sieben Brückengeräte ohne Loopback, die nun Fehler sind.
5. Die neuen Helfer: Richtfunk abräumen, Kunden-Anschluss abräumen, VLAN am Port, Funkbrücke
   vom Router-Port, Kabel übernehmen. Je ein Commit:
   - Funkbrücke vom Router-Port, erledigt 2026-10-23: Die Richtfunk-Zeile eines Routers
     bietet den Vorschlag „Funkbrücke“ (unvollständig oder mit Befund), mit dem Router-Port als
     `interface` und dem Port des verkabelten Brückengeräts als `peer`. Das Modal bettet dann
     den Helfer dieses Brückengeräts ein (`HelperPreset::RadioFrom`, Router und Router-Port
     vorbelegt; erreicht an seiner eigenen Adresse). Der Helfer setzt nun auch die Labels an
     beiden Kabeln, sonst behöbe der Vorschlag den Label-Hinweis nicht.
   - Richtfunk abräumen, erledigt 2026-10-23: „Verbindung entfernen“ auf jeder
     Richtfunk-Zeile mit Funk-Port (Router wie Brückengerät), Helfer `radio_disconnect` (siehe
     [Config-Helper](config-helper.md#richtfunk-abräumen)). Zum Segment gehören auch Adressen aus
     dem Transfer-Pool an nur einem Ende: im Export hat bei schopf-burri nur der Router eine. Der
     Wireless Link geht mit einem Schalter im Plan, neu als Schritt `DeleteWirelessLink`.
     Er braucht den Funk-Port der Zeile.
   - Kunden-Anschluss abräumen, erledigt 2026-10-23: „Verbindung entfernen“ auf jeder
     Kunden-Zeile, Helfer `customer_disconnect` (siehe
     [Config-Helper](config-helper.md#kunden-anschluss-abräumen)), mit den neuen Schritten
     `UpdatePortVlan` (taugt auch für „VLAN am Port“) und `DeleteInterface`. NetBox nimmt
     `mode: null` zum Entfernen des Modes (4.7.1, im Lab an einem Test-Interface geprüft). Im
     Lab mit Anwenden geprüft: lab-np2 `lan` abgeräumt und mit „Kunden-Netz“ neu angelegt,
     lab-wap `ether2` (nur das VLAN) abgeräumt und per Rückgängig wiederhergestellt.
   - VLAN am Port, erledigt 2026-10-24: Helfer `port_vlan` (siehe
     [Config-Helper](config-helper.md#vlan-am-port)). „Fehlt nur L2“ ist ein neuer Befund,
     `VlanWithoutPort` (Warnung): ein VLAN-Interface mit Adresse, dessen VLAN kein anderes
     Interface des Geräts führt, in keinem WLAN liegt und dessen Bereich (`vlan_domain`) nur das
     Gerät umfasst; das Netz erreicht niemanden. Auf seiner Kunden-Zeile ist „VLAN am Port“ der
     Lösungsvorschlag mit dem VLAN; auf einer unbekannten Zeile eines Ethernet-Ports kommt es zu
     „Rolle zuweisen“ dazu, mit dem Port, ebenso auf einem freien Port. Im
     gespeicherten Export kommt der Befund nicht vor.
   - Kabel übernehmen, erledigt 2026-10-24: Helfer `cable_takeover` (siehe
     [Config-Helper](config-helper.md#verbindung)). Die Vorschläge kommen aus der Live-Prüfung
     (`LiveRow.actions`), nicht aus der Ansicht: Sieht das Gerät an einem Port genau einen Router,
     ist es bei `CableElsewhere` und `OtherFarPort` „Kabel übernehmen“, bei `CableMissing`
     „Verbindung“ mit beiden Enden. Sie stehen vor den Aktionen der Zeile und ersetzen dort eine
     mit demselben Helfer.

## Entschieden (2026-10-23)

- **Uplinks** sind eine eigene Aufgabe, nur angezeigt und geprüft. Modelliert werden sie
  vollständig in NetBox, Helfer gibt es dafür keine.
- **Keine tagged VLANs an physischen Ports.** L2 zu einem anderen Gerät geht über VXLAN; eigene
  APs und Switches hängen über eine Direktverbindung. Zeigen sich damit echte Nachteile, kommt
  das wieder auf den Tisch.
- **Ein L3-Übergang pro VLAN** ist ein Fehler, auch für Switches: Ein Switch, der ein VLAN nur auf
  L2 bereitstellt, hat darin keine Adresse und ist nur über den Router des VLANs erreichbar.
