# Konzept: Interfaces nach Aufgabe

Status: Konzept, nicht umgesetzt. Ersetzt auf der [Geräteseite](device-page.md) die Karte
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
| Direktverbindung | Kabel (auch über Patch-Panels) zu einem Port eines anderen RouterOS-Geräts (Router, AP, Switch), das kein Brückengerät ist |
| Richtfunk-Verbindung | Kabel zu einem Brückengerät (Rolle `wlan-bridge` oder genau ein Funk-Port mit RF-Rolle) |
| Kunden-Anschluss | VLAN am Port (Mode access), oder Adresse in einem Prefix ohne Tag `transfer`/`loopback`, oder Tag `dhcp` |
| Unbekannt | alles andere, auch freie Ports |

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
- **Freie Ports**, ohne Kabel und ohne Daten in NetBox, stehen zusammengefasst als eine Zeile
  („12 freie Ports“), aufklappbar. Sonst gehen die Interfaces mit Daten zwischen ihnen unter.

## Oberfläche (PatternFly, patternfly-yew 0.8)

### Platz auf der Geräteseite

Karte **„Interfaces“** über die volle Breite, an der Stelle von „NetBox ergänzen“. PoE und die
Flag-Tabelle passen nicht in eine Zeile pro Interface. Sie bleiben erreichbar über das Menü
„Weitere Helfer“ im Kartenkopf (`CardHeader` mit `actions`, `MenuToggle` mit `Menu`).

```
┌ Interfaces ────────────────────────────────────── [Live prüfen ⟳] [Weitere Helfer ▾] ┐
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
│ ▸ ether7     kein Kabel · sieht lab-np3 ether2         ○ ohne Rolle  [Rolle zuweisen ▾]│
│ ▸ 5 freie Ports                                                                       │
└───────────────────────────────────────────────────────────────────────────────────────┘
```

- **Filter:** `ToggleGroup` mit einem Eintrag pro Aufgabe und der Anzahl, dazu `Switch` „nur
  mit Befund“. Zuerst steht eine Aufgabe mit Fehlern. Die Wahl merkt sich der Browser
  (`localStorage`, nur Komfort).
- **Pro Aufgabe** ein `Title` (h3) mit `Badge` und darunter eine Tabelle (`Table`,
  `TableMode::CompactExpandable`, auf schmalen Bildschirmen `grid`). Die Loopback ist keine
  Tabelle, sondern eine `DescriptionList` (horizontal). Fehlt `lo`, steht dort ein `EmptyState`
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
- **Frontend:** `netbox_helper.rs` (über 2000 Zeilen) wird aufgeteilt: die Ansicht
  (`components/interfaces/`, eine Datei pro Aufgabe) und das Modal mit dem Ablauf
  (`components/helper_flow.rs`). Die heutigen Formulare der Helfer ziehen in das Modal um.

## Reihenfolge

1. Zuordnung und Ansicht nur lesend: Aufgaben (Uplinks eingeschlossen), Prüfungen aus den
   vorhandenen Befunden, ohne Live-Prüfungen und ohne Knöpfe. Danach Screenshots schmal und breit im Lab.
2. Lösungsvorschläge über die vorhandenen Helfer (Loopback, Verbindung, Abräumen,
   Kunden-Netz, Funkbrücke) im Modal mit Stepper; die Karte „NetBox ergänzen“ fällt weg.
3. Live-Prüfungen (Nachbarn, OSPF, Ping der Brückengeräte).
4. Die neuen Befunde (ein L3-Übergang pro VLAN, tagged VLAN an einem Port, Strecke
   unvollständig, …).
5. Die neuen Helfer: Richtfunk abräumen, Kunden-Anschluss abräumen, VLAN am Port, Funkbrücke
   vom Router-Port, Kabel übernehmen.

## Entschieden (2026-10-23)

- **Uplinks** sind eine eigene Aufgabe, nur angezeigt und geprüft. Modelliert werden sie
  vollständig in NetBox, Helfer gibt es dafür keine.
- **Keine tagged VLANs an physischen Ports.** L2 zu einem anderen Gerät geht über VXLAN; eigene
  APs und Switches hängen über eine Direktverbindung. Zeigen sich damit echte Nachteile, kommt
  das wieder auf den Tisch.
- **Ein L3-Übergang pro VLAN** ist ein Fehler, auch für Switches: Ein Switch, der ein VLAN nur auf
  L2 bereitstellt, hat darin keine Adresse und ist nur über den Router des VLANs erreichbar.
