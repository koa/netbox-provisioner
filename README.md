# netbox-provisioner

Baut MikroTik-Netze aus NetBox: Der Provisioner liest die Topologie aus
[NetBox](https://netboxlabs.com/oss/netbox/), leitet daraus für jedes RouterOS-Gerät die
vollständige Konfiguration ab, vergleicht sie mit dem, was auf dem Gerät läuft, und wendet die
Differenz an. NetBox ist die einzige Quelle der Wahrheit; was dort nicht steht, entfernt ein
Apply vom Gerät.

Bedient wird er über eine Weboberfläche (Rust/WebAssembly) mit OIDC-Login. Zu jedem Gerät zeigt
sie, was es von NetBox unterscheidet, als RouterOS-Skript, das man prüft und dann anwendet.

## Was er kann

- **Konfiguration aus NetBox:** Identity und Loopback, Bridges und VLANs (auch VLAN-Filtering in
  Hardware, wo das Board es kann), IP-Adressen, DHCP-Server und -Client, OSPF v2/v3, VXLAN für
  Netze über mehrere Standorte, Perimeter-Firewall mit NAT und Port-Weiterleitungen, WLAN (CAPsMAN
  und das neue `interface/wifi`), Funkbrücken, Watchdogs, Syslog. Wie man Geräte dafür in NetBox
  pflegt, steht in [docs/netbox-modeling.md](docs/netbox-modeling.md).
- **Diff und Anwenden mit Rückfallebene:** Vor dem Anwenden sichert das Gerät seine
  Konfiguration und lädt sie selbständig zurück, wenn der Provisioner es danach nicht mehr
  erreicht. Eine Änderung, die den eigenen Zugang kappt, nimmt sich so selbst zurück. Mehrere
  Geräte, etwa die beiden Seiten einer Funkstrecke, lassen sich gemeinsam anwenden.
- **Erreichbarkeit über Umwege:** Geräte werden direkt, über einen SOCKS-Proxy auf einem
  Einstiegsrouter oder über einen Nachbarn erreicht (auch per MAC-Telnet für die
  Ersteinrichtung eines Geräts im Werkszustand).
- **RouterOS-Updates** mit Kanalwahl, Downgrade, zwei Partitionen als Fallback, Device-Mode und
  „Strom aus und ein“ über den speisenden PoE-Port.
- **Befunde über NetBox:** was in NetBox fehlt oder widersprüchlich ist, mit Helfern, die es in
  NetBox ergänzen (neues Gerät an einem freien Port, Funkbrücke, Adressen aus Pools), und ein
  Vergleich der Kabel in NetBox mit dem, was die Geräte als Nachbarn sehen.
- **Jobs**, die einen Neustart des Provisioners überstehen, ein Protokoll pro Job im Journal des
  Geräts in NetBox, Seiten, die sich live aktualisieren, und Prometheus-Metriken mit einem
  [Grafana-Dashboard](charts/netbox-provisioner/dashboards/device-queries.json), das das Helm-Chart
  für den Grafana-Sidecar mitbringt.

Beispiele dessen, was herauskommt, liegen in [examples/](examples/).

## Voraussetzungen

- NetBox mit den Tags und Custom Fields des Provisioners. Beides lässt sich aus
  [docs/netbox-import/](docs/netbox-import/) importieren ([Custom Fields](docs/custom-fields.md),
  [Tags](docs/netbox-modeling.md#tags)). Ein Token zum Lesen, für die Helfer ein zweites mit
  Schreibrechten.
- Ein OIDC-Provider, z.B. [Pocket-ID](https://pocket-id.org/) oder Keycloak
  ([docs/login.md](docs/login.md)).
- MikroTik-Geräte mit RouterOS 7, deren API (Port 8728) vom Provisioner aus erreichbar ist,
  direkt oder über einen Management-Proxy.

## Konfiguration

Der Provisioner liest `config.yaml` aus seinem Arbeitsverzeichnis. Jeder Wert lässt sich mit
einer Umgebungsvariablen übersteuern (`APP_SETTINGS__NETBOX_TOKEN` für `settings.netbox_token`).
Ein Minimum:

```yaml
settings:
  # Login (docs/login.md)
  auth_issuer: https://id.example.ch
  auth_client_id: 3f2c9a1e-...
  auth_users: [8b1d6e2a-...]       # der Claim "sub" der Benutzer, die hinein dürfen

  netbox_url: https://netbox.example.ch/graphql/
  netbox_token: "..."

  # Zugangsdaten der Geräte, gewählt über das Custom Field mikrotik_credentials am Tenant
  mikrotik_credentials:
    standort-a:
      user: admin
      password: "..."

  # Schreiben ist ausgeschaltet, bis es hier freigegeben wird
  allow_apply: false               # auf Geräte
  allow_netbox_write: false        # nach NetBox (die Helfer), mit netbox_write_token
```

Ohne `allow_apply` liest und vergleicht der Provisioner nur. Weitere Einstellungen
(Management-Proxy, `api_networks`, RouterOS-Kanal, Syslog, Fallback-Zeiten) sind in
[backend/src/config.rs](backend/src/config.rs) beschrieben.

UI und API laufen auf `server_port` (Vorgabe 8080), `/metrics` und `/health` auf
`server_mgmt_port` (Vorgabe 9080).

## Betrieb

Image und Helm-Chart veröffentlicht die CI auf GitHub Container Registry:

```sh
helm install netbox-provisioner oci://ghcr.io/koa/charts/netbox-provisioner \
  -f my-values.yaml
```

Jede Einstellung von `config.yaml` ist ein eigener Wert unter `settings`; Geheimnisse kommen als
Wert oder aus einem eigenen Secret
([charts/netbox-provisioner/values.yaml](charts/netbox-provisioner/values.yaml)). Das Image ist
mit Nix gebaut und enthält nur das Binary und seine Bibliotheken, keine Shell. Mehr dazu, zu
Probes, Netzwerk und Beenden in [docs/deployment.md](docs/deployment.md).

## Entwicklung

Das Projekt braucht die beiden Bibliotheken [mikrotik-model](https://github.com/koa/mikrotik-model)
und [mikrotik-api](https://github.com/koa/mikrotik-api), als Git-Abhängigkeiten (Zweig `master`, festgelegt in
`Cargo.lock`). Um sie mitzuändern, liegen sie als Nachbarverzeichnisse daneben; solange eine
geändert wird, zeigt `backend/Cargo.toml` vorübergehend mit `path` auf sie
([docs/deployment.md](docs/deployment.md)):

```
workspace/
├── mikrotik-api/
├── mikrotik-model/
└── netbox-provisioner/
```

Bauen mit Nix (bringt die ganze Toolchain mit):

```sh
nix build .#provisioner   # das Binary
nix build .#image         # das OCI-Image
nix flake check           # Tests und Clippy
```

Oder von Hand mit Rust samt Ziel `wasm32-unknown-unknown`, Node.js, `trunk` und `dart-sass`:
`build.sh` installiert die Node-Module in `frontend/node`, baut das Frontend
(`trunk build --release`) und dann das Binary, das es einbettet (`cargo build -p binary`).

Aufbau:

| Verzeichnis | Inhalt |
|---|---|
| `backend/` | NetBox-Abfrage, Topologie, Generierung der RouterOS-Konfiguration, Jobs, GraphQL-API |
| `frontend/` | die Weboberfläche (Yew, WebAssembly) |
| `binary/` | der Server: Login, HTTP, WebSocket, Metriken |
| `i18n/` | die deutschen Texte, gemeinsam für Frontend und Backend |
| `charts/` | das Helm-Chart |
| `local/` | lokale Testumgebung: NetBox, Keycloak, Lab-Skripte |
| `docs/` | die Beschreibung, siehe unten |

Die Tests unter `backend/tests/chr_network/` starten echte RouterOS-Instanzen (CHR in QEMU) und
laufen nur mit `cargo test -- --ignored`.

## Dokumentation

Die Dokumentation ist auf Deutsch. Einstieg ist [docs/overview.md](docs/overview.md):

- [Geräte in NetBox modellieren](docs/netbox-modeling.md) und [Custom Fields](docs/custom-fields.md)
- [Geräteseite](docs/device-page.md), [Interfaces nach Aufgabe](docs/interface-view.md),
  [Config-Helper](docs/config-helper.md)
- [Jobs](docs/jobs.md), [Gerätezustand](docs/device-state.md),
  [Update-Fallback](docs/update-fallback.md), [Watchdog](docs/watchdog.md),
  [Firewall](docs/firewall.md)
- [Login](docs/login.md), [Deployment](docs/deployment.md)

Was geplant ist, steht in [docs/concepts/](docs/concepts/), die offenen Aufgaben in
[docs/todo.md](docs/todo.md).
