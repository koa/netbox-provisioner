# Aufgaben

Alle offenen Aufgaben über die drei Repositories (netbox-provisioner, mikrotik-model,
mikrotik-api), gesammelt aus [pending.md](pending.md), den [Konzepten](concepts/) und der
Durchsicht des Codes vom 2026-10-19. Die Nummern bleiben, auch wenn umsortiert wird; erledigte
Aufgaben werden mit Datum und Commit markiert, nicht gelöscht.

Aufwand grob: **S** Stunden, **M** ein bis zwei Tage, **L** mehr.
Status: **offen**, **in Arbeit**, **erledigt**.

## Reihenfolge

1. B1, B2, B3, B4, B5, B6, B7, B8, B10
2. C12
3. D1-D5

Dann schauen wir weiter

## Entscheidungen

Fragen aus den Konzepten und der Durchsicht, mit der Antwort und den Aufgaben, die davon
abhängen. Eine neue Frage kommt mit Status **offen** dazu; entschieden, wird sie mit Datum
markiert und ins Konzept übernommen (dort unter „Entschieden“). Offen ist zurzeit keine.

| # | Frage | Entscheidung | betrifft |
|---|---|---|---|
| Z1 | Woran erkennt der Provisioner seine eigenen Firewall-Regeln? | Kommentar-Präfix; fremde Regeln bleiben stehen (2026-10-23) | A1 |
| Z2 | Neues Tag `internet` oder genügt `nat`? | neues Tag `internet`; `nat` setzt es voraus (2026-10-23) | C1 |
| Z3 | Woher kommt der äussere Port eines Port-Forwardings? | aussen immer derselbe Port wie innen, kein Umsetzen (2026-10-23) | C2 |
| Z4 | Freigaben der internen Firewall: Custom Fields am Interface oder Prefix-Tags? | Custom Fields am Interface (2026-10-23) | C3 |
| Z5 | `netbox-model` entfernen oder für cynic verwenden? | entfernen (2026-10-23) | B3, C4 |
| Z6 | cynic oder Abfragen in einzelne Dateien? | cynic (2026-10-23) | C4, B10 |
| Z7 | Befehle ohne Strings: lohnt Stufe 2? | vorerst nur Stufe 1, Stufe 2 nicht geplant (2026-10-23) | C5 |
| Z8 | Nix-Image oder Containerfile? | Nix (2026-10-23) | C6 |
| Z9 | Welcher Cluster? | nur Image und Helm-Chart; die Cluster-Konfiguration macht der Betreiber von Hand (2026-10-23) | C6 |
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
| Z21 | Diff aller Geräte einer Site live oder als Job? | Job mit Zwischenstand (2026-10-23) | D2 |
| Z22 | Kaltstart PoE-gespeister Geräte selbst auslösen? | ja, auf Knopfdruck mit Bestätigung, nie von selbst (2026-10-23) | C11 |
| Z23 | Updates ganz ohne Fallback? | nur mit ausdrücklicher Bestätigung (2026-10-23) | D3 |
| Z24 | Welche OSPF-Nachbarn müssen nach einem Update `Full` sein? | die vorher benachbart waren (`Full`, oder `TwoWay` zwischen zwei DROther, nach berghof-stall-haus) (2026-10-23) | D3 |
| Z25 | Uplinks in der Interface-Ansicht? | eigene Aufgabe, nur angezeigt und geprüft; modelliert ganz in NetBox, keine Helfer (2026-10-23) | C12 |
| Z26 | Tagged VLANs an physischen Ports (Trunk zu AP/Switch)? | nicht vorgesehen: L2 zu anderen Geräten über VXLAN, eigene APs und Switches über eine Direktverbindung; zurück auf den Tisch, wenn sich echte Nachteile zeigen (2026-10-23) | C12 |
| Z27 | Ein L3-Übergang pro VLAN: Fehler oder Warnung, auch für Switches? | Fehler, auch für Switches: ein Switch mit dem VLAN nur auf L2 hat darin keine Adresse und ist nur über den Router des VLANs erreichbar (2026-10-23) | C12 |

## A. Sicherheit

Heute offen: Ein Router mit Internet-Zugang ist nach einem Apply von aussen offen, bis auf DNS
(siehe [Firewall](concepts/firewall.md#ausgangslage)).

| # | Aufgabe | Aufwand | Status |
|---|---|---|---|
| A1 | Firewall-Abgleich: nur eigene Regeln verwalten (Kommentar-Präfix, Z1), fremde stehen lassen, eigene vor den fremden; Reihenfolge der Regeln beachten. Heute paart `generate_add_update_remove_by_id` die Regeln, schreibt sie um und entfernt den Rest | M | offen |
| A2 | `/ip/service` ins Modell: telnet, ftp, www aus; API nur aus den eigenen Netzen bzw. api-ssl; für alle Geräte | S–M | offen |
| A3 | SOCKS des Management-Proxys nur von der Adresse des Provisioners annehmen (heute auf allen Interfaces, nur mit Passwort) | S | offen |
| A4 | Abhängigkeiten (`cargo audit`: 34 Hinweise, 12 Warnungen): `actix-4-jwt-auth` 1.2.5 → 1.2.9 (rustls 0.20, ring 0.16), `prometheus` 0.13 → 0.14 und Ersatz für `actix-web-prometheus` (protobuf 2.28), Subscriptions-Feature von `async-graphql-actix-web` weglassen (`actix-web-actors`, crossbeam-channel), `cargo update` (h2, openssl, quinn-proto, rsa, bincode) | S–M | offen |
| A5 | Prüfen, ob schon ein Router mit eigener Firewall angewendet wurde; falls ja, seine Firewall kontrollieren (Betreiber) | – | offen |

## B. Bereinigungen

| # | Aufgabe | Aufwand | Status |
|---|---|---|---|
| B1 | `binary/build.rs` startet `trunk` mit `spawn()` und wartet nicht (kann ein halbfertiges Frontend einbetten); die Hooks in `frontend/Trunk.toml` (`clear`, `npm install`) gehören aus dem Build | S | erledigt 2026-10-23 („build the frontend before the binary“): `build.rs` prüft nur `dist` und baut neu, wenn es sich ändert; `npm install` in `build.sh` und `run-provisioner.sh` |
| B2 | `config::Environment::with_prefix("app")` ohne Trenner: einzelne Werte (`settings.netbox_token`) lassen sich nicht per Umgebungsvariable setzen; Trenner `__` | S | erledigt 2026-10-23 („set single settings from the environment“): `APP_SETTINGS__NETBOX_TOKEN` |
| B3 | Crate `netbox-model` (cynic, eigenes Schema, eingecheckte `.idea/`) wird von niemandem benutzt: entfernen (Z5) | S | erledigt 2026-10-23 („remove the unused netbox-model crate“) |
| B4 | `DeviceCfg::generate_from` und `BaseDeviceDataTarget::generate_from` rufen dieselben `setup_*`-Schritte je selbst auf (PoE fehlte so einmal im echten Weg): eine gemeinsame Funktion | S | erledigt 2026-10-23 („share the setup steps of both generate_from“): `setup_addressing` vor, `setup_ports` nach den Radios |
| B5 | `lazy_static` (3 Dateien, dazu der Generator von mikrotik-model) durch `std::sync::LazyLock` ersetzen | S | erledigt 2026-10-23 („replace lazy_static with LazyLock“, in beiden Repos) |
| B6 | Ein vergifteter Mutex in `job.rs` (`.expect("jobs")`) lässt den Server abstürzen statt nur den Job | S | erledigt 2026-10-23 („keep the jobs going after a panic holding their locks“), ebenso die Leases in `device/power.rs` |
| B7 | Grosse Dateien aufteilen: `frontend/src/components/netbox_helper.rs` (2090 Zeilen, mit C12), `device/ros/mod.rs` (2067, erledigt 2026-10-23 „split the radios and the DHCP pool off the generator“: 1247 Zeilen, Radios in `radio/`, Pool in `dhcp.rs`), `frontend/src/i18n.rs` (1609, erledigt 2026-10-23 „give each part of the UI texts a file“: `i18n/`, eine Datei pro Modul), `helper/mod.rs` (1391, erledigt 2026-10-23 „split the plan steps and the writing off the helpers“: 541 Zeilen, `plan.rs`, `write.rs`) | M | erledigt 2026-10-23, bis auf `netbox_helper.rs`, das mit C12 neu aufgeteilt wird |
| B8 | mikrotik-model: Der Generator gibt die Enums jedes Mal in anderer Reihenfolge aus (HashMap); Diffs des erzeugten Codes unlesbar, vermutlich unnötiges Neukompilieren | S | erledigt 2026-10-23 (mikrotik-model „generate the same code on every build“) |
| B9 | Ersatz für `speed: k` an `/interface/ethernet` suchen (nur mangels Alternative akzeptiert) | M | offen |
| B10 | `graphql_client` 0.16 von crates.io statt des Forks (`Chrisss93/graphql-client`, Branch `reqwest-upgrade`) prüfen; nur als Übergang, bis C4 den Fork ablöst (Z6) | S | erledigt 2026-10-23 („take graphql_client from crates.io“): ohne Features, `json` direkt an reqwest; Topologie aus der Lab-NetBox und Site-Seite im Browser geprüft |

## C. Konzepte zum Umsetzen

| # | Aufgabe | Konzept | Aufwand | Status |
|---|---|---|---|---|
| C1 | Perimeter-Firewall: Tag `internet` (Z2), Grundregeln IPv4/IPv6, SSH mit Bruteforce-Schutz, Zugang des Provisioners, IPv4-NAT | [Firewall](concepts/firewall.md#perimeter-firewall) | M | offen |
| C2 | Port-Forwarding IPv4 (ohne Umsetzen der Ports, Z3) und Freigaben IPv6 aus NetBox-Services (Tag `forward`) | [Firewall](concepts/firewall.md#perimeter-firewall) | M | offen |
| C3 | Interne Firewall: `guest` und `protected` am Gateway-Interface, Freigaben als Custom Fields (Z4), nur ein Router pro Netz | [Firewall](concepts/firewall.md#interne-firewall) | M | offen |
| C4 | cynic statt graphql_client, Abfrage für Abfrage (Spike erfolgreich) | [GraphQL-Dateien](concepts/graphql-files.md) | M–L | offen |
| C5 | Befehle ohne Strings: Stufe 1, getypte Befehle aus dem Modell (Stufe 2 nicht geplant, Z7) | [Befehle ohne Strings](concepts/command-dsl.md) | M–L | offen |
| C6 | Deployment: Nix-Image (Z8), Flake, Git-Abhängigkeiten für mikrotik-model/-api, GitHub Actions, Helm-Chart; Cluster und Argo CD von Hand (Z9) | [Deployment](concepts/deployment.md) | L | offen |
| C7 | Self-Service und Personas: Rollen aus OIDC, NetBox-Owner als Anwohner, Leserechte in NetBox nur auf eigene WLANs (Z10, Z11), Leases, SSID/PSK, Ports, eigene APs, Apply sofort (Z12) | [Self-Service](concepts/self-service.md) | L | offen |
| C8 | Überwachung: Collector, Prometheus-Metriken (Alarme dort, Z13), Ping zum Uplink-Gateway (Z14), Zustandsseite, Ansicht für Anwohner | [Überwachung](concepts/monitoring.md) | L | offen |
| C9 | Netz-Karte: Graph aus der OSPF-LSDB, SVG, Farben nach Erreichbarkeit, Health nur angezeigt (Z16), PoE, Layout publizieren oder im Browser (Z15) | [Netz-Karte](concepts/network-map.md) | L | offen |
| C10 | Updates im ganzen Netz: mehrere Sites (Z20), Pilot vorgeschlagen (Z18), ohne Wartungsfenster (Z19) | [Updates im ganzen Netz](concepts/network-update.md) | L | offen |
| C11 | Kaltstart eines PoE-gespeisten Geräts auf Knopfdruck: PoE am speisenden Port aus und wieder ein, mit Bestätigung (Z22); für Device-Mode und Rettung nach einem Update | [Update-Fallback](concepts/update-fallback.md) | S–M | offen |
| C12 | Interfaces nach Aufgabe statt Helfer-Auswahl: Zuordnung (Loopback, Uplink, Direkt, Richtfunk, Kunden, Unbekannt) und Ansicht nur lesend (M); Lösungsvorschläge im Modal mit Stepper über die vorhandenen Helfer, „NetBox ergänzen“ fällt weg (M); Live-Prüfungen (S–M); neue Befunde: ein L3-Übergang pro VLAN (Z27), tagged VLAN am Port (Z26), Strecke unvollständig, Brückengerät ohne Loopback, Label (M); neue Helfer: Richtfunk und Kunden-Anschluss abräumen, VLAN am Port, Funkbrücke vom Router-Port, Kabel übernehmen (M–L); Uplinks ohne Helfer (Z25) | [Interfaces nach Aufgabe](concepts/interface-view.md) | L | in Arbeit: Schritt 1 erledigt 2026-10-23 („show the interfaces of a device by task“), Schritt 2 erledigt 2026-10-23 („open the helpers from the interface rows“), Schritt 3 erledigt 2026-10-23 („check the interface rows live“) |

## D. In Arbeit, Rest offen

| # | Aufgabe | Konzept | Aufwand | Status |
|---|---|---|---|---|
| D1 | Helfer „Neues Gerät“: Gerät in NetBox (Site, Name, Typ erkennen oder eingeben), Ablauf vom Gerät im Feld bis zum ersten Apply; Loopback, Verbindung und Ersteinrichtung gibt es | [Config-Helper](concepts/config-helper.md), [User Journeys](concepts/user-journeys.md#rollout) | L | in Arbeit |
| D2 | Datenkorrektur: Übersicht pro Site, wie weit jedes Gerät von NetBox abweicht, als Job mit Zwischenstand (Z21, M); Adressen übernehmen, die nur auf dem Gerät stehen (M) | [User Journeys](concepts/user-journeys.md#datenkorrektur) | M–L | in Arbeit |
| D3 | Update-Fallback Stufe 2: Rettung über mac-telnet (M), Downgrade auf Knopfdruck (S–M), Bestätigung für ein Update ohne Fallback (Z23, S); das selbständige Zurückschalten im Lab testen (S) | [Update-Fallback](concepts/update-fallback.md) | M | in Arbeit |
| D4 | Geräteseite: Bereich für Liegengebliebenes auf dem Gerät und Befunde | [Geräteseite](concepts/device-page.md) | S–M | in Arbeit |
| D5 | Jobs: Events speichern (`state_dir`, M), geordnetes Beenden (S), Liegengebliebenes anzeigen (S) | [Jobs](concepts/job-polling.md) | M | in Arbeit |
