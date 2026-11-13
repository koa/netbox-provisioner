# Netz-Ansicht (Ist-Zustand, Health, PoE)

Gebaut als C9 (2026-11-01 bis 2026-11-13), alle fünf Schritte der
[Reihenfolge](#reihenfolge-der-umsetzung): LSA-Parser, Zustand der Kanten und Knoten,
`Query.topology.networkState(siteId)`, SVG-Karte, Health und Ressourcen im Popup
(`backend/src/network_state/`, `components::network_map`), PSE→PD-Pfeile mit Soll-gegen-Ist und
die Last der PoE-Gruppen gegen ihre Grenzen, Drag, `localStorage`, `publishLayout`. Nicht
gebaut: der Klick auf eine Kante (keine Seitenleiste, entschieden) und der Log-Aggregator
(spätere Stufe, unten).

Eine Seite pro Site, und eine über alle Sites (`/network-map`, im Site-Menü der Breadcrumb; die
Karte einer Site im Geräte-Menü): das Netz als Graph, Soll aus
NetBox, Ist live aus dem Netz. Knoten sind Geräte, Kanten Kabel und
Funkbrücken; ein Gerät ohne OSPF (`network_state::graphql::supports_ospf`: kein RouterOS, keine
primäre IPv4 oder kein `ospf`-Interface) erscheint auf keiner der beiden Karten, mit seinen
Kanten – **ausser** es trägt, als reiner Switch ohne eigenes OSPF, die Strecke zwischen zwei
Routern, die laut LSDB tatsächlich Nachbarn sind (ein L2-Segment über mehrere Kabel, siehe
unten): ein solcher Switch bleibt mit seinen Kabeln auf der Karte (`NodeState::NotOspf`,
`network_state::graphql::on_map_devices`), und seine beiden Router gelten als erreicht, auch
wenn keines ihrer Kabel für sich allein je `Up` sein kann (dafür bräuchte ein Port eine eigene
OSPF-Adresse, die ein reiner Switch-Port nicht hat). Ein Gerät ohne jede OSPF-Verbindung
dahinter (ein Kundendrucker) bleibt dagegen weg. Farben zeigen den Zustand, Health und
Ressourcen stehen im Tooltip beim Überfahren (**Entschieden 2026-11-02**: keine Seitenleiste,
der Klick bleibt die Navigation auf die Geräteseite, an deren eigener Site, nicht der der
Karte).

## Gemessen (CHR und Lab, 2026-10-15, nur lesend)

Das Lab ist seit 2026-10-31 kleiner (lab-np2 und lab-wap). Auf lab-np2 am 2026-11-01: 503 LSAs,
darunter 43 Router-LSAs v2 (die Produktion über `e15`).

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

Umgesetzt in `network_state::edges` (`EdgeState`: `Up`, `V3Down`, `Down` mit Grund, `NotOspf`).
Dabei gelernt:

- **LSAs bleiben liegen:** Ein Router, der wegfällt, lässt seine LSAs im LSDB der anderen, bis sie
  nach einer Stunde altern. War er DR, steht sein Network-LSA weiter mit beiden Routern da. Ein
  Segment zählt deshalb nur, wenn die Router-LSAs beider Router einen Transit-Link zu seinem DR
  haben (wie im SPF); in v3 entsprechend `neighbor-router-id` = DR. Fehlt das, ist die Kante unten
  (`NoSegment` am nahen, `NotOnSegment` am fernen Ende).
- **Wie schnell:** Ein Port, der abgeschaltet wird, ohne dass die Gegenseite den Link verliert,
  ist erst nach dem Dead-Interval (40 s) unten.
- RouterOS schreibt im v3-Router-LSA `neighbor-iface-if` (sic).
- Der Parser liegt im Backend (`network_state::lsa`), nicht in mikrotik-model: Der `body` ist
  freier Text, kein modelliertes Feld.

Geprüft: Unit-Tests in `network_state` (Zeilen wie auf lab-np2 gelesen) und der CHR-Test
`the_lsdb_gives_the_state_of_each_edge` (Kette aus drei Routern, LSDB vom ersten gelesen; Port
des dritten ab: nur dessen Kante unten; wieder an: beide oben).

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
abgeschaltetem IPv6 sofort gezeigt (`docs/ospf-generation.md`). Der ursprüngliche Wunsch „wahlweise v2
oder v3“ wird so zu „v2 für die Zuordnung, v3 als Prüfung“. Ein Umschalter ist trotzdem
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
| grün | erreichbar, Router-ID im LSDB |
| rot | weder erreichbar noch im LSDB |
| orange | nur über OSPF sichtbar, aber API nicht erreichbar (oder umgekehrt) |

Die Farbe hängt nur an der Erreichbarkeit. Health und Ressourcen werden nicht beurteilt, sondern
angezeigt (beim Überfahren, `i18n::network_map::resources`/`health_row`): keine Grenzwerte,
keine Warnung. So leuchtet auch ein absichtlich nicht angeschlossenes Netzteil
(`psu1-state=fail` auf lab-core) nicht.

„Gerät entspricht NetBox“ gehört nicht zum Zustand: Den Diff zu berechnen kostet zu viel für alle
30 s. Ein Abzeichen zeigt den zuletzt gesehenen Diff-Zustand von der Geräteseite.

## PoE

- **Kante PSE → PD:** zusätzlich zur Kabel-Kante ein Pfeil vom speisenden zum gespeisten Gerät,
  nur wo NetBox ein Kabel zum gespeisten Port hat. **Umgesetzt** (2026-11-02), vereinfacht:
  gestrichelt statt animiert (Strichelung ohne Animation ist dem Projekt genug, siehe
  `local.css`), Farbe statt Dicke für die Leistung (eine Zahl in der Beschriftung, nicht die
  Linienstärke, ist genauer und einfacher). `device::poe::feeding` liefert pro Port Modus, Status,
  Leistung (`poe-out-power`), Gegenseite und eine Abweichung, wenn eine vorliegt; `mismatches`
  (Geräteseite, Soll-gegen-Ist-Check) ist jetzt ein Filter darüber, unverändert in dem, was es
  meldet.
- **Soll gegen Ist** (`helper::poe::PoeProblem`, orange auf der Karte):
  - `poe_mode=pse` in NetBox, aber `poe-out-status` ist nicht `powered-on`: orange.
  - Strom fliesst an einem Port ohne `pse` in NetBox: orange („nicht modelliert“,
    `PoeProblem::Undocumented`, nur für die Karte – `mismatches` meldet das weiterhin nicht, wie
    schon vor dieser Karte).
- **Am speisenden Gerät:** was jede PoE-Gruppe zieht, gegen ihre Grenze, im Popup des Knotens
  (`device::poe::load`). **Umgesetzt** (2026-11-13). MikroTik gibt das Budget als Strom an,
  nicht als Leistung, pro Gruppe von Ports und getrennt unter und über 30 V
  (`hw_facts::poe_groups`, aus den Produktseiten): netPower 16P je 8 Ports 2,8 A / 1,4 A, hEX PoE
  2 A, hAP ac 0,7 A / 0,35 A, hAP ax² 0,6 A, cAP ax 0,6 A / 0,4 A. Ein netPower schaltet die
  Spannung pro Port und hat keinen Wandler: 24 V aus dem Netzteil an der ersten Buchse, 48 V aus
  dem an der zweiten, beide nur mit zwei Netzteilen (Benutzer, 2026-11-13). Darum zählt jeder
  Port nach der Spannung, die er meldet (`poe-out-voltage`), und die beiden Grenzen einer Gruppe
  gelten getrennt (das Handbuch sagt nicht, wie sie zusammenwirken; welche 8 Ports eine Gruppe
  sind, auch nicht: angenommen ether1–8 und ether9–16). Ein Board ohne eingetragene Grenzen
  zeigt nichts statt einer falschen Zahl. RouterOS meldet nur die Summe
  (`poe-out-consumption`), kein Budget. Im Lab an lab-np2 (zwei Netzteile): ether1–8 unter
  30 V 0,11 A von 2,8 A, über 30 V 0,05 A von 1,4 A, ether9–16 über 30 V 0,08 A von 1,4 A.
- Die PoE-Kette ist dieselbe, die Updates beachten. Ein Update, das auf ein gespeistes Gerät
  wartet, kann die Ansicht direkt an der Kante zeigen.

## Darstellung (Frontend, Yew)

- SVG, Knoten mit Icon nach NetBox-Rolle (`DeviceRole`: Router, Switch, Funk, AP, Endgerät) und
  dem Namen darunter; Kantenfarbe nach Zustand; Funkbrücken gestrichelt.
- **Layout:** kräftebasiert (Fruchterman-Reingold), selbst in Rust geschrieben. Bei einigen Dutzend
  Knoten reichen etwa 150 Zeilen, ohne JavaScript-Bibliothek. Ein Knoten, den der Benutzer
  verschoben hat, bleibt fest; die übrigen ordnet das Layout um ihn herum. **Entschieden
  2026-11-02:** so umgesetzt (Schritt 5), nicht das feste Spalten-Layout von Schritt 2
  weiterverwendet. **Der Algorithmus ist umgesetzt** (2026-11-02, `components::network_map::force_layout`,
  gegen das Lab geprüft): Knoten stossen sich ab (`k²/d`), Kanten ziehen ihre Enden zusammen
  (`d²/k`), `k = √(Fläche/n)`; die Schrittweite kühlt über 300 Runden linear auf 0 ab. Ohne
  festgehaltene Position startet ein Knoten nicht zufällig, sondern an einer aus seiner ID
  gehashten Position (MurmurHash3-Finalizer), damit derselbe Graph immer gleich aussieht;
  deterministisch, damit das Ergebnis testbar ist (`network_map::test`, erste Unit-Tests im
  Frontend überhaupt – `cargo test -p frontend --bin frontend`, native, nicht `wasm32`). Die
  Routers ausserhalb NetBox sind ein Knoten im Graphen wie jeder andere, mit Kanten zu den
  Site-Routern, an deren Segment sie grenzen – kein eigener Platz mehr nötig. **Verschieben per
  Drag, umgesetzt** (2026-11-02, gegen das Lab geprüft): `onpointerdown`/`-move`/`-up`, die
  Position per `getBoundingClientRect()` der `<svg>` in deren Koordinatenraum umgerechnet (Yews
  Events sind an einer gemeinsamen Wurzel delegiert, `event.target()` plus `.closest("svg")`
  statt `current_target()`, das sonst irgendeinen entfernten Vorfahren träfe). Ein verschobener
  Knoten geht als `pinned` in `force_layout` ein (einzelne Site). Über alle Sites legt
  `clustered_layout` die Site des Knotens ein zweites Mal aus, mit dem Pin in deren lokale
  Koordinaten zurückgerechnet (Verschiebung und Schrumpfung des ersten Laufs umgekehrt,
  näherungsweise), sodass die übrigen Geräte darauf reagieren; die genaue Position wird danach
  gesetzt. Ein ganzer Site-Kreis lässt sich ebenso ziehen: alle Geräte darin verschieben sich
  gleich (je Gerät gespeichert wie beim einzelnen Ziehen), die Kreismitte zusätzlich je Site.
  Jede Bewegung rechnet von der Lage beim Greifen plus der gesamten Zeigerbewegung, nicht
  schrittweise vom letzten Ergebnis: zwischen zwei Renderings kommen mehrere `pointermove`, und
  schrittweise ging dabei Bewegung verloren. Festgehalten wird im Browser (`localStorage`,
  je Seite ein Schlüssel, Override gewinnt über das offizielle Layout), publiziert mit
  „Anordnung veröffentlichen“ (sichtbar nur mit `allowNetboxWrite`) über die Mutation
  `publishLayout(positions)` nach `map_position`; „Eigene Anordnung zurücksetzen“ verwirft die
  Browser-Overrides wieder. Die Aktionsleiste und beide Buttons: `components::network_map`
  (`Interaction`, `PublishState`); der gemeinsame `localStorage`-Zugriff liegt jetzt in
  `local_storage.rs` (zuvor in `components::device::interfaces`, dort weiterverwendet).
- **Sites gruppiert:** über alle Sites zieht `clustered_layout` die Geräte jeder Site in einen
  eigenen, grau gestrichelten Kreis (benannt), die farbigen Kanten bleiben auf Geräte-Ebene.
  **Entschieden 2026-11-02**, nach einem ersten, unbefriedigenden Versuch ohne das: mit vielen
  kleinen, über Funk quer verbundenen Sites (Produktions-Export, 43 Geräte über 12 Sites)
  überlappten die Kreise stark. **Umgesetzt** (2026-11-02, zweimal verfeinert am selben Tag,
  gegen den Produktions-Export als statische Momentaufnahme geprüft, da das Lab nur eine Site
  hat – `clustered_layout`s Gruppierung greift erst ab zwei Sites unter den Knoten): zweistufig,
  beide Stufen `force_layout` selbst (keine zweite Implementierung). Zuerst die Sites
  gegeneinander, als Knoten einer eigenen Kette (Kante kreuzt Sites → Kante zwischen deren
  Kreisen), mit dem Kreisradius als Mindestabstand in der Abstossung (`force_layout`s
  `radii`/`gap`), statt sie hinterher auseinanderzuschieben: kurze Kanten und keine Überlappung
  kommen so aus derselben Simulation, nicht eines nachträglich auf Kosten des anderen. Dann je
  Site ihre Geräte, wie bisher durch die Kanten innerhalb der Site gezogen, zusätzlich durch
  einen festen Punkt je Nachbar-Site in deren Richtung (weit ausserhalb des Kreises, sodass er
  zieht, aber nie erreicht wird): ein Gerät mit einer Kante zu einer anderen Site landet so auf
  der Seite seines Kreises, die dieser Site zugewandt ist, nicht irgendwo.
  **Überlappung, garantiert keine** (verschärft nach einem ersten Versuch, der nur einen vorab
  aus der Geräteanzahl *geschätzten* Radius an die Site-Abstossung gab: in der Praxis zog das
  Verankern der Geräte zu Nachbar-Sites ihre tatsächliche Ausdehnung teils über die Schätzung
  hinaus, sodass Kreise trotz Abstossung wieder überlappten): derselbe Radius, den die
  Site-Abstossung reserviert, wird nach dem Geräte-Layout erzwungen (`promised_radius`, eine
  einzige Zahl für beides statt zwei auseinanderlaufenden), die Geräte nötigenfalls gleichmässig
  um die Mitte zur Mitte geschrumpft; zusätzlich ein geometrischer Nachlauf (wie in Schritt 5
  vor der kräftebasierten Version), der ein verbliebenes, zu nahes Kreispaar auseinanderschiebt
  – nicht der Hauptmechanismus, sondern eine harte Garantie für den Fall, dass die feste
  Rundenzahl der Simulation nicht reicht. **Kreuzungen, minimiert, nicht vermieden:**
  `reduce_crossings`, ein 2-opt-artiges Vertauschen von je zwei Knotenpositionen, wo das die
  Zahl sich kreuzender Kanten senkt (Kreuzungsminimierung ist NP-hart, daher ein Heuristik, kein
  exaktes Verfahren), auf den Site-Positionen gegeneinander und, separat, je Site mit mehreren
  Geräten auf deren eigenen; bei einem dichten, realen Switch-Schrank (ein gutes Dutzend über
  mehrere Switches verkabelter Geräte) blieben im Produktions-Export einzelne Kreuzungen übrig,
  die das lokale Verfahren nicht mehr auflöste. Die Site-Stufe beginnt seit 2026-11-04 nicht
  mehr bei gehashten Positionen, sondern nach Graph-Tiefe geschichtet (`seed_from_graph`:
  direkt verbundene Sites starten eine Schicht auseinander), und `reduce_site_crossings`
  vertauscht danach ganze Sites samt Geräten, wo das die Kreuzungen der echten Gerätekanten senkt,
  dazu die Kanten, die durch einen dritten Site-Kreis laufen (`count_intrusions`). Auf dem
  Site-Graphen treffen sich alle Kanten einer Site in ihrem einen Punkt und gelten nie als
  Kreuzung, darum diese zweite Runde auf Geräte-Ebene. **Verbindungen aus einer Site hinaus**
  (E21, 2026-11-06): Sie liefen quer durch ihren Kreis, über die übrigen Geräte. Jetzt zählt das
  Vertauschen in der Site die Linie zum Anker jeder fremden Site mit, wie ein Kabel, und dazu jede
  Linie, die durch ein Gerät läuft, an dem sie nicht endet (`count_through_devices`). Ein Gerät mit
  einer Verbindung hinaus liegt am Rand seines Kreises (`RIM`), im Winkel, den die Anker ihm
  gaben. Haben vertauschte Sites ihre Plätze gewechselt, legt die Karte die Geräte der Sites neu
  aus, sonst zeigten sie zum alten Platz der anderen. Zuletzt vertauscht
  `reduce_faults_within_sites` Geräte einer Site auf den echten Kanten an den endgültigen
  Positionen (zwei Linien zur selben Site kreuzten sich sonst unterwegs). Kennzahl im Test
  `the_exports_map_draws_with_few_faults` auf dem Graphen des gespeicherten Exports: vorher
  4 Kreuzungen und 3 Linien durch ein Gerät, jetzt 1 Kreuzung in Berg 1654. Das Bündeln
  mehrerer Verbindungen zwischen zwei Sites (Ansatz 3) war dafür nicht nötig. Das Lab hat nur
  eine Site und zeigt diese Darstellung nicht.
- **Export:** „Als SVG exportieren“ und „Als PNG exportieren“ (`svg_export.rs`) speichern die
  Karte als Datei. Die Live-`<svg>` sieht nur mit dem Stylesheet der Seite richtig aus (Klassen,
  Design-Tokens), darum wird eine Kopie mit dem berechneten Stil jedes Elements als Inline-Stil
  und einem Hintergrund gespeichert; PNG zeichnet dieses SVG in ein Canvas in der Grösse der
  Karte. **Umgesetzt** 2026-11-04.
- **Klick auf einen Knoten:** navigiert auf die Geräteseite (an der eigenen Site des Knotens,
  nicht der der Karte), wie bisher; keine Seitenleiste (**Entschieden 2026-11-02**). Health,
  Ressourcen stehen in einem Popup beim Überfahren; bei einem PSE→PD-Pfeil dessen Leistung und,
  bei Abweichung, welche.
- **Klick auf eine Kante:** beide Ports, OSPF-Zustand v2/v3, Fehlerzähler, bei PoE die Leistung.
  **Nicht umgesetzt** (nur der Tooltip, kein Klick-Verhalten für Kanten, konsistent mit
  „keine Seitenleiste“ oben).
- **Aktualisierung:** Die Seite fragt alle 5 s nach (wie die Job-Seite jede Sekunde). Die
  Subscriptions ([Gerätezustand](device-state.md)) wecken nur bei Änderungen des Caches, die
  Karte zeigt aber Zähler, die sich laufend ändern; Polling reicht hier. **Umgesetzt**
  (2026-11-02), in zwei getrennten Abfragen: `networkState` (Übersicht: Knoten, Kanten,
  OSPF-Zustand) kommt allein aus der LSDB, ohne ein Gerät zu fragen, und ist darum schnell;
  `networkStateDetails` (Health, Ressourcen, PoE-Kanten) braucht pro Gerät eine eigene Anfrage und
  ist entsprechend langsamer. Beide werden unabhängig voneinander alle 5 s neu gefragt, sodass ein
  langsames Gerät nie die Übersicht blockiert. Das Popup eines Knotens zeigt einen Spinner, bis
  die erste Antwort von `networkStateDetails` da ist; danach bleibt es leer, wenn der Knoten dort
  fehlt (kein eigenes OSPF, oder das Gerät hat nicht geantwortet) statt für immer zu drehen.

### Positionen speichern

**Umgesetzt** (2026-11-02), siehe Darstellung oben für die Einzelheiten.

- **Offizielles Layout in NetBox:** ein Custom Field `map_position` (JSON) am Device. Ein Admin
  „publiziert“ seine Anordnung dorthin (Mutation `publishLayout`, Guard `NetboxWritesAllowed`,
  Button nur mit `allowNetboxWrite` sichtbar).
- **Eigene Änderungen im Browser:** Jeder Benutzer startet mit dem offiziellen Layout; was er
  verschiebt, bleibt in seinem Browser (`localStorage`) und lässt sich auf das offizielle
  zurücksetzen.

## Backend

- Neues Modul `network_state`: ein Hintergrund-Task pro Site. Er startet, wenn jemand die Ansicht
  öffnet, und läuft weiter, solange in der letzten Minute jemand gefragt hat. LSDB alle 5 s,
  Gerätedaten alle 30 s, beides im Speicher.
- Parser für den LSA-`body` (Zeilen `key=value`, eingerückte Link-Zeilen) in `mikrotik-model`,
  wo `RosValue` lebt, mit Tests aus den hier gemessenen Zeilen.
- GraphQL: **umgesetzt**, zwei Abfragen statt einer (2026-11-02, siehe Darstellung oben) –
  `Query.networkState(siteId)` → Knoten (Gerät, Zustand) und Kanten (Ports, Art, Zustand v2/v3),
  nur aus der LSDB; `Query.networkStateDetails(siteId)` → Health, Ressourcen und PoE-Kanten, pro
  Gerät gelesen. Mutation `publishLayout(positions)` (schreibt `map_position` je Device, je ein
  REST-PATCH statt der `NetboxPlan`/`helper::write`-Maschinerie: eine einzelne, idempotente,
  nicht umkehrbedürftige Feldänderung braucht deren Fingerprint-Check, Undo-Log oder Job nicht;
  Guard `NetboxWritesAllowed`, **umgesetzt** 2026-11-02). Ohne `siteId`-Parameter, anders als
  hier zuerst notiert: jede `DevicePositionInput` trägt ihre `deviceId` schon selbst, ein
  zusätzliches `siteId` hätte keinen eigenen Nutzen.
- Alles nur lesend; `allow_apply` ist dafür nicht nötig.

## Rollout

Während ein Rollout läuft ([Updates im ganzen Netz](concepts/network-update.md)), hat jeder
Knoten, den er aktualisiert, einen Ring in der Farbe seines Stands (`NetworkState.rollout`);
die Füllung bleibt die Erreichbarkeit. **Umgesetzt** 2026-11-13.

## Alternative: Log-Aggregator (spätere Stufe)

Die ursprüngliche Idee: Alle Router schicken ihre OSPF-Meldungen per Syslog
(`/system/logging action` mit `remote`, Topic `ospf`) an den Provisioner. Vorteil: Ereignisse im
Moment, in dem sie passieren, und eine Historie („Funkbrücke 3-mal in der letzten Stunde
weggefallen“). Nachteile: Logging muss in die generierte Konfiguration jedes Geräts, Syslog über
UDP geht genau dann verloren, wenn die Strecke dazwischen ausfällt, und den aktuellen Zustand
muss man trotzdem abfragen. Vorschlag: zuerst Polling wie oben. Syslog später nur für die
Historie, ergänzend.

## Reihenfolge der Umsetzung

1. LSA-Parser und Zuordnung zu NetBox-Kanten, mit Unit-Tests aus den gemessenen Daten und einem
   CHR-Test (Link abschalten → Kante rot). **Umgesetzt** (2026-11-01).
2. `networkState` mit Kanten und Knoten nur aus dem LSDB, SVG-Ansicht mit festem Layout.
   **Umgesetzt** (2026-11-01): Spalten nach Hops vom gelesenen Router, gegen die Lab/Produktion
   geprüft (lab-np2 liest 43 Router-LSAs v2, zeigt lab-wap als `REACHED` über die Kante
   `ether3`–`ether1` und 41 nicht in NetBox bekannte Router als „ausserhalb“). Dazu eine Karte
   über alle Sites (`siteId` weggelassen), und Geräte ohne OSPF (mit ihren Kanten) auf beiden
   nicht dargestellt – ausser ein reiner Switch, der laut LSDB zwei Nachbarn verbindet, siehe
   oben –, mit Unit-Tests dafür (`network_state::graphql::test`).
3. Gerätedaten: Health, Ressourcen. **Umgesetzt** (2026-11-02): `/system/health/print` und
   `/system/resource/print` alle 30 s pro Gerät (`network_state::device_data`, `FactKind::DeviceMapData`),
   nur für Geräte mit eigenem OSPF (nicht für einen Switch, der nach Schritt 2 nur als Relais
   auf der Karte ist); im Tooltip beim Überfahren statt einer Seitenleiste
   (Entschieden 2026-11-02). Unit-Tests der Zeilen-Auswertung (`device_data::test`).
4. PoE-Kanten und -Budget. **Umgesetzt**: PSE→PD-Pfeile mit Leistung (`poe-out-power`) und
   Soll-gegen-Ist (`device::poe::feeding`, geteilt mit `mismatches` der Geräteseite), im
   Tooltip die Leistung und, bei Abweichung, welche (2026-11-02); die Last der PoE-Gruppen
   gegen ihre Grenzen im Popup des speisenden Knotens (2026-11-13, siehe [PoE](#poe)).
   Unit-Tests in `device::poe::test` (`feeding`, `load`) und `network_state::graphql::test`
   (die Pfeile selbst).
5. Verschiebbare Knoten, Positionen im Browser, Publizieren des offiziellen Layouts.
   **Umgesetzt** (2026-11-02): der Algorithmus (`force_layout`, siehe Darstellung oben), Drag,
   `localStorage`, NetBox-Feld `map_position` und die Mutation `publishLayout`, alles gegen das
   Lab geprüft (Drag einer echten Lab-Site-Karte, Reload, Veröffentlichen, Zurücksetzen; dabei
   zwei Koordinatenfehler gefunden und behoben, siehe Darstellung oben).

## Entschieden (2026-10-23)

- Positionen: Admins **publizieren** das offizielle Layout in ein NetBox Custom Field; andere
  nehmen es oder speichern eigene Änderungen **im Browser**.
- Health wird **nicht beurteilt**, nur angezeigt (Hover oder Popup). Die Farben hängen nur an der
  Erreichbarkeit.
- Power Ports müssen dafür nicht modelliert werden (entfällt mit dem Punkt davor).
