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

## B. Bereinigungen

| # | Aufgabe | Aufwand | Erledigt |
|---|---|---|---|
| B1 | `binary/build.rs` startet `trunk` mit `spawn()` und wartet nicht (kann ein halbfertiges Frontend einbetten); die Hooks in `frontend/Trunk.toml` (`clear`, `npm install`) gehören aus dem Build | S | 2026-10-23 („build the frontend before the binary“): `build.rs` prüft nur `dist` und baut neu, wenn es sich ändert; `npm install` in `build.sh` und `run-provisioner.sh` |
| B2 | `config::Environment::with_prefix("app")` ohne Trenner: einzelne Werte (`settings.netbox_token`) lassen sich nicht per Umgebungsvariable setzen; Trenner `__` | S | 2026-10-23 („set single settings from the environment“): `APP_SETTINGS__NETBOX_TOKEN` |
| B3 | Crate `netbox-model` (cynic, eigenes Schema, eingecheckte `.idea/`) wird von niemandem benutzt: entfernen (Z5) | S | 2026-10-23 („remove the unused netbox-model crate“) |
| B4 | `DeviceCfg::generate_from` und `BaseDeviceDataTarget::generate_from` rufen dieselben `setup_*`-Schritte je selbst auf (PoE fehlte so einmal im echten Weg): eine gemeinsame Funktion | S | 2026-10-23 („share the setup steps of both generate_from“): `setup_addressing` vor, `setup_ports` nach den Radios |
| B5 | `lazy_static` (3 Dateien, dazu der Generator von mikrotik-model) durch `std::sync::LazyLock` ersetzen | S | 2026-10-23 („replace lazy_static with LazyLock“, in beiden Repos) |
| B6 | Ein vergifteter Mutex in `job.rs` (`.expect("jobs")`) lässt den Server abstürzen statt nur den Job | S | 2026-10-23 („keep the jobs going after a panic holding their locks“), ebenso die Leases in `device/power.rs` |
| B7 | Grosse Dateien aufteilen: `device/ros/mod.rs` (2067 Zeilen), `frontend/src/i18n.rs` (1609), `helper/mod.rs` (1391) | M | 2026-10-23: „split the radios and the DHCP pool off the generator“ (1247 Zeilen, Radios in `radio/`, Pool in `dhcp.rs`), „give each part of the UI texts a file“ (`i18n/`, eine Datei pro Modul), „split the plan steps and the writing off the helpers“ (541 Zeilen, `plan.rs`, `write.rs`); `netbox_helper.rs` offen (todo.md) |
| B8 | mikrotik-model: Der Generator gibt die Enums jedes Mal in anderer Reihenfolge aus (HashMap); Diffs des erzeugten Codes unlesbar, vermutlich unnötiges Neukompilieren | S | 2026-10-23 (mikrotik-model „generate the same code on every build“) |
| B10 | `graphql_client` 0.16 von crates.io statt des Forks (`Chrisss93/graphql-client`, Branch `reqwest-upgrade`) prüfen; nur als Übergang, bis C4 den Fork ablöst (Z6) | S | 2026-10-23 („take graphql_client from crates.io“): ohne Features, `json` direkt an reqwest; Topologie aus der Lab-NetBox und Site-Seite im Browser geprüft |

## C. Konzepte

| # | Aufgabe | Konzept | Aufwand | Erledigt |
|---|---|---|---|---|
| C6 | Deployment: Nix-Image (Z8), Flake, Git-Abhängigkeiten für mikrotik-model/-api, GitHub Actions, Helm-Chart; Cluster und Argo CD von Hand (Z9) | [Deployment](concepts/deployment.md) | L | 2026-10-23 (`build the image with nix and add the helm chart and ci`), 2026-10-24 (`publish snapshot charts and clean up old snapshots`) |
| C12 | Interfaces nach Aufgabe statt Helfer-Auswahl: Zuordnung, Ansicht, Lösungsvorschläge im Modal, Live-Prüfungen, neue Befunde und Helfer | [Interfaces nach Aufgabe](concepts/interface-view.md) | – | Schritt 1 2026-10-23 („show the interfaces of a device by task“), Schritt 2 2026-10-23 („open the helpers from the interface rows“), Schritt 3 2026-10-23 („check the interface rows live“), Schritt 4 2026-10-23 („find gateways, tagged ports, radio paths and labels“); aus Schritt 5: Funkbrücke vom Router-Port 2026-10-23 („open the radio bridge helper from the router port“), Richtfunk abräumen 2026-10-23 („take a radio link's network out of netbox“), Kunden-Anschluss abräumen 2026-10-23 („take a customer port's network out of netbox“), VLAN am Port 2026-10-24 („put a customer's vlan on a port“), Kabel übernehmen 2026-10-24 („take over the cable a device sees“) |
| C13 | Syslog der Geräte: RouterOS-eigene Logging-Einträge im Ziel, mit `device_syslog` die Action `remote` in CEF zum Server und eine Regel pro Topic | – | S | 2026-10-24 (`send the devices' log to a syslog server`, mikrotik-model `model the logging menus`) |
