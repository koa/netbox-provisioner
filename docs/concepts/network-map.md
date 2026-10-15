# Konzept: Netz-Ansicht (Ist-Zustand, Health, PoE)

Status: Konzept, nicht umgesetzt. Offener Punkt aus [pending.md](../pending.md#visualisierung-netz-zustand).

Eine Seite pro Site: das Netz als Graph, Soll aus NetBox, Ist live aus dem Netz. Knoten sind
Geräte, Kanten Kabel und Funkbrücken. Farben zeigen den Zustand, ein Klick auf einen Knoten zeigt
Health und PoE.

## Gemessen (CHR und Lab, 2026-10-15, nur lesend)

Die Datenquellen unten sind nicht geraten. Gelesen auf einer 3-Knoten-CHR-Kette
(`ospf_chain.rs`) und auf allen acht Lab-Geräten:

- **LSAs:** `/routing/ospf/lsa/print` auf lab-core liefert 480 LSAs (rund 150 kB) in 0,04 s. Ein
  Abfragen alle 5 s ist also billig. Darin stehen nicht nur die 8 Lab-Router, sondern alle 32 Router
  der Area: Das Lab hängt über `e01-uplink` in derselben Area wie die Produktion (vgl.
  `propagate_default_route`).
- **Live-Änderungen:** `/routing/ospf/lsa/listen` hat auf der CHR in 90 s nichts gemeldet,
  obwohl sich das Router-LSA des Nachbarn geändert hat (Link wurde zu `type=stub`). Es bleibt also
  beim Abfragen, nicht beim Zuhören.
- **Health** unterscheidet sich je nach Board. RouterOS 7 liefert aber immer Zeilen
  `name`/`value`/`type` (Einheit):
  - CCR2004 (lab-core): `cpu-temperature`, `sfp-temperature`, `switch-temperature`,
    `board-temperature1/2`, `fan-state`, `fan1/2-speed`, `psu1-state`, `psu2-state`. Im Lab steht
    **`psu1-state=fail`**, vermutlich nur ein Netzteil angeschlossen.
  - CRS318-16P (lab-np1/np2): `cpu-temperature`, `psu1/2-voltage`, `psu1/2-current`,
    `poe-out-consumption` (6,4 W bzw. 10,4 W).
  - SXT, wAP, RB912: nur `voltage` (23–23,2 V über PoE) und `temperature`.
  - CHR: keine Werte.
- **PoE pro Port:** `/interface/ethernet/poe/monitor numbers=… once` liefert pro Port
  `poe-out-status` (`powered-on`, `waiting-for-load`, …) sowie Spannung (V), Strom (mA) und
  Leistung (W). Auf lab-np1 ergeben `e01-sxt-a` 3,3 W und `e02-ap1` 2,9 W zusammen genau die
  `poe-out-consumption` von 6,4 W.
- **NetBox** kennt PoE nur am Interface: `poe_mode` (PSE/PD) und `poe_type` (802.3af/at/bt,
  passiv 24/48 V). Power Ports mit `maximum_draw`/`allocated_draw` sind für die Netzversorgung,
  nicht für PoE. Der Provisioner liest heute `poe_mode` (`PowerChain`).

## Daten

### Ist-Zustand der Strecken aus den LSAs (OSPFv2)

Die Abfrage läuft auf *einem* Router, standardmässig dem Gerät des Management-Proxys, weil es
direkt erreichbar ist. Ist es nicht erreichbar, nimmt sie den nächsten OSPF-Router der Site.

- **Router-LSA** (eines pro Router, `originator` = Router-ID = `primary_ip_v4` aus NetBox):
  - Zeilen `type=network id=<DR-Adresse> data=<eigene Adresse>` sind ein Transit-Netz mit
    Nachbarn;
  - Zeilen `type=stub id=<Netz> data=<Maske>` sind ein Port ohne Nachbarn (so auf der CHR, als
    das Gegenüber abgeschaltet war);
  - Zeilen `type=p2p` gibt es nur bei Point-to-Point-Interfaces.
- **Network-LSA** (eines pro Broadcast-Segment): `netmask` und die Router-IDs aller angeschlossenen
  Router.

Daraus ergibt sich der Zustand einer Kante aus NetBox (Kabel oder Funkbrücke zwischen Port A und
Port B):

| Zustand | Bedingung |
|---|---|
| grün | Die Router beider Enden stehen im selben Network-LSA, und zwar in dem, dessen Netz die Adresse von A enthält. |
| rot | Ein Ende meldet für das Netz `stub` oder gar nichts, oder ein Router fehlt ganz im LSDB. |
| orange | OSPFv2 ist oben, OSPFv3 nicht (siehe unten), oder Fehlerzähler oder `link-downs` des Ports steigen (Gerätedaten, siehe unten). |

Ein L2-Segment über mehrere Kabel geht auch: Im Lab steht im Network-LSA `192.168.101.12` lab-np1,
lab-sxt-a, lab-sxt-b und lab-np2, weil die Funkbrücke auf L2 durchgeht. Alle drei Kanten der Kette
sind grün. Fällt die Funkbrücke, zerfällt das Segment in zwei Network-LSAs, und nur die
Funkbrücken-Kante wird rot.

Ein Port ohne OSPF (ein Kunden-Port wie `e04-kunde` auf lab-np2) erscheint nicht in den LSAs.
Seine Kante bekommt den Zustand aus den Gerätedaten des Switches: `running` und bei PoE
`poe-out-status`.

### OSPFv3 als zweite Meinung

Die OSPFv3-Router-LSAs nennen keine Adressen, sondern `neighbor-router-id` und `iface-id`. Damit
lässt sich nur „Router X ist Nachbar von Router Y“ zuordnen. Zwei parallele Kabel zwischen
denselben Geräten lassen sich nicht unterscheiden. Darum ist v2 die Grundlage, und v3 dient als
Gegenprobe: Kante oben in v2, aber nicht in v3, heisst orange. Das hätte den Fall von lab-ap1 mit
abgeschaltetem IPv6 sofort gezeigt (`docs/ospf-generation.md`). Der Wunsch „wahlweise v2 oder v3“
aus `pending.md` wird so zu „v2 für die Zuordnung, v3 als Prüfung“. Ein Umschalter ist trotzdem
möglich.

### Router ausserhalb von NetBox

Router-IDs im LSDB ohne Gerät in der Site (im Lab die 24 Router der Produktion) werden zu *einem*
Sammelknoten „ausserhalb (24 Router)“ an der Kante, über die sie hereinkommen. Mehr zeigt die
Ansicht nicht davon an, die Produktion ist nicht ihr Gegenstand.

### Gerätedaten (Health, Ressourcen, PoE)

Pro Gerät alle 30 s, nur lesend, über denselben Weg wie heute (`DeviceAccess::connect`):

- `/system/health/print`: alle Zeilen, generisch als `(name, value, unit)`;
- `/system/resource/print`: `uptime`, `cpu-load`, `free-memory`/`total-memory`,
  `free-hdd-space`, `version`;
- `/interface/ethernet/poe/monitor … once` auf Geräten mit PoE-Ausgang (aus `hw_facts.rs`);
- `/interface/print`: `running`, `link-downs`, `rx-error`/`tx-error` für die Kanten ohne OSPF und
  für „orange“.

Bei 30 Geräten sind das etwa 4 Anfragen pro Sekunde über den Management-Proxy. Ein Gerät, das
nicht antwortet, wird nicht schneller wiederholt als die anderen.

### Zustand eines Knotens

| Zustand | Bedingung |
|---|---|
| grün | erreichbar, Router-ID im LSDB, keine Health-Warnung |
| rot | weder erreichbar noch im LSDB |
| orange | erreichbar mit Problemen: Health-Warnung, nur über OSPF sichtbar aber API nicht erreichbar (oder umgekehrt), übrig gebliebener Fallback-Scheduler |

Warnungen aus Health, generisch nach Einheit und Name, mit Grenzwerten in der Konfiguration:

- Temperatur (`C`): ab 70 °C orange, ab 85 °C rot.
- `*-state` ungleich `ok`: orange.
- Spannung mehr als 10 % unter dem gewohnten Wert: orange.
- `cpu-load` über 80 % in drei Abfragen nacheinander: orange.

Ein Netzteil, das absichtlich nicht angeschlossen ist (`psu1-state=fail` auf lab-core), würde so
dauerhaft orange leuchten. Abhilfe: In NetBox die Power Ports des Geräts modellieren, und ein
`psuN-state=fail` gilt nur als Warnung, wenn Power Port N verkabelt ist.

„Gerät entspricht NetBox“ gehört nicht zum Zustand: Den Diff zu berechnen kostet zu viel für alle
30 s. Ein Abzeichen zeigt den zuletzt gesehenen Diff-Zustand von der Geräteseite.

## PoE

- **Kante PSE → PD:** zusätzlich zur Kabel-Kante ein Pfeil vom speisenden zum gespeisten Gerät
  (aus `poe_mode`, wie `PowerChain`), mit animiert fliessender Strichelung. Die Dicke folgt der
  Leistung, die Beschriftung zeigt sie („3,3 W“).
- **Soll gegen Ist:**
  - `poe_mode=pse` in NetBox, aber `poe-out-status` ist nicht `powered-on`: orange.
  - Strom fliesst an einem Port ohne `pse` in NetBox: orange („nicht modelliert“).
- **Am speisenden Gerät:** Summe (`poe-out-consumption`) gegen Budget. Das Budget pro Board kommt
  aus `hw_facts.rs`, wo die Board-Fakten liegen; die Werte müssen pro Board nachgetragen werden.
- Die PoE-Kette ist dieselbe, die Updates beachten. Ein Update, das auf ein gespeistes Gerät
  wartet, kann die Ansicht direkt an der Kante zeigen.

## Darstellung (Frontend, Yew)

- SVG, Knoten mit Icon nach NetBox-Rolle (`DeviceRole`: Router, Switch, Funk, AP, Endgerät) und
  dem Namen darunter; Kantenfarbe nach Zustand; Funkbrücken gestrichelt.
- **Layout:** kräftebasiert (Fruchterman-Reingold), selbst in Rust geschrieben. Bei einigen Dutzend
  Knoten reichen etwa 150 Zeilen, ohne JavaScript-Bibliothek. Ein Knoten, den der Benutzer
  verschoben hat, bleibt fest; die übrigen ordnet das Layout um ihn herum.
- **Klick auf einen Knoten:** Seitenleiste mit Health (alle Zeilen), Ressourcen, PoE pro Port,
  Version und einem Link auf die Geräteseite.
- **Klick auf eine Kante:** beide Ports, OSPF-Zustand v2/v3, Fehlerzähler, bei PoE die Leistung.
- **Aktualisierung:** Die Seite fragt alle 5 s nach (wie die Job-Seite jede Sekunde). Der
  Provisioner hat bisher keine Subscriptions (`EmptySubscription`), und Polling reicht hier.

### Positionen speichern

Der Provisioner hat keine Datenbank. Möglich sind:

1. **Datei** `network-layout.json` neben `config.yaml` (Einstellung `layout_file`). Einfach; gilt
   für alle Benutzer.
2. **Custom Field** `map_position` (JSON) am Device in NetBox. Überlebt jede Neuinstallation des
   Provisioners, braucht aber ein NetBox-Token mit Schreibrecht. Der Provisioner schreibt heute nie
   nach NetBox.
3. **Browser** (`localStorage`): pro Benutzer und Browser, geht verloren.

Vorschlag: Datei (1). Positionen sind Darstellung, keine Netzdaten, und der Provisioner bleibt
gegenüber NetBox nur lesend.

## Backend

- Neues Modul `network_state`: ein Hintergrund-Task pro Site. Er startet, wenn jemand die Ansicht
  öffnet, und läuft weiter, solange in der letzten Minute jemand gefragt hat. LSDB alle 5 s,
  Gerätedaten alle 30 s, beides im Speicher.
- Parser für den LSA-`body` (Zeilen `key=value`, eingerückte Link-Zeilen) in `mikrotik-model`,
  wo `RosValue` lebt, mit Tests aus den hier gemessenen Zeilen.
- GraphQL: `Query.networkState(siteId)` → Knoten (Gerät, Zustand, Gründe, Health, Ressourcen,
  PoE) und Kanten (Ports, Art, Zustand v2/v3, PoE-Leistung), plus `measuredAt`. Mutation
  `saveNodePosition(deviceId, x, y)`.
- Alles nur lesend; `allow_apply` ist dafür nicht nötig.

## Alternative: Log-Aggregator (spätere Stufe)

Die Idee aus `pending.md`: Alle Router schicken ihre OSPF-Meldungen per Syslog
(`/system/logging action` mit `remote`, Topic `ospf`) an den Provisioner. Vorteil: Ereignisse im
Moment, in dem sie passieren, und eine Historie („Funkbrücke 3-mal in der letzten Stunde
weggefallen“). Nachteile: Logging muss in die generierte Konfiguration jedes Geräts, Syslog über
UDP geht genau dann verloren, wenn die Strecke dazwischen ausfällt, und den aktuellen Zustand
muss man trotzdem abfragen. Vorschlag: zuerst Polling wie oben. Syslog später nur für die
Historie, ergänzend.

## Reihenfolge der Umsetzung

1. LSA-Parser und Zuordnung zu NetBox-Kanten, mit Unit-Tests aus den gemessenen Daten und einem
   CHR-Test (Link abschalten → Kante rot).
2. `networkState` mit Kanten und Knoten nur aus dem LSDB, SVG-Ansicht mit festem Layout.
3. Gerätedaten: Health, Ressourcen, Seitenleiste.
4. PoE-Kanten und -Budget.
5. Verschiebbare Knoten, gespeicherte Positionen.

## Zu entscheiden

- Wo die Positionen liegen (Vorschlag: Datei).
- Grenzwerte für Health: global in `config.yaml`, oder pro Gerät über den NetBox Config Context?
  Vorschlag: global, mit Config Context als spätere Übersteuerung.
- Power Ports in NetBox modellieren, damit `psuN-state` richtig bewertet wird?
