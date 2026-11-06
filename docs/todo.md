# Aufgaben

Alle offenen Aufgaben über die drei Repositories (netbox-provisioner, mikrotik-model,
mikrotik-api), gesammelt aus [pending.md](pending.md), den [Konzepten](concepts/) und der
Durchsichten des Codes vom 2026-10-19 und 2026-11-06. Die Nummern bleiben, auch wenn umsortiert wird; eine
erledigte Aufgabe wandert mit Datum und Commit nach [done.md](done.md), ein erledigter Teil
ebenso, der Rest bleibt hier.

Aufwand grob: **S** Stunden, **M** ein bis zwei Tage, **L** mehr.
Status: **offen**, **in Arbeit**.

## Reihenfolge

Die Bereinigungen aus der Durchsicht vom 2026-11-06, je ein Commit, Tests und clippy grün,
das Verhalten unverändert (die Fehler B12 und B13 sind behoben):

1. **cynic:** C4, Abfrage für Abfrage. Danach die Codequalität nochmals beurteilen (wie die
   Durchsicht vom 2026-11-06: clippy mit strengeren Lints, Duplikate, Modulstruktur) und die
   folgenden Schritte anpassen, bevor es weitergeht: Mit gemeinsamen Typen im Frontend fallen
   Teile von B14, B15 und B21 kleiner aus oder weg.
2. **Gerät-Ziel als ein Input:** B14, dann B15. Verkleinert Backend und Frontend zugleich und
   ändert das Schema, deshalb vor dem Verschieben der Module.
3. **Module ordnen:** B16 (Wiederaufnehmen raus aus `graphql/mod.rs`), dann B17 (Apply raus
   aus `ros/graphql.rs`). Reines Verschieben, danach sind die Dateien überschaubar.
4. **Lange Funktionen teilen:** B18 (`helper/write.rs`), B19 (`ros/mod.rs`), B20
   (`build_topology_from`), B21 (Geräteseite), B22 (Layout der Netzkarte, vor E21). In dieser
   Folge, weil die ersten am meisten Fehlerrisiko tragen; jede für sich, keine hängt an einer
   anderen.
5. **Bei Gelegenheit:** B23 (`hw_facts`, Firewall v4/v6), wenn dort ohnehin etwas dazukommt.

## Entscheidungen

Fragen aus den Konzepten und der Durchsicht, mit der Antwort und den Aufgaben, die davon
abhängen. Eine neue Frage kommt mit Status **offen** dazu; entschieden, wird sie mit Datum
markiert und ins Konzept übernommen (dort unter „Entschieden“); sind alle Aufgaben einer
Entscheidung erledigt, wandert sie nach [done.md](done.md#entscheidungen).

| # | Frage | Entscheidung | betrifft |
|---|---|---|---|
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
| Z34 | Abgelaufene Anmeldung ohne Refresh-Token (Pocket-ID ohne `offline_access`): `yew-oauth2` hält das Token weiter für gültig, jede Seite bekommt `403`. Die Job-Ansicht bietet „Erneut anmelden“ an, wo eine HTTP-Abfrage abgelehnt wird (E5); die WebSocket-Verbindung der Live-Ansichten (seit E18 auch der Job-Ansicht) endet beim Ablauf des Tokens, baut sich mit dem aktuellen Token neu auf und versucht es nach einer Ablehnung still weiter. Sollen alle Seiten beim Ablauf von selbst neu anmelden (Umleitung zum Provider und zurück; bei laufender Sitzung dort ohne Eingabe, aber ein offenes Formular ginge verloren), nur einen Hinweis mit Knopf oben zeigen, oder Pocket-ID mit `offline_access` betreiben (dort noch nicht getestet)? | Hinweis mit Knopf auf allen Seiten, kein automatisches Umleiten (ein offenes Formular bliebe erhalten); `offline_access` bei Pocket-ID zusätzlich ausprobieren (2026-11-06) | E20 |
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
| B14 | Gerät-Ziel als ein GraphQL-Input: `target`, `credentialName`, `adhocCredentials` stehen einzeln in 8 Mutationen (`graphql/mod.rs`, `apply_cfg`, `reboot_device`, `change_device_mode`, `repartition_device`, `boot_partition`, `clear_leftover`, Updates); ein InputObject `DeviceTarget`. Im Frontend fallen damit die 6 fast gleichen Funktionen in `pages/devices/show.rs` (`reboot`, `change_mode`, `repartition`, `boot_from`, `clear`, `apply`, je mit eigenem `AdhocCredentials`) auf eine Umwandlung von `SelectedTarget` zusammen. Schema-Änderung: Backend und Frontend im selben Commit | M | offen |
| B15 | Die Körper dieser Mutationen sind gleich (Gerät holen, `PowerChain::of`, `job::start` mit `connect_for_job`, `log.enter`): ein Helfer `start_device_job(kind, state, target, \|link, log\| …)` | S | offen |
| B16 | Wiederaufnehmen gespeicherter Jobs nach einem Neustart aus `graphql/mod.rs` nach `saved_job` (oder `job/resume.rs`): `resume_saved_jobs`, `resume_batch`, `saved_update`, `connect_until`, `resume_apply`, `resume_update`, `Watchers`, `restore_watchdogs` (rund 250 Zeilen); `graphql/mod.rs` behält nur Schema und Resolver | S | offen |
| B17 | `device/ros/graphql.rs` (1912 Zeilen) aufteilen: darin stehen die Generierung (`DeviceCfg`, `render_script`), der ganze Apply-Ablauf (`impl AccessibleDevice`, `apply`, `apply_planned`, `provision_radios`, `reprovision`), seine Events (`ApplyPlanned` … `RadiosReprovisioned`), Log und RouterOS-Versionen; `job.rs` importiert es bezeichnenderweise als `ros::graphql as apply`. Vorschlag `ros/cfg.rs`, `ros/apply.rs` (Ablauf und Events), `graphql.rs` nur mit den Resolvern | M | offen |
| B18 | `helper/write.rs` `write` (381 Zeilen, kognitive Komplexität 54): der `match` über alle `PlanStep`s inline; je Schritt eine Funktion (oder Methode am Schritt). Die Update-Funktionen enden dreimal gleich mit `rest.update` und `NetboxWritten` (Zeilen 660, 794, 846): eine gemeinsame `write_update` | M | offen |
| B19 | `device/ros/mod.rs`: `impl BaseDeviceDataTarget` hat rund 1180 Zeilen. DHCP, Firewall, Radio, Speed, Syslog sind schon eigene Module; OSPF, Adressen, PoE, MTU sowie DNS/Services/Proxy nach demselben Muster auslagern | M | offen |
| B20 | `topology/fetch.rs` `build_topology_from` (657 Zeilen, rund 15 lokale Maps, kognitive Komplexität 59): in Durchläufe je Objektart teilen (Builder-Struktur mit den Maps als Feldern). Dabei `data.clone()` der ganzen NetBox-Antwort (Zeile 39) vermeiden, wenn `Source` die Antwort teilen kann | M | offen |
| B21 | Geräteseite `pages/devices/show.rs`: `view` 454 Zeilen, `update` 253; die Karten (RouterOS, Partitionen, Watchdogs, Änderungen) als eigene Funktionen oder Komponenten | M | offen |
| B22 | `components/network_map.rs` (1396 Zeilen): die Layout-Algorithmen sind reine Funktionen (Force-Layout, `clustered_layout`, `seed_from_graph`, `count_intrusions`, `reduce_site_crossings`) und gehören nach `network_map/layout.rs`, neben das vorhandene `test.rs` | S | offen |
| B23 | Wiederholungen bei Geräten und Firewall: die Port-Ketten pro Board in `hw_facts.rs` (`repeat_n…chain…enumerate`, z.B. CCR1009-7G/8G) als Tabelle oder Makro, dort auch 4 gleiche Match-Arme; die Builder für v4 und v6 in `ros/firewall.rs` (`v4`/`v6`) gleich bis auf die Typen | S–M | offen |

## C. Konzepte zum Umsetzen

| # | Aufgabe | Konzept | Aufwand | Status |
|---|---|---|---|---|
| C3 | Interne Firewall: `guest` und `protected` am Gateway-Interface, Freigaben als Custom Fields (Z4), nur ein Router pro Netz | [Firewall](concepts/firewall.md#interne-firewall) | M | offen |
| C4 | cynic statt graphql_client, Abfrage für Abfrage (Spike erfolgreich) | [GraphQL-Dateien](concepts/graphql-files.md) | M–L | in Arbeit |
| C5 | Befehle ohne Strings: Stufe 1, getypte Befehle aus dem Modell (Stufe 2 nicht geplant, Z7) | [Befehle ohne Strings](concepts/command-dsl.md) | M–L | offen |
| C7 | Self-Service und Personas: Rollen aus OIDC, NetBox-Owner als Anwohner, Leserechte in NetBox nur auf eigene WLANs (Z10, Z11), Leases, SSID/PSK, Ports, eigene APs, Apply sofort (Z12) | [Self-Service](concepts/self-service.md) | L | offen |
| C8 | Überwachung: Collector, Prometheus-Metriken (Alarme dort, Z13), Ping zum Uplink-Gateway (Z14), Zustandsseite, Ansicht für Anwohner | [Überwachung](concepts/monitoring.md) | L | offen |
| C9 | Netz-Karte: Graph aus der OSPF-LSDB, SVG, Farben nach Erreichbarkeit, Health nur angezeigt (Z16), PoE, Layout publizieren oder im Browser (Z15) | [Netz-Karte](concepts/network-map.md) | L | Schritt 1–3 und 5 erledigt (Drag, `localStorage`, `publishLayout`, gegen das Lab geprüft), Schritt 4 ohne Budget (PSE→PD-Pfeile mit Soll-gegen-Ist aus `helper::poe::feeding`, Budget pro Board offen: `hw_facts.rs` fehlen die Watt-Werte je Board) |
| C10 | Updates im ganzen Netz: mehrere Sites (Z20), Pilot vorgeschlagen (Z18), ohne Wartungsfenster (Z19) | [Updates im ganzen Netz](concepts/network-update.md) | L | offen |
| C14 | Metriken des mikrotik-metrik-exporter im Provisioner (erster Teil von C8): Funk-Clients mit DHCP-Hostname, Health, PoE, dazu System, Interfaces und DHCP-Leases; Labels `device` und `site` aus NetBox, Dashboards im Repo angepasst; danach weder Exporter noch `prometheus`-Benutzer nötig | [Überwachung](concepts/monitoring.md) | M–L | offen |

## D. In Arbeit, Rest offen

| # | Aufgabe | Konzept | Aufwand | Status |
|---|---|---|---|---|

## E. Erweiterungen umgesetzter Teile

| # | Aufgabe | Doku | Aufwand | Status |
|---|---|---|---|---|
| E1 | Watchdog-Befunde: kein Weg zu einer Wurzel (kein Watchdog möglich) als Warnung; ein Custom Field für Sonderfälle (aus, oder eine feste Adresse) | [Watchdog](watchdog.md) | S–M | offen |
| E3 | Das Image in einem Cluster ausrollen und die beiden Workflows (CI, Aufräumen) auf GitHub einmal laufen lassen | [Deployment](deployment.md#noch-nicht-geprüft) | S | offen |
| E4 | rustls statt OpenSSL, dann musl und ein Image nur aus Binary und Zertifikaten | [Deployment](deployment.md#noch-nicht-geprüft) | M | offen |
| E6 | Kunden-Netz: optional eine IP-Range mit Rolle `dhcp` anlegen (heute ist der DHCP-Pool der freie Rest des Netzes) | [Config-Helper](config-helper.md#kunden-netz) | S | offen |
| E8 | Weitere Befunde (Rest; erledigt: WLAN-Gruppe ohne Controller, Land): die Fehler des Generators gesammelt pro Site; gegen das Gerät ein OSPF-Nachbar `Full` über einen Port ohne `ospf`, das gemeldete Board gegen den Device Type und PoE als Befund für alle Geräte einer Site | [Config-Helper](config-helper.md#befunde) | M | offen |
| E9 | Automatischer Test der Helfer gegen das Lab-NetBox: ausführen, Vorschau mit dem Ergebnis vergleichen, zurücknehmen | [Config-Helper](config-helper.md#technik) | M | offen |
| E10 | Rollout im Feld: ein kurzer Test „Strecke ok“ (Ping, Bandbreite zum Nachbarn) und eine Ansicht der Strecke | [Geräteseite](device-page.md) | M | offen |
| E12 | Ansicht „Was fehlt diesem Gerät noch“: die Befunde des Geräts mit den Helfern als Knopf, der Reihe nach durch Loopback, Verbindung usw., ohne Wizard zu sein | [Geräteseite](device-page.md) | M | offen |
| E13 | Diff in Worten neben dem Skript („entfernt 2 Adressen, ändert die SSID“), die heiklen Teile hervorgehoben (Adressen, OSPF, Bridge-Ports) | [Geräteseite](device-page.md) | M | offen |
| E14 | Zwei Partitionen: nach jedem gelungenen Anwenden (Fallback weg, Diff geprüft; im Batch erst, wenn alle durch sind) die andere Partition nachziehen, auch wenn sie das RouterOS von vor einem Update hält (Z28). Bei instabilem Strom bootet RouterOS manchmal von selbst die andere Partition, weil es fälschlich ein Boot-Problem erkennt, und liefe dann mit alter Konfiguration. Gleiche Version: nur die Konfiguration (`/partitions save-config-to`, im Lab zu prüfen, CHR hat keine Partitionen); andere Version: alles (`copy-to`) | [Update-Fallback](update-fallback.md#zwei-partitionen) | M | offen |
| E15 | Port angeschlossener Geräte ohne RouterOS erkennen: Für ein Gerät mit dokumentierter IPv4 oder IPv6 über ARP, IPv6-Nachbarn und die Host-Tabelle der Bridge finden, an welchem Port des Routers oder Switches es hängt, und in der Live-Prüfung der Interfaces ein fehlendes oder anders gelegtes Kabel melden, mit einem Helfer, der es in NetBox anlegt (Plan, Vorschau, Rückgängig). Zuordnung über die IP, nie über MACs aus NetBox; mehrere MACs an einem Port (Switch ohne RouterOS dahinter) machen ihn mehrdeutig | [Config-Helper](config-helper.md#netbox-gegen-gerät) | M–L | offen |
| E17 | VLANs auf einem CRS1xx/2xx in Hardware: dort läuft eine VLAN-Filtering-Bridge auf der CPU; die VLANs gehören ins Switch-Menü (`/interface/ethernet/switch/vlan`, `…/egress-vlan-tag`, `…/ingress-vlan-translation`, Ports als `vlan-type`), die Bridge bleibt ohne VLAN-Filtering. Für weitere CRS1xx/2xx, die heute ausser Betrieb sind; dsk-switch1 (CRS109, `QCA-8513L`) | [L2-Generierung](l2-generation.md#nach-dem-switch-chip-l2setupfor_switch_chips) | M | offen |
| E20 | Abgelaufene Anmeldung auf allen Seiten (Z34): oben ein Hinweis „Anmeldung abgelaufen“ mit „Erneut anmelden“, sobald eine HTTP-Abfrage `401`/`403` bekommt oder der WebSocket die Anmeldung ablehnt; wie in der Job-Ansicht (E5, `pages::start_login`) nimmt die Seite danach wieder auf, ohne Umleitung von selbst. Zusätzlich Pocket-ID mit `offline_access` ausprobieren (dann erneuert das UI still) und das Ergebnis in `docs/login.md` | [Login](login.md) | S–M | offen |
| E21 | Netzkarte: Verbindungen, die eine Location verlassen, gehen kreuz und quer durch ihren Kreis (Berg, 2026-11-06: klr-sw-1 und klr-sw-3 nach draussen quer über die übrigen Geräte), während die Entzerrung innerhalb gut ist. Heute zieht nur ein fester Anker in Richtung der anderen Site an einem Gerät (`clustered_layout`, `anchor_edges`), und das Entkreuzen in der Site (`reduce_crossings`) zählt nur die internen Kabel. Ansätze, in dieser Folge zu versuchen: (1) die externen Verbindungen beim Entkreuzen in der Site mitzählen, als Strecke vom Gerät zum Rand in Richtung der Zielsite, auch gegen Geräte, durch die sie führen (wie `count_intrusions`); (2) Geräte mit externen Verbindungen an den Rand des Kreises legen, im Winkel ihrer Zielsites geordnet, die übrigen innen (dann kreuzen sich die abgehenden Linien in der Site nicht mehr untereinander); (3) eine externe Verbindung am Kreisrand austreten lassen und mehrere zwischen denselben zwei Sites bündeln. Messbar machen mit einer Kennzahl in `network_map/test.rs` (Kreuzungen und Durchquerungen einer realistischen Topologie vorher/nachher), dazu Screenshots aus dem Lab | [Netz-Ansicht](concepts/network-map.md#darstellung-frontend-yew) | M | offen |
