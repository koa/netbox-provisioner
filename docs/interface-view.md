# Interfaces nach Aufgabe

Die Karte „Interfaces“ auf der [Geräteseite](device-page.md) zeigt die Interfaces eines Geräts,
sortiert nach ihrer Aufgabe. Jedes sagt, was geprüft ist und was nicht passt, und von jedem Port
aus geht es mit einem Klick zum passenden Helfer ([Config-Helper](concepts/config-helper.md)).
Die Helfer selbst (Plan, Vorschau, Schreiben, Rückgängig) sind dieselben wie anderswo.

Code: `topology/interfaces.rs` (Zuordnung, Zeilen, Aktionen), `device/interface_live.rs`
(Live-Prüfungen), `components/interfaces/` und `components/helper_flow.rs` (Frontend).

## Aufgaben

Der Server ordnet jedes Interface genau einer Aufgabe zu, in dieser Reihenfolge (die erste, die
passt, gilt):

| Aufgabe | erkannt an |
|---|---|
| Loopback | Name `lo` |
| Uplink | Tag `dhcp-client` oder `nat` |
| Richtfunk-Verbindung | das Interface oder ein Port seiner Bridge ist ein Ende eines Wireless Links |
| Kunden-Anschluss | untagged VLAN am Interface, oder Tag `dhcp` |
| Direktverbindung | Kabel (auch über Patch-Panels) zu einem Port eines anderen RouterOS-Geräts (Router, AP, Switch), das kein Brückengerät ist |
| Richtfunk-Verbindung | Kabel zu einem Brückengerät |
| Kunden-Anschluss | Adresse in einem Prefix, das weder selbst noch über ein Eltern-Prefix im Pool `transfer`/`loopback` liegt |
| Unbekannt | alles andere, auch freie Ports |

Ein **Brückengerät** hat die Rolle `wlan-bridge` oder genau einen Funk-Port mit RF-Rolle oder
Wireless Link. Die Rolle allein reicht nicht: Die Brückengeräte im Lab haben die Rolle `router`,
und im gespeicherten Export haben viele Link-Enden keine RF-Rolle (das ist dann der Befund
`RadioWithoutRole`, die Zeile bleibt Richtfunk).

Die Reihenfolge kommt aus dem gespeicherten Export: Eine Kunden-Bridge eines Switches hat an
ihren Ports andere RouterOS-Geräte (`1og-sw1` `switch-lan`, VLAN 1), darum zählen VLAN und `dhcp`
vor den Kabeln. Die Links dort liegen dagegen oft in Prefixes ohne Pool (`router-hegglin`
`172.17.0.16/29`), darum zählt das Prefix erst nach den Kabeln. Was der Export ergibt, schreibt
der Test `interfaces_of_netbox_example` nach `target/netbox-example/interfaces.txt`.

- **Bridges:** Gezählt wird das L3-Interface. Eine Bridge bekommt die Aufgabe über ihre Ports und
  Adressen; ihre Ports stehen eingerückt unter ihr (`InterfaceRow.portRows`).
- **Brückengeräte:** Auf der Seite eines Brückengeräts zeigt der Funk-Port die Strecke von dort
  aus; der Port zum Router zählt zur Richtfunk-Verbindung. Geprüft wird dasselbe wie vom Router
  aus.
- **Eigene APs und Switches** hängen über eine Direktverbindung (L3, `ospf`) am Router, nicht über
  einen Trunk. Ihre Kunden-VLANs bekommen sie über VXLAN. Tagged VLANs an physischen Ports sind
  nicht vorgesehen; zeigen sich damit echte Nachteile, kommt das wieder auf den Tisch.

Die Zuordnung rechnet der Server aus der Topologie, wie die Befunde (`topology/findings.rs`). Das
Frontend rechnet nichts selbst.

## Was geprüft wird

Jede Prüfung hat einen von vier Zuständen: erfüllt, Warnung, Fehler, nicht geprüft (Gerät nicht
erreichbar oder noch am Lesen), und einen Satz, was erwartet wird und was gefunden wurde. Die
Prüfungen aus NetBox *sind* die Befunde: Die Ansicht gruppiert die Befunde des Geräts pro
Interface und rechnet sie nicht ein zweites Mal. Eine neue Prüfung kommt deshalb als neuer Befund
dazu und zeigt sich auch auf der Site-Seite. Befunde, die keinem Interface gehören, stehen unter
„Weitere Befunde zum Gerät“.

Die Live-Prüfungen (`AccessibleDevice.interfaceLive`) lesen die Nachbarn (wie „Nachbarn lesen“,
dieselben Befunde), `/routing/ospf/neighbor` und `/ip/address` und pingen die Brückengeräte einer
Strecke an ihrer Primary IPv4. Ein OSPF-Nachbar gehört zur Zeile seines Interfaces: RouterOS nennt
Ports nach Kurzname und Label (`e01-sxt-a`), Bridges und VLAN-Interfaces so, wie die
L2-Generierung sie erzeugt (`bridge-link` für NetBox' `link`), darum zählen ausser dem Portnamen
die Adressen, die `/ip/address` dort hat. Sein Gerät ist das mit der Router-ID als Primary IPv4.
Benachbart heisst `Full`, oder `TwoWay` zwischen zwei DROther, wie nach einem Update
(`update_fallback::is_adjacent`).

### Loopback

- `lo` vorhanden, IPv4 `/32` und IPv6 `/128` darauf, je als Primary IP des Geräts.
- `ospf` an mindestens einem Interface (sonst kündigt niemand die Loopback an,
  `LoopbackWithoutOspf`).
- Auf einem Brückengerät ist eine fehlende Loopback ein Fehler (`NoLoopback`): Es ist nur über
  sie erreichbar.

### Uplink

Uplinks werden vollständig in NetBox modelliert. Der Provisioner erkennt und zeigt sie, ändert sie
aber nicht: keine Helfer, nur „In NetBox öffnen“. Angezeigt werden Port, Adresse bzw.
`dhcp-client` und `nat`; geprüft wie die übrigen Befunde.

### Direktverbindung

- Netz: an beiden Enden je eine IPv4 im selben Netz, ein `/31` (ein `/30` gilt, mit Hinweis), als
  Prefix im Transfer-Pool (`NoCommonSubnet`, `AddressWithoutPrefix`).
- `ospf` an beiden Enden (`OspfOnOneEnd`), gleiche MTU (`MtuMismatch`).
- Label: der Name der Gegenseite (`LabelNotFarEnd`, Hinweis; ohne Gross/Klein und Satzzeichen,
  eins im anderen enthalten).
- Live: Das Gerät sieht über den Port den Nachbarn, den NetBox als Gegenstelle hat; OSPF-Nachbar
  benachbart mit den RouterOS-Geräten an den Kabeln.

### Richtfunk-Verbindung

Die Strecke vom Router-Port aus:

```
lab-np1 e01 ── lab-sxt-a ether1 ┄┄ lab-link-3b ┄┄ lab-sxt-b ether1 ── lab-np2 e01
```

- Kette vollständig: Kabel zum Brückengerät, dort Port und Funk-Port in einer Bridge, ein Wireless
  Link mit SSID zum zweiten Brückengerät, von dort ein Kabel zu einem Router-Port
  (`RadioPathIncomplete`, Warnung, einmal pro Strecke).
- Funk: RF-Rolle an beiden Enden, je ein AP und eine Station (`RadioWithoutRole`), WPA mit PSK.
- Netz: ein `/29` mit je einer Adresse an allen vier L3-Enden (zwei Router-Ports, zwei Bridges),
  `ospf` an allen vieren, das Prefix im Transfer-Pool; MTU gleich an beiden Kabeln.
- Live: Der Router sieht über den Port den Router der Gegenseite (über eine Brücke sieht er nicht
  das Brückengerät); OSPF benachbart mit allen Geräten der Strecke mit `ospf` (die Brückengeräte
  sind OSPF-Router über ihre Bridge); beide Brückengeräte antworten auf Ping. Ein fehlender
  Nachbar oder ein stummes Brückengerät ist ein Fehler.

### Kunden-Anschluss

- L2: Mode access mit einem untagged VLAN am Port bzw. an seiner Bridge; das VLAN in einer
  VLAN-Gruppe (`VlanWithoutGroup`). Ein Port mit tagged VLANs, eigenen oder (ohne eigenes VLAN)
  denen seiner Bridge, ist eine Warnung mit Verweis auf VXLAN (`TaggedVlanOnPort`). Das VLAN ist
  wahlfrei: Ein Port ohne VLAN mit Adresse ist ein Kunden-Anschluss ohne VLAN.
- L3 auf diesem Gerät, wenn es eine Adresse hat: IPv4 mit der ersten Adresse als Gateway, `dhcp`,
  Pool nicht leer (`DhcpPoolEmpty`); IPv6 `/64` mit `::1`, angekündigt über `dhcp`. Eine Familie
  genügt.
- L3 anderswo: Hat das Gerät für das VLAN keine Adresse, zeigt die Zeile, wo der L3-Übergang
  liegt („L3 auf `lab-np1`“, mit Link).
- **Genau ein L3-Übergang pro VLAN:** Ein VLAN ist ein Netz pro Bereich (`topology/vlan_domain.rs`):
  mit `vxlan_vni` alle RouterOS-Geräte mit dem VLAN, ohne die Geräte, die Kabel mit dem VLAN an
  beiden Enden verbinden. Dasselbe VLAN lokal auf zwei Geräten sind so zwei Netze, je mit eigenem
  Gateway. Zwei Geräte mit Adresse in einem Bereich sind ein Fehler (`VlanWithSeveralGateways`),
  keins eine Warnung (`VlanWithoutGateway`). Das gilt auch für Switches: Ein Switch, der ein VLAN
  nur auf L2 bereitstellt, hat darin keine Adresse und ist nur über den Router des VLANs
  erreichbar. Ausgenommen ist das Management-VLAN einer WLAN-Gruppe (`wlan_mgmt`): CAPsMAN findet
  seine CAPs darin auf L2.
- Fehlt nur L2 (`VlanWithoutPort`, Warnung): ein VLAN-Interface mit Adresse, dessen VLAN kein
  anderes Interface des Geräts führt, in keinem WLAN liegt und dessen Bereich nur das Gerät
  umfasst; das Netz erreicht niemanden.

### Unbekannt

Die Zeile zeigt, was NetBox am Interface hat (Kabel, Adressen, Tags, VLANs), und live, wen das
Gerät dort sieht. Freie Ports, ohne Kabel und ohne Daten in NetBox, stehen zusammengefasst unter
einer aufklappbaren Zeile („5 freie Ports“); aufgeklappt hat jeder seine eigene Zeile.

## Aktionen pro Port

Jede Zeile hat genau die Aktionen, die auf diesem Port möglich sind, und der Helfer startet mit
ihm. Das Backend gibt sie mit (`InterfaceRow.actions`, die der Loopback in
`InterfaceView.loopbackActions`, die aus der Live-Prüfung in `LiveRow.actions`): Art
(Lösungsvorschlag, Verbindung entfernen, Rolle zuweisen), Helfer und Vorbelegung. So bleibt die
Entscheidung, was ein Vorschlag tut, im Backend.

| Port | Aktionen |
|---|---|
| frei oder unbekannt (Router) | Als Kunden-Anschluss, Als Direktverbindung; an einem Ethernet-Port Als VLAN am Port, ohne Kabel zu einem Router Als Richtfunk, ohne Kabel oder offen an einem Patchpanel Neues Gerät anschliessen; PoE, Flags |
| frei oder unbekannt (Brückengerät) | Als Richtfunk; PoE, Flags |
| Direktverbindung | Lösungsvorschlag (mit Befund), Verbindung entfernen; PoE, Flags |
| Richtfunk | Lösungsvorschlag (unvollständig oder mit Befund), Verbindung entfernen (mit Funk-Port); PoE, Flags |
| Kunden-Anschluss | VLAN am Port (`VlanWithoutPort`), Kunden-Netz (ohne L3 anderswo, solange eine Familie fehlt), Verbindung entfernen; PoE, Flags |
| Port einer Bridge | PoE; in einem Kunden-Anschluss auch Aus Bridge lösen |
| aus der Live-Prüfung | Kabel übernehmen (sieht das Gerät genau einen Router: bei `CableElsewhere` und `OtherFarPort`), Verbindung mit dem gesehenen Router (`CableMissing`), Neues Gerät anschliessen für ein unbekanntes MikroTik an einem freien Port, Adresse übernehmen für eine, die nur auf dem Gerät steht, Lease übernehmen für einen statischen DHCP-Lease, den nur der Router hat |
| Uplink | keine |
| Loopback | Loopback, nur wo sie fehlt |

- Die Aktionen der Live-Prüfung stehen vor denen der Zeile und ersetzen dort eine mit demselben
  Helfer.
- **Ports einer Bridge:** Ihre Tags zählen nicht, die der Bridge schon, darum dort keine Flags.
  Die Ports der Bridge eines Brückengeräts bleiben, ohne sie gibt es keinen Richtfunk.
- **PoE** gibt es an jedem Ethernet-Port, auch ohne Gegenseite (nur `PSE`, das Gerät dort kommt
  später) und zum Entfernen („Kein PoE“). Ob das Board dort PoE-out hat, weiss NetBox nicht; die
  Live-Prüfung vergleicht den Modus mit dem Gerät und schlägt bei einem Widerspruch „PoE-Eintrag
  entfernen“ vor.
- **Flags** zeigt nur die vier Schalter dieses Interfaces.
- **Funkbrücke vom Router-Port:** Die Richtfunk-Zeile eines Routers bettet den Helfer des
  verkabelten Brückengeräts ein (`HelperPreset::RadioFrom`, Router und Router-Port vorbelegt).
  „Als Richtfunk“ von einem freien Router-Port fragt zuerst nach dem Brückengerät (genau ein Radio,
  kein Kabel zu einem Router; `HelperPreset::RadioChoose`). Die andere Brücke und ihr Router
  werden im Dialog gewählt, sie liegen auf anderen Geräten.
- **Die Dialoge** fragen den Port, von dem sie starten, nicht: Er steht im Titel. Von der Seite
  eines Brückengeräts und für VLAN am Port von einem VLAN-Interface her bleibt die Auswahl, dort
  ist kein Port gegeben (`HelperPreset::fixes_port`).

## Oberfläche

- **Filter:** eine `ToggleGroup` mit einem Eintrag pro Aufgabe und der Anzahl, dazu der Schalter
  „nur mit Befund“. Die Wahl merkt sich der Browser (`localStorage`, nur Komfort).
- **Pro Aufgabe** eine Überschrift und eine Tabelle (auf schmalen Bildschirmen umbrechend). Die
  Loopback ist eine Liste ihrer Angaben; fehlt `lo`, steht dort ein `EmptyState` mit dem Knopf.
  Leere Aufgaben sind ausgeblendet, ausser der Loopback.
- **Spalten:**

  | Aufgabe | Spalten |
  |---|---|
  | Uplink | Port · Adresse / DHCP-Client · NAT · Zustand · In NetBox öffnen |
  | Direkt | Port · Gegenstelle (Link zur Geräteseite) · Netz · MTU · Zustand · Aktionen |
  | Richtfunk | Port · Strecke · Netz · Zustand · Aktionen |
  | Kunden | Port/VLAN-Interface · VLAN (untagged) · IPv4 · IPv6 · DHCP · Zustand · Aktionen |
  | Unbekannt | Port · NetBox hat · Gerät sieht · Aktionen |

- **Zustand:** ein `Label` mit Farbe und Icon: in Ordnung (grün), Warnungen (orange), Fehler
  (rot), in NetBox geändert, noch nicht angewendet (blau), ohne Gerät (grau, live nicht geprüft).
- **Aufgeklappte Zeile:** die Prüfungen mit Icon und Satz „erwartet / gefunden“, live, wen das
  Gerät sieht, darunter die Knöpfe der Aktionen; „Verbindung entfernen“ als Danger-Button. Die
  Aktionen-Spalte ist ein Kebab-Menü mit denselben Aktionen.
- **Strecke** (Richtfunk) als Folge kleiner Labels mit Kabel- und Funk-Zeichen dazwischen.
- **Live:** Die Karte zeigt zuerst, was aus NetBox kommt; die Live-Prüfungen lädt sie danach in
  einer eigenen Abfrage, mit einem Spinner, bis sie da sind. „Live prüfen“ fragt neu. Ist das
  Gerät nicht erreichbar, sagt ein Hinweis über den Aufgaben warum, und die Zeilen stehen grau auf
  „ohne Gerät“; das zählt nicht als Fehler.

### Vom Vorschlag zum Gerät

Eine Aktion öffnet ein `Modal` mit einem `ProgressStepper`: **Plan → Vorschau → In NetBox
schreiben → Anwenden**. Das Modal bettet den Helfer ein (`NetboxHelper` mit `embedded` und
`preset`), so bleiben Plan, Vorschau, Schreiben und Rückgängig an einer Stelle; seine Phase meldet
der Helfer dem Stepper. Die Vorschau zeigt die Befunde vorher und nachher und das Skript der
betroffenen Geräte. „Jetzt anwenden“ springt zur Karte „Änderungen“ (bei einer Strecke zu
„Gemeinsam anwenden“), „Später“ schliesst das Modal.

Nach dem Schreiben lädt die Seite die Topologie neu. Die geänderte Zeile zeigt das blaue Label mit
„Anwenden“ und „Rückgängig“ (öffnet den Helfer wieder, der die letzte Schreibung des Geräts
zurücknimmt). Das Label bleibt, bis ein Anwenden des Geräts fertig ist, und nur im Browser, bis
die Seite neu lädt.

## Technik

- `DeviceAccess.interfaceView` (nur NetBox): pro Interface die Aufgabe, die Gegenstelle bzw. Strecke,
  die Befunde und die Aktionen als fertige Helfer-Eingaben, die das Frontend unverändert an `plan`
  weiterschickt.
- `AccessibleDevice.interfaceLive`: die Live-Prüfungen, im Backend auf die Zeilen der Ansicht
  gruppiert (`DeviceAccess::interface_rows`); das Frontend fügt über das Interface der Zeile
  zusammen.
- Frontend: `components/interfaces/` (`rows.rs` das Modell, `table.rs` die Tabelle mit den Spalten
  pro Aufgabe, `loopback.rs`, `live.rs`), `components/helper_flow.rs` (Modal und Ablauf),
  `components/netbox_helper/` (ein Modul pro Helfer mit Formular).
