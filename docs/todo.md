# Aufgaben

Alle offenen Aufgaben über die drei Repositories (netbox-provisioner, mikrotik-model,
mikrotik-api), gesammelt aus [pending.md](pending.md), den [Konzepten](concepts/) und der
Durchsicht des Codes vom 2026-10-19. Die Nummern bleiben, auch wenn umsortiert wird; eine
erledigte Aufgabe wandert mit Datum und Commit nach [done.md](done.md), ein erledigter Teil
ebenso, der Rest bleibt hier.

Aufwand grob: **S** Stunden, **M** ein bis zwei Tage, **L** mehr.
Status: **offen**, **in Arbeit**.

## Reihenfolge

Zurzeit keine; dann schauen wir weiter

## Entscheidungen

Fragen aus den Konzepten und der Durchsicht, mit der Antwort und den Aufgaben, die davon
abhängen. Eine neue Frage kommt mit Status **offen** dazu; entschieden, wird sie mit Datum
markiert und ins Konzept übernommen (dort unter „Entschieden“); sind alle Aufgaben einer
Entscheidung erledigt, wandert sie nach [done.md](done.md#entscheidungen).

| # | Frage | Entscheidung | betrifft |
|---|---|---|---|
| Z2 | Neues Tag `internet` oder genügt `nat`? | neues Tag `internet`; `nat` setzt es voraus (2026-10-23) | C1 |
| Z3 | Woher kommt der äussere Port eines Port-Forwardings? | aussen immer derselbe Port wie innen, kein Umsetzen (2026-10-23) | C2 |
| Z4 | Freigaben der internen Firewall: Custom Fields am Interface oder Prefix-Tags? | Custom Fields am Interface (2026-10-23) | C3 |
| Z5 | `netbox-model` entfernen oder für cynic verwenden? | entfernen (2026-10-23) | B3, C4 |
| Z6 | cynic oder Abfragen in einzelne Dateien? | cynic (2026-10-23) | C4, B10 |
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
| Z28 | Die andere Partition nach jedem Anwenden nachziehen (E14) überschreibt das RouterOS von vor einem Update, den Weg zurück über „Von partN starten“. Immer kopieren, nur bei gleicher Version, oder erst eine Weile nach einem Update? | immer kopieren: Eine Partition mit alter Konfiguration ist eine Falle, die später zu einem Ausfall führen kann. Bei gleicher Version nur die Konfiguration, sonst alles (2026-10-25) | E14 |

## A. Sicherheit

Heute offen: Ein Router mit Internet-Zugang ist nach einem Apply von aussen offen, bis auf DNS
(siehe [Firewall](concepts/firewall.md#ausgangslage)).

| # | Aufgabe | Aufwand | Status |
|---|---|---|---|
| A3 | SOCKS des Management-Proxys nur von der Adresse des Provisioners annehmen (heute auf allen Interfaces, nur mit Passwort) | S | offen |
| A6 | h2 0.3 loswerden (RUSTSEC-2026-0258, DoS über leere DATA-Frames, nur über HTTP/2): `actix-4-jwt-auth` bindet actix-web mit den Default-Features, damit `http2`; upstream ohne Default-Features oder Fork, dann `async-graphql-actix-web` ohne `http2` | S | offen |
| A7 | WebFig über HTTPS: `www-ssl` statt `www`, mit einem Zertifikat pro Gerät, das der Provisioner anlegt (selbst signiert mit Browser-Warnung, oder von einer eigenen CA, deren Zertifikat die Admins einmal importieren); danach `www` aus. Heute geht WebFig im Klartext, nur aus `api_networks` | M | offen |

## B. Bereinigungen

| # | Aufgabe | Aufwand | Status |
|---|---|---|---|
| B9 | Ersatz für `speed: k` an `/interface/ethernet` suchen (nur mangels Alternative akzeptiert) | M | offen |

## C. Konzepte zum Umsetzen

| # | Aufgabe | Konzept | Aufwand | Status |
|---|---|---|---|---|
| C1 | Perimeter-Firewall: Tag `internet` (Z2), Grundregeln IPv4/IPv6, SSH mit Bruteforce-Schutz, Zugang des Provisioners, IPv4-NAT | [Firewall](concepts/firewall.md#perimeter-firewall) | M | offen |
| C2 | Port-Forwarding IPv4 (ohne Umsetzen der Ports, Z3) und Freigaben IPv6 aus NetBox-Services (Tag `forward`) | [Firewall](concepts/firewall.md#perimeter-firewall) | M | offen |
| C3 | Interne Firewall: `guest` und `protected` am Gateway-Interface, Freigaben als Custom Fields (Z4), nur ein Router pro Netz | [Firewall](concepts/firewall.md#interne-firewall) | M | offen |
| C4 | cynic statt graphql_client, Abfrage für Abfrage (Spike erfolgreich) | [GraphQL-Dateien](concepts/graphql-files.md) | M–L | offen |
| C5 | Befehle ohne Strings: Stufe 1, getypte Befehle aus dem Modell (Stufe 2 nicht geplant, Z7) | [Befehle ohne Strings](concepts/command-dsl.md) | M–L | offen |
| C7 | Self-Service und Personas: Rollen aus OIDC, NetBox-Owner als Anwohner, Leserechte in NetBox nur auf eigene WLANs (Z10, Z11), Leases, SSID/PSK, Ports, eigene APs, Apply sofort (Z12) | [Self-Service](concepts/self-service.md) | L | offen |
| C8 | Überwachung: Collector, Prometheus-Metriken (Alarme dort, Z13), Ping zum Uplink-Gateway (Z14), Zustandsseite, Ansicht für Anwohner | [Überwachung](concepts/monitoring.md) | L | offen |
| C9 | Netz-Karte: Graph aus der OSPF-LSDB, SVG, Farben nach Erreichbarkeit, Health nur angezeigt (Z16), PoE, Layout publizieren oder im Browser (Z15) | [Netz-Karte](concepts/network-map.md) | L | offen |
| C10 | Updates im ganzen Netz: mehrere Sites (Z20), Pilot vorgeschlagen (Z18), ohne Wartungsfenster (Z19) | [Updates im ganzen Netz](concepts/network-update.md) | L | offen |
| C14 | Metriken des mikrotik-metrik-exporter im Provisioner (erster Teil von C8): Funk-Clients mit DHCP-Hostname, Health, PoE, dazu System, Interfaces und DHCP-Leases; Labels `device` und `site` aus NetBox, Dashboards im Repo angepasst; danach weder Exporter noch `prometheus`-Benutzer nötig | [Überwachung](concepts/monitoring.md) | M–L | offen |
| C16 | Funkbrücke ohne RouterOS (zwei UISP Wave Nano, von Hand eingerichtet), Management von beiden Seiten erreichbar. Im Lab aufgebaut mit DHCP: je Seite ein eigenes /29, Transfernetz /31 für OSPF; Lease-Zeit aus NetBox und der Helfer „Funkbrücke“ dafür sind gebaut. Offen: die Ausfall-Tests (Strecke gestört, ein MikroTik aus, Neustart einer Wave Nano) | [Funkbrücke ohne RouterOS](concepts/foreign-radio-bridge.md) | M | offen |

## D. In Arbeit, Rest offen

Zurzeit keine.

## E. Erweiterungen umgesetzter Teile

| # | Aufgabe | Doku | Aufwand | Status |
|---|---|---|---|---|
| E1 | Watchdog-Befunde: kein Weg zu einer Wurzel (kein Watchdog möglich) als Warnung; ein Custom Field für Sonderfälle (aus, oder eine feste Adresse) | [Watchdog](watchdog.md) | S–M | offen |
| E2 | Die Ersteinrichtung setzt den PoE-Ping des Speisers aus wie ein Update, damit ein ersetztes gespeistes Gerät im Werkszustand nicht alle 5 min stromlos wird | [Watchdog](watchdog.md#grenzen) | S | offen |
| E3 | Das Image in einem Cluster ausrollen und die beiden Workflows (CI, Aufräumen) auf GitHub einmal laufen lassen | [Deployment](deployment.md#noch-nicht-geprüft) | S | offen |
| E4 | rustls statt OpenSSL, dann musl und ein Image nur aus Binary und Zertifikaten | [Deployment](deployment.md#noch-nicht-geprüft) | M | offen |
| E5 | Job-Ansicht bei abgelaufenem Token (`401`/`403`): einmal neu anmelden lassen und weiterfragen, statt bei jedem Versuch denselben Fehler zu zeigen | [Jobs](jobs.md#verfolgen-im-browser) | S | offen |
| E6 | Kunden-Netz: optional eine IP-Range mit Rolle `dhcp` anlegen (heute ist der DHCP-Pool der freie Rest des Netzes) | [Config-Helper](config-helper.md#kunden-netz) | S | offen |
| E7 | OSPF-Priorität 0 auf Brückengeräten (nie DR/BDR): Regel im Generator (`priority` kennt mikrotik-model), vorher auf CHR vier OSPF-Sprecher in einem Segment, Strecke getrennt und wieder verbunden | [Config-Helper](config-helper.md#brückengeräte-mit-fester-adresse) | M | offen |
| E8 | Weitere Befunde: WLAN-Gruppe ohne Controller, Region ohne Land, die Fehler des Generators gesammelt pro Site; gegen das Gerät ein OSPF-Nachbar `Full` über einen Port ohne `ospf`, das gemeldete Board gegen den Device Type und PoE als Befund für alle Geräte einer Site | [Config-Helper](config-helper.md#befunde) | M | offen |
| E9 | Automatischer Test der Helfer gegen das Lab-NetBox: ausführen, Vorschau mit dem Ergebnis vergleichen, zurücknehmen | [Config-Helper](config-helper.md#technik) | M | offen |
| E10 | Rollout im Feld: ein kurzer Test „Strecke ok“ (Ping, Bandbreite zum Nachbarn) und eine Ansicht der Strecke | [Geräteseite](device-page.md) | M | offen |
| E11 | Helfer-Formulare und Vorschau auf dem Telefon prüfen (lange Skripte, Selects) | [Geräteseite](device-page.md) | S | offen |
| E12 | Ansicht „Was fehlt diesem Gerät noch“: die Befunde des Geräts mit den Helfern als Knopf, der Reihe nach durch Loopback, Verbindung usw., ohne Wizard zu sein | [Geräteseite](device-page.md) | M | offen |
| E13 | Diff in Worten neben dem Skript („entfernt 2 Adressen, ändert die SSID“), die heiklen Teile hervorgehoben (Adressen, OSPF, Bridge-Ports) | [Geräteseite](device-page.md) | M | offen |
| E14 | Zwei Partitionen: nach jedem gelungenen Anwenden (Fallback weg, Diff geprüft; im Batch erst, wenn alle durch sind) die andere Partition nachziehen, auch wenn sie das RouterOS von vor einem Update hält (Z28). Bei instabilem Strom bootet RouterOS manchmal von selbst die andere Partition, weil es fälschlich ein Boot-Problem erkennt, und liefe dann mit alter Konfiguration. Gleiche Version: nur die Konfiguration (`/partitions save-config-to`, im Lab zu prüfen, CHR hat keine Partitionen); andere Version: alles (`copy-to`) | [Update-Fallback](update-fallback.md#zwei-partitionen) | M | offen |
| E15 | Port angeschlossener Geräte ohne RouterOS erkennen: Für ein Gerät mit dokumentierter IPv4 oder IPv6 über ARP, IPv6-Nachbarn und die Host-Tabelle der Bridge finden, an welchem Port des Routers oder Switches es hängt, und in der Live-Prüfung der Interfaces ein fehlendes oder anders gelegtes Kabel melden, mit einem Helfer, der es in NetBox anlegt (Plan, Vorschau, Rückgängig). Zuordnung über die IP, nie über MACs aus NetBox; mehrere MACs an einem Port (Switch ohne RouterOS dahinter) machen ihn mehrdeutig | [Config-Helper](config-helper.md#netbox-gegen-gerät) | M–L | offen |
| E16 | Protokoll: ein ausgelöster Apply-Fallback (Beobachtung ohne Job) nennt die Marken des Skripts, die noch auf dem Gerät liegen (`device::ros::progress`) | [Protokoll](protocol.md#beobachtungen-ohne-job) | S | offen |
| E17 | VLANs auf einem CRS1xx/2xx in Hardware: dort läuft eine VLAN-Filtering-Bridge auf der CPU; die VLANs gehören ins Switch-Menü (`/interface/ethernet/switch/vlan`, `…/egress-vlan-tag`, `…/ingress-vlan-translation`, Ports als `vlan-type`), die Bridge bleibt ohne VLAN-Filtering. Für weitere CRS1xx/2xx, die heute ausser Betrieb sind; dsk-switch1 (CRS109, `QCA-8513L`) | [L2-Generierung](l2-generation.md#nach-dem-switch-chip-l2setupfor_switch_chips) | M | offen |
