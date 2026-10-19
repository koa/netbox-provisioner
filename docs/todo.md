# Aufgaben

Alle offenen Aufgaben über die drei Repositories (netbox-provisioner, mikrotik-model,
mikrotik-api), gesammelt aus [pending.md](pending.md), den [Konzepten](concepts/) und der
Durchsicht des Codes vom 2026-10-19. Die Nummern bleiben, auch wenn umsortiert wird; erledigte
Aufgaben werden mit Datum und Commit markiert, nicht gelöscht.

Aufwand grob: **S** Stunden, **M** ein bis zwei Tage, **L** mehr.
Status: **offen**, **in Arbeit**, **erledigt**.

## Reihenfolge

Vorschlag, noch nicht entschieden:

1. A1–A4 (Sicherheit)
2. B1, B2, B3, B4, B8, B10 (klein)
3. C1 (Perimeter-Firewall, baut auf A1 auf)
4. danach nach Priorität: D1 und D2 bringen im Alltag am meisten, C6 braucht der Betrieb

## Offene Entscheidungen

Die „Zu entscheiden“-Punkte der Konzepte und was die Durchsicht offen liess, mit dem Vorschlag
(falls einer gemacht ist) und den Aufgaben, die davon abhängen. Entschiedenes wird hier mit
der Entscheidung markiert und ins Konzept übernommen.

| # | Frage | Vorschlag | betrifft |
|---|---|---|---|
| Z1 | Firewall-Abgleich: woran erkennt der Provisioner seine eigenen Regeln? Kommentar-Präfix, oder alle Regeln an den Interfaces, die NetBox kennt? | Kommentar-Präfix | A1 |
| Z2 | Neues Tag `internet` für jedes Interface zum Internet, oder genügt `nat` (dann bleibt ein Uplink ohne NAT ungeschützt)? | `internet` | C1 |
| Z3 | `public_port` als Custom Field am Service, oder ein zweiter Service am Router? | – | C2 |
| Z4 | Freigaben der internen Firewall als Custom Fields am Interface, oder als eigene Prefix-Tags? | – | C3 |
| Z5 | `netbox-model` entfernen oder als Grundlage für cynic im Backend nehmen? | entfernen, falls C4 nicht bald kommt | B3, C4 |
| Z6 | cynic oder Abfragen in einzelne Dateien aufteilen (und dann Unterordner jetzt oder bei Bedarf)? | cynic | C4, B10 |
| Z7 | Befehle ohne Strings: lohnt Stufe 2, oder reichen Tests auf die gerenderten Skripte? | zuerst Tests, Stufe 2 nur bei weiteren Fehlern | C5 |
| Z8 | Nix-Image oder Containerfile? | Nix | C6 |
| Z9 | Welcher Cluster? | – | C6 |
| Z10 | Anwohner als Tenant oder als eigenes Objekt (Contact Group)? | Tenant | C7 |
| Z11 | Ports eines Anwohners über ein Custom Field `owner` am Interface, oder nur über das Gerät? | – | C7 |
| Z12 | Wird ein Apply eines Anwohners sofort ausgeführt oder von einem Admin freigegeben? | – | C7 |
| Z13 | Alarme im Provisioner selbst oder nur über Prometheus/Alertmanager? | nur Prometheus | C8 |
| Z14 | Ziel des Uplink-Pings (je NAT-Router eins, als Config Context)? | – | C8 |
| Z15 | Wo liegen die Positionen der Netz-Karte? | Datei | C9 |
| Z16 | Grenzwerte für Health global in `config.yaml` oder pro Gerät im Config Context? | global, Config Context als spätere Übersteuerung | C9 |
| Z17 | Power Ports in NetBox modellieren, damit `psuN-state` richtig bewertet wird? | – | C9 |
| Z18 | Pilot eines Netz-Updates automatisch wählen oder anklicken lassen? | automatisch vorschlagen, im Plan änderbar | C10 |
| Z19 | Wartungsfenster (Start zu einer festen Zeit)? | vorerst nicht | C10 |
| Z20 | Netz-Update pro Site oder über alle Sites eines Tenants? | pro Site | C10 |
| Z21 | Diff-Grösse aller Geräte einer Site live beim Öffnen oder als Job mit Zwischenstand? | Job | D2 |
| Z22 | Kaltstart PoE-gespeister Geräte selbst auslösen (PoE am speisenden Switch aus und ein)? | – | D3 |
| Z23 | Updates ganz ohne Fallback (16 MiB, kein erreichbarer Nachbar) nur mit Bestätigung? | ja, mit Bestätigung | D3 |
| Z24 | Erfolgsbedingung nach einem Update: nur die OSPF-Nachbarn, die vorher `Full` waren? | ja | D3 |

## A. Sicherheit

Heute offen: Ein Router mit Internet-Zugang ist nach einem Apply von aussen offen, bis auf DNS
(siehe [Firewall](concepts/firewall.md#ausgangslage)).

| # | Aufgabe | Aufwand | Status |
|---|---|---|---|
| A1 | Firewall-Abgleich: nur eigene Regeln verwalten (z.B. per Kommentar-Präfix), fremde stehen lassen; Reihenfolge der Regeln beachten. Heute paart `generate_add_update_remove_by_id` die Regeln, schreibt sie um und entfernt den Rest | M | offen |
| A2 | `/ip/service` ins Modell: telnet, ftp, www aus; API nur aus den eigenen Netzen bzw. api-ssl; für alle Geräte | S–M | offen |
| A3 | SOCKS des Management-Proxys nur von der Adresse des Provisioners annehmen (heute auf allen Interfaces, nur mit Passwort) | S | offen |
| A4 | Abhängigkeiten (`cargo audit`: 34 Hinweise, 12 Warnungen): `actix-4-jwt-auth` 1.2.5 → 1.2.9 (rustls 0.20, ring 0.16), `prometheus` 0.13 → 0.14 und Ersatz für `actix-web-prometheus` (protobuf 2.28), Subscriptions-Feature von `async-graphql-actix-web` weglassen (`actix-web-actors`, crossbeam-channel), `cargo update` (h2, openssl, quinn-proto, rsa, bincode) | S–M | offen |
| A5 | Prüfen, ob schon ein Router mit eigener Firewall angewendet wurde; falls ja, seine Firewall kontrollieren (Betreiber) | – | offen |

## B. Bereinigungen

| # | Aufgabe | Aufwand | Status |
|---|---|---|---|
| B1 | `binary/build.rs` startet `trunk` mit `spawn()` und wartet nicht (kann ein halbfertiges Frontend einbetten); die Hooks in `frontend/Trunk.toml` (`clear`, `npm install`) gehören aus dem Build | S | offen |
| B2 | `config::Environment::with_prefix("app")` ohne Trenner: einzelne Werte (`settings.netbox_token`) lassen sich nicht per Umgebungsvariable setzen; Trenner `__` | S | offen |
| B3 | Crate `netbox-model` (cynic, eigenes Schema, eingecheckte `.idea/`) wird von niemandem benutzt: entfernen oder als Grundlage für C4 nehmen | S | offen |
| B4 | `DeviceCfg::generate_from` und `BaseDeviceDataTarget::generate_from` rufen dieselben `setup_*`-Schritte je selbst auf (PoE fehlte so einmal im echten Weg): eine gemeinsame Funktion | S | offen |
| B5 | `lazy_static` (3 Dateien) durch `std::sync::LazyLock` ersetzen | S | offen |
| B6 | Ein vergifteter Mutex in `job.rs` (`.expect("jobs")`) lässt den Server abstürzen statt nur den Job | S | offen |
| B7 | Grosse Dateien aufteilen: `frontend/src/components/netbox_helper.rs` (2090 Zeilen), `device/ros/mod.rs` (2067), `frontend/src/i18n.rs` (1609), `helper/mod.rs` (1391) | M | offen |
| B8 | mikrotik-model: Der Generator gibt die Enums jedes Mal in anderer Reihenfolge aus (HashMap); Diffs des erzeugten Codes unlesbar, vermutlich unnötiges Neukompilieren | S | offen |
| B9 | Ersatz für `speed: k` an `/interface/ethernet` suchen (nur mangels Alternative akzeptiert) | M | offen |
| B10 | `graphql_client` 0.16 von crates.io statt des Forks (`Chrisss93/graphql-client`, Branch `reqwest-upgrade`) prüfen; Zwischenschritt vor C4 | S | offen |

## C. Konzepte zum Umsetzen

| # | Aufgabe | Konzept | Aufwand | Status |
|---|---|---|---|---|
| C1 | Perimeter-Firewall: Tag `internet`, Grundregeln IPv4/IPv6, SSH mit Bruteforce-Schutz, Zugang des Provisioners, IPv4-NAT | [Firewall](concepts/firewall.md#perimeter-firewall) | M | offen |
| C2 | Port-Forwarding IPv4 und Freigaben IPv6 aus NetBox-Services (Tag `forward`) | [Firewall](concepts/firewall.md#perimeter-firewall) | M | offen |
| C3 | Interne Firewall: `guest` und `protected` am Gateway-Interface, nur ein Router pro Netz | [Firewall](concepts/firewall.md#interne-firewall) | M | offen |
| C4 | cynic statt graphql_client, Abfrage für Abfrage (Spike erfolgreich) | [GraphQL-Dateien](concepts/graphql-files.md) | M–L | offen |
| C5 | Befehle ohne Strings: Stufe 1 getypte Befehle aus dem Modell, Stufe 2 Skripte | [Befehle ohne Strings](concepts/command-dsl.md) | L | offen |
| C6 | Deployment: Nix-Image (Versuch erfolgreich), Flake, Git-Abhängigkeiten für mikrotik-model/-api, GitHub Actions, Helm, Argo CD | [Deployment](concepts/deployment.md) | L | offen |
| C7 | Self-Service und Personas: Rollen aus OIDC, Tenant als Anwohner, Leases, SSID/PSK, Ports, eigene APs | [Self-Service](concepts/self-service.md) | L | offen |
| C8 | Überwachung: Collector, Prometheus-Metriken, Zustandsseite, Ansicht für Anwohner | [Überwachung](concepts/monitoring.md) | L | offen |
| C9 | Netz-Karte: Graph aus der OSPF-LSDB, SVG, Zustandsfarben, PoE | [Netz-Karte](concepts/network-map.md) | L | offen |
| C10 | Updates im ganzen Netz | [Updates im ganzen Netz](concepts/network-update.md) | L | offen |

## D. In Arbeit, Rest offen

| # | Aufgabe | Konzept | Aufwand | Status |
|---|---|---|---|---|
| D1 | Helfer „Neues Gerät“: Gerät in NetBox (Site, Name, Typ erkennen oder eingeben), Ablauf vom Gerät im Feld bis zum ersten Apply; Loopback, Verbindung und Ersteinrichtung gibt es | [Config-Helper](concepts/config-helper.md), [User Journeys](concepts/user-journeys.md#rollout) | L | in Arbeit |
| D2 | Datenkorrektur: Übersicht pro Site, wie weit jedes Gerät von NetBox abweicht (M); Adressen übernehmen, die nur auf dem Gerät stehen (M) | [User Journeys](concepts/user-journeys.md#datenkorrektur) | M–L | in Arbeit |
| D3 | Update-Fallback Stufe 2: Rettung über mac-telnet (M), Downgrade auf Knopfdruck (S–M); das selbständige Zurückschalten im Lab testen (S) | [Update-Fallback](concepts/update-fallback.md) | M | in Arbeit |
| D4 | Geräteseite: Bereich für Liegengebliebenes auf dem Gerät und Befunde | [Geräteseite](concepts/device-page.md) | S–M | in Arbeit |
| D5 | Jobs: Events speichern (`state_dir`, M), geordnetes Beenden (S), Liegengebliebenes anzeigen (S) | [Jobs](concepts/job-polling.md) | M | in Arbeit |
