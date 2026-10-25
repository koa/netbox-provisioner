# Erledigt

Die erledigten Aufgaben aus [todo.md](todo.md), mit Datum und Commit; die Nummern bleiben
dieselben. Eine Aufgabe, die nur zum Teil erledigt ist, steht in beiden Dateien: hier der
erledigte Teil, dort der Rest.

## Entscheidungen

Entscheidungen, deren Aufgaben alle erledigt sind (die übrigen stehen in
[todo.md](todo.md#entscheidungen)).

| # | Frage | Entscheidung | betrifft |
|---|---|---|---|
| Z8 | Nix-Image oder Containerfile? | Nix (2026-10-23) | C6 |
| Z9 | Welcher Cluster? | nur Image und Helm-Chart; die Cluster-Konfiguration macht der Betreiber von Hand (2026-10-23) | C6 |
| Z22 | Kaltstart PoE-gespeister Geräte selbst auslösen? | ja, auf Knopfdruck mit Bestätigung, nie von selbst (2026-10-23) | C11 |
| Z23 | Updates ganz ohne Fallback? | nur mit ausdrücklicher Bestätigung (2026-10-23) | D3 |
| Z24 | Welche OSPF-Nachbarn müssen nach einem Update `Full` sein? | die vorher benachbart waren (`Full`, oder `TwoWay` zwischen zwei DROther, nach berghof-stall-haus) (2026-10-23) | D3 |

## B. Bereinigungen

| # | Aufgabe | Aufwand | Erledigt |
|---|---|---|---|
| B1 | `binary/build.rs` startet `trunk` mit `spawn()` und wartet nicht (kann ein halbfertiges Frontend einbetten); die Hooks in `frontend/Trunk.toml` (`clear`, `npm install`) gehören aus dem Build | S | 2026-10-23 („build the frontend before the binary“): `build.rs` prüft nur `dist` und baut neu, wenn es sich ändert; `npm install` in `build.sh` und `run-provisioner.sh` |
| B2 | `config::Environment::with_prefix("app")` ohne Trenner: einzelne Werte (`settings.netbox_token`) lassen sich nicht per Umgebungsvariable setzen; Trenner `__` | S | 2026-10-23 („set single settings from the environment“): `APP_SETTINGS__NETBOX_TOKEN` |
| B3 | Crate `netbox-model` (cynic, eigenes Schema, eingecheckte `.idea/`) wird von niemandem benutzt: entfernen (Z5) | S | 2026-10-23 („remove the unused netbox-model crate“) |
| B4 | `DeviceCfg::generate_from` und `BaseDeviceDataTarget::generate_from` rufen dieselben `setup_*`-Schritte je selbst auf (PoE fehlte so einmal im echten Weg): eine gemeinsame Funktion | S | 2026-10-23 („share the setup steps of both generate_from“): `setup_addressing` vor, `setup_ports` nach den Radios |
| B5 | `lazy_static` (3 Dateien, dazu der Generator von mikrotik-model) durch `std::sync::LazyLock` ersetzen | S | 2026-10-23 („replace lazy_static with LazyLock“, in beiden Repos) |
| B6 | Ein vergifteter Mutex in `job.rs` (`.expect("jobs")`) lässt den Server abstürzen statt nur den Job | S | 2026-10-23 („keep the jobs going after a panic holding their locks“), ebenso die Leases in `device/power.rs` |
| B7 | Grosse Dateien aufteilen: `device/ros/mod.rs` (2067 Zeilen), `frontend/src/i18n.rs` (1609), `helper/mod.rs` (1391) | M | 2026-10-23: „split the radios and the DHCP pool off the generator“ (1247 Zeilen, Radios in `radio/`, Pool in `dhcp.rs`), „give each part of the UI texts a file“ (`i18n/`, eine Datei pro Modul), „split the plan steps and the writing off the helpers“ (541 Zeilen, `plan.rs`, `write.rs`); 2026-10-24: „split the netbox helper into a module per helper“ (`components/netbox_helper/`, `mod.rs` 660 Zeilen) |
| B8 | mikrotik-model: Der Generator gibt die Enums jedes Mal in anderer Reihenfolge aus (HashMap); Diffs des erzeugten Codes unlesbar, vermutlich unnötiges Neukompilieren | S | 2026-10-23 (mikrotik-model „generate the same code on every build“) |
| B10 | `graphql_client` 0.16 von crates.io statt des Forks (`Chrisss93/graphql-client`, Branch `reqwest-upgrade`) prüfen; nur als Übergang, bis C4 den Fork ablöst (Z6) | S | 2026-10-23 („take graphql_client from crates.io“): ohne Features, `json` direkt an reqwest; Topologie aus der Lab-NetBox und Site-Seite im Browser geprüft |

## C. Konzepte

| # | Aufgabe | Konzept | Aufwand | Erledigt |
|---|---|---|---|---|
| C6 | Deployment: Nix-Image (Z8), Flake, Git-Abhängigkeiten für mikrotik-model/-api, GitHub Actions, Helm-Chart; Cluster und Argo CD von Hand (Z9) | [Deployment](concepts/deployment.md) | L | 2026-10-23 (`build the image with nix and add the helm chart and ci`), 2026-10-24 (`publish snapshot charts and clean up old snapshots`) |
| C11 | Kaltstart eines PoE-gespeisten Geräts auf Knopfdruck: PoE am speisenden Port aus und wieder ein, mit Bestätigung (Z22); für Device-Mode und Rettung nach einem Update | [Update-Fallback](concepts/update-fallback.md) | S–M | 2026-10-24 („cold boot a device over its poe supply“) |
| C12 | Interfaces nach Aufgabe statt Helfer-Auswahl: Zuordnung, Ansicht, Lösungsvorschläge im Modal, Live-Prüfungen, neue Befunde und Helfer | [Interfaces nach Aufgabe](concepts/interface-view.md) | – | Schritt 1 2026-10-23 („show the interfaces of a device by task“), Schritt 2 2026-10-23 („open the helpers from the interface rows“), Schritt 3 2026-10-23 („check the interface rows live“), Schritt 4 2026-10-23 („find gateways, tagged ports, radio paths and labels“); aus Schritt 5: Funkbrücke vom Router-Port 2026-10-23 („open the radio bridge helper from the router port“), Richtfunk abräumen 2026-10-23 („take a radio link's network out of netbox“), Kunden-Anschluss abräumen 2026-10-23 („take a customer port's network out of netbox“), VLAN am Port 2026-10-24 („put a customer's vlan on a port“), Kabel übernehmen 2026-10-24 („take over the cable a device sees“) |
| C13 | Syslog der Geräte: RouterOS-eigene Logging-Einträge im Ziel, mit `device_syslog` die Action `remote` in CEF zum Server und eine Regel pro Topic | – | S | 2026-10-24 (`send the devices' log to a syslog server`, mikrotik-model `model the logging menus`) |
| D6 | Helfer vom Port aus: jeder Port eine Zeile mit den Aktionen, die auf ihm möglich sind (freie Ports, Ports einer Bridge einzeln, PoE und Flags pro Port, Funkbrücke von einem freien Router-Port mit Wahl des Brückengeräts, Aus Bridge lösen für den Port eines Kunden-Anschlusses), ohne „Weitere Helfer“ | [Interfaces nach Aufgabe](concepts/interface-view.md#aktionen-pro-port) | – | Aktionen pro Port 2026-10-24 („offer each port's helpers on its own row“), Dialoge ohne Wahl des Start-Ports 2026-10-24 („leave the start port out of the helper forms“) |
| D1 | Helfer „Neues Gerät“: vom freien Port eines Routers (ohne Kabel oder offen an einem Patchpanel; Vorschlag, wenn die Live-Prüfung dort ein unbekanntes MikroTik sieht), Device Type aus NetBox nach Board vorgewählt, Rolle, WLAN-Gruppe eines Access Points, Tenant des Routers; ein Plan mit Gerät, Port, Loopback und Verbindung, danach zur Ersteinrichtung | [Config-Helper](concepts/config-helper.md#neues-gerät) | L | 2026-10-24 („add a device from a router's free port“) |
| D2 | Datenkorrektur: Übersicht pro Site, wie weit jedes Gerät von NetBox abweicht, als Job der Site mit Zwischenstand (Z21): Änderungen je Menü, entfernte Adressen, OSPF und Bridge-Ports hervorgehoben, nicht lesbare Geräte mit Grund; Adressen, die nur auf dem Gerät stehen, in der Live-Prüfung mit „übernehmen“ | [User Journeys](concepts/user-journeys.md#datenkorrektur) | M–L | Übersicht 2026-10-24 („show how far a site's devices are from netbox“), Adressen übernehmen 2026-10-24 („take over addresses only the device has“), statische Leases verwalten und übernehmen 2026-10-24 („manage static dhcp leases from netbox“, mikrotik-model „model the static dhcp leases“) |
| D3 | Update-Fallback Stufe 2: Bestätigung für ein Update ganz ohne Weg zurück (Z23), das selbständige Zurückschalten im Lab getestet, Downgrade auf Knopfdruck, Rettung über mac-telnet mit dem Log des Geräts | [Update-Fallback](concepts/update-fallback.md) | M | Bestätigung 2026-10-24 („confirm an update without a way back“), Zurückschalten im Lab geprüft 2026-10-25 (mit „cold boot a device over its poe supply“), Downgrade 2026-10-25 („downgrade routeros from the device page“), Rettung 2026-10-25 („rescue a device over mac-telnet“) |
| D4 | Geräteseite: Bereich „Liegengebliebenes“ unter dem Kopf (Fallback des Anwendens mit „Fallback stoppen“ und „Jetzt zurück“, Fallback des Updates, ausgesetzte Watchdogs, ein noch laufendes Apply-Skript, hochgeladene Pakete); die Befunde bleiben auf den Zeilen der Interface-Karte | [Geräteseite](concepts/device-page.md) | S–M | 2026-10-25 („show what a job left on the device“) |
| D5 | Jobs, Teil „Liegengebliebenes anzeigen“ (mit D4); der Rest bleibt in todo.md | [Jobs](concepts/job-polling.md) | S | 2026-10-25 („show what a job left on the device“) |
