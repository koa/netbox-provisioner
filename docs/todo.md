# Aufgaben

Alle offenen Aufgaben über die drei Repositories (netbox-provisioner, mikrotik-model,
mikrotik-api), gesammelt aus [pending.md](pending.md), den [Konzepten](concepts/) und der
Durchsichten des Codes vom 2026-10-19 und 2026-11-06. Die Nummern bleiben, auch wenn umsortiert wird; eine
erledigte Aufgabe wandert mit Datum und Commit nach [done.md](done.md), ein erledigter Teil
ebenso, der Rest bleibt hier.

Aufwand grob: **S** Stunden, **M** ein bis zwei Tage, **L** mehr.
Status: **offen**, **in Arbeit**.

## Reihenfolge

Nach C4 (cynic) nochmals durchgesehen (2026-11-06, dieselben Lints): Das Frontend hat rund
2500 Zeilen mehr, weil die Abfragen jetzt Structs sind, dafür geteilte Typen (`types`) und keine
nennenswerten Doppelungen (`cancelJob` nahm noch einen eigenen Typ, behoben). Die langen
Funktionen sind dieselben wie vorher; B15 bis B23 bleiben, wie sie sind, B14 ist im Frontend
kleiner geworden.

## Entscheidungen

Fragen aus den Konzepten und der Durchsicht, mit der Antwort und den Aufgaben, die davon
abhängen. Eine neue Frage kommt mit Status **offen** dazu; entschieden, wird sie mit Datum
markiert und ins Konzept übernommen (dort unter „Entschieden“); sind alle Aufgaben einer
Entscheidung erledigt, wandert sie nach [done.md](done.md#entscheidungen).

| # | Frage | Entscheidung | betrifft |
|---|---|---|---|
| Z4 | Freigaben der internen Firewall: Custom Fields am Interface oder Prefix-Tags? | Custom Fields am Interface (2026-10-23) | C3 |
| Z7 | Befehle ohne Strings: lohnt Stufe 2? | vorerst nur Stufe 1, Stufe 2 nicht geplant (2026-10-23) | C5 |
| Z10 | Anwohner als Tenant oder Contact Group, und wo stehen seine Benutzer? | weder noch: ein NetBox-Owner mit seinen NetBox-Benutzern in `users` (Login über Pocket-ID), in der Pflicht-Owner-Group „Anwohner“; Mitglieder werden in NetBox gepflegt, damit dieselbe Zuordnung die Leserechte in NetBox steuert (in der Lab-NetBox geprüft, 2026-10-23) | C7 |
| Z11 | Ports eines Anwohners an einem fremden Switch? | NetBox-Feld `owner` am Interface, kein Custom Field (2026-10-23) | C7 |
| Z12 | Apply eines Anwohners sofort oder mit Admin-Freigabe? | sofort (2026-10-23) | C7 |
| Z13 | Alarme im Provisioner oder über Prometheus? | nur Prometheus/Alertmanager (2026-10-23) | C8 |
| Z14 | Ziel des Uplink-Pings? | das Gateway des Uplinks (2026-10-23) | C8 |
| Z15 | Wo liegen die Positionen der Netz-Karte? | Admins publizieren das offizielle Layout in ein NetBox Custom Field; andere nehmen es oder speichern Änderungen im Browser (2026-10-23) | C9 |
| Z16 | Grenzwerte für Health? | keine: Health nicht beurteilen, nur anzeigen (Hover/Popup); Farben nur nach Erreichbarkeit (2026-10-23) | C8, C9 |
| Z17 | Power Ports in NetBox modellieren für `psuN-state`? | entfällt mit Z16 (2026-10-23) | C9 |
| Z18 | Pilot eines Netz-Updates? | Vorschlag des Provisioners, im Plan änderbar (2026-10-23) | C10 |
| Z19 | Wartungsfenster? | vorerst nicht (2026-10-23) | C10 |
| Z20 | Netz-Update pro Site oder über mehrere? | mehrere Sites, frei gewählt (2026-10-23) | C10 |
| Z35 | Kunden-Netz mit DHCP-Range (E6): Welche Grenzen soll die Range bekommen? Der ganze freie Rest wäre dasselbe wie heute ohne Range. Vorschlag: im Formular ein Häkchen „DHCP-Range anlegen“ mit Start und Ende, vorgegeben die obere Hälfte des Netzes (bei einem `/24` `.128`–`.254`), die untere bleibt für feste Adressen. Oder lieber feste Anzahl Adressen am Anfang frei (z.B. die ersten 20)? | Häkchen mit Start und Ende, vorgegeben die obere Hälfte des Netzes, änderbar; eine feste Anzahl liesse in kleinen Netzen kaum Range übrig (2026-11-06) | E6 |
| Z36 | Watchdog-Befunde (E1): Geräte ohne `ospf` (APs, Switches, CAPs) haben nie einen Parent; eine Warnung „kein Weg zu einer Wurzel“ für jedes wäre Rauschen. Warnen nur bei Geräten mit `ospf`-Interface, aber ohne Weg zu einer Wurzel (und ohne `propagate_default_route`)? Und das Custom Field für Sonderfälle: am Gerät `watchdog_target` (Text: leer automatisch, `none` aus, sonst eine feste IPv4), oder lieber eine Auswahl plus ein IP-Feld? Ein neues Custom Field muss auch in die produktive NetBox importiert werden | Warnen nur bei Geräten mit `ospf`-Interface ohne Weg zu einer Wurzel und ohne `propagate_default_route`; ein Textfeld `watchdog_target` am Gerät (leer automatisch, `none` aus, sonst eine IPv4) mit Validierungs-Regex in NetBox, keine widersprüchlichen Kombinationen (2026-11-06) | E1 |

## A. Sicherheit

| # | Aufgabe | Aufwand | Status |
|---|---|---|---|
| A8 | Schlüssel des OIDC-Issuers neu laden: heute holt `binary` die JWKS einmal beim Start (`Oidc::new`). Nach einem Schlüsselwechsel beim Provider lehnt der Provisioner jedes Login bis zum Neustart ab, ein zurückgezogener Schlüssel gilt bis dahin weiter. Periodisch und bei unbekannter `kid` neu holen; `actix-4-jwt-auth` hält die Schlüssel unveränderlich (`Arc<JWKSet>`), also das `Oidc` austauschen (z.B. hinter einem `ArcSwap` als App-Data) oder die Prüfung selbst mit biscuit machen | S–M | offen |
| A7 | WebFig über HTTPS: `www-ssl` statt `www`, mit einem Zertifikat pro Gerät, das der Provisioner anlegt (selbst signiert mit Browser-Warnung, oder von einer eigenen CA, deren Zertifikat die Admins einmal importieren); danach `www` aus. Heute geht WebFig im Klartext, nur aus `api_networks` | M | offen |

## B. Bereinigungen

| # | Aufgabe | Aufwand | Status |
|---|---|---|---|
| B9 | Ersatz für `speed: k` an `/interface/ethernet` suchen (nur mangels Alternative akzeptiert) | M | offen |

## C. Konzepte zum Umsetzen

| # | Aufgabe | Konzept | Aufwand | Status |
|---|---|---|---|---|
| C3 | Interne Firewall: `guest` und `protected` am Gateway-Interface, Freigaben als Custom Fields (Z4), nur ein Router pro Netz | [Firewall](concepts/firewall.md#interne-firewall) | M | offen |
| C5 | Befehle ohne Strings: Stufe 1, getypte Befehle aus dem Modell (Stufe 2 nicht geplant, Z7) | [Befehle ohne Strings](concepts/command-dsl.md) | M–L | offen |
| C7 | Self-Service und Personas: Rollen aus OIDC, NetBox-Owner als Anwohner, Leserechte in NetBox nur auf eigene WLANs (Z10, Z11), Leases, SSID/PSK, Ports, eigene APs, Apply sofort (Z12) | [Self-Service](concepts/self-service.md) | L | offen |
| C8 | Überwachung: Collector, Prometheus-Metriken (Alarme dort, Z13), Ping zum Uplink-Gateway (Z14), Zustandsseite, Ansicht für Anwohner | [Überwachung](concepts/monitoring.md) | L | offen |
| C9 | Netz-Karte: Graph aus der OSPF-LSDB, SVG, Farben nach Erreichbarkeit, Health nur angezeigt (Z16), PoE, Layout publizieren oder im Browser (Z15) | [Netz-Karte](concepts/network-map.md) | L | Schritt 1–3 und 5 erledigt (Drag, `localStorage`, `publishLayout`, gegen das Lab geprüft), Schritt 4 ohne Budget (PSE→PD-Pfeile mit Soll-gegen-Ist aus `helper::poe::feeding`, Budget pro Board offen: `hw_facts.rs` fehlen die Watt-Werte je Board) |
| C10 | Updates im ganzen Netz: mehrere Sites (Z20), Pilot vorgeschlagen (Z18), ohne Wartungsfenster (Z19) | [Updates im ganzen Netz](concepts/network-update.md) | L | Regel beim Neustart und Plan erledigt; offen: der Rollout-Job (Wellen, Pause nach den Piloten, Überspringen), Download-Cache, Fortschritt auf der Seite |
| C14 | Metriken des mikrotik-metrik-exporter im Provisioner (erster Teil von C8): Funk-Clients mit DHCP-Hostname, Health, PoE, dazu System, Interfaces und DHCP-Leases; Labels `device` und `site` aus NetBox, Dashboards im Repo angepasst; danach weder Exporter noch `prometheus`-Benutzer nötig | [Überwachung](concepts/monitoring.md) | M–L | offen |

## D. In Arbeit, Rest offen

| # | Aufgabe | Konzept | Aufwand | Status |
|---|---|---|---|---|

## E. Erweiterungen umgesetzter Teile

| # | Aufgabe | Doku | Aufwand | Status |
|---|---|---|---|---|
| E1 | Watchdog-Befunde: kein Weg zu einer Wurzel (kein Watchdog möglich) als Warnung; ein Custom Field für Sonderfälle (aus, oder eine feste Adresse) | [Watchdog](watchdog.md) | S–M | offen |
| E3 | Das Image in einem Cluster ausrollen und die beiden Workflows (CI, Aufräumen) auf GitHub einmal laufen lassen | [Deployment](deployment.md#noch-nicht-geprüft) | S | offen |
| E6 | Kunden-Netz: optional eine IP-Range mit Rolle `dhcp` anlegen (heute ist der DHCP-Pool der freie Rest des Netzes) | [Config-Helper](config-helper.md#kunden-netz) | S | offen |
| E8 | Weitere Befunde (Rest; erledigt: WLAN-Gruppe ohne Controller, Land): die Fehler des Generators gesammelt pro Site; gegen das Gerät ein OSPF-Nachbar `Full` über einen Port ohne `ospf`, das gemeldete Board gegen den Device Type und PoE als Befund für alle Geräte einer Site | [Config-Helper](config-helper.md#befunde) | M | offen |
| E9 | Automatischer Test der Helfer gegen das Lab-NetBox: ausführen, Vorschau mit dem Ergebnis vergleichen, zurücknehmen | [Config-Helper](config-helper.md#technik) | M | offen |
| E10 | Rollout im Feld: ein kurzer Test „Strecke ok“ (Ping, Bandbreite zum Nachbarn) und eine Ansicht der Strecke | [Geräteseite](device-page.md) | M | offen |
| E12 | Ansicht „Was fehlt diesem Gerät noch“: die Befunde des Geräts mit den Helfern als Knopf, der Reihe nach durch Loopback, Verbindung usw., ohne Wizard zu sein | [Geräteseite](device-page.md) | M | offen |
| E13 | Diff in Worten neben dem Skript („entfernt 2 Adressen, ändert die SSID“), die heiklen Teile hervorgehoben (Adressen, OSPF, Bridge-Ports) | [Geräteseite](device-page.md) | M | offen |
| E15 | Port angeschlossener Geräte ohne RouterOS erkennen: Für ein Gerät mit dokumentierter IPv4 oder IPv6 über ARP, IPv6-Nachbarn und die Host-Tabelle der Bridge finden, an welchem Port des Routers oder Switches es hängt, und in der Live-Prüfung der Interfaces ein fehlendes oder anders gelegtes Kabel melden, mit einem Helfer, der es in NetBox anlegt (Plan, Vorschau, Rückgängig). Zuordnung über die IP, nie über MACs aus NetBox; mehrere MACs an einem Port (Switch ohne RouterOS dahinter) machen ihn mehrdeutig | [Config-Helper](config-helper.md#netbox-gegen-gerät) | M–L | offen |
| E17 | VLANs auf einem CRS1xx/2xx in Hardware: dort läuft eine VLAN-Filtering-Bridge auf der CPU; die VLANs gehören ins Switch-Menü (`/interface/ethernet/switch/vlan`, `…/egress-vlan-tag`, `…/ingress-vlan-translation`, Ports als `vlan-type`), die Bridge bleibt ohne VLAN-Filtering. Für weitere CRS1xx/2xx, die heute ausser Betrieb sind; dsk-switch1 (CRS109, `QCA-8513L`) | [L2-Generierung](l2-generation.md#nach-dem-switch-chip-l2setupfor_switch_chips) | M | offen |
