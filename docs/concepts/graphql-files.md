# Konzept: Eine Datei pro GraphQL-Abfrage

Status: Konzept, nicht umgesetzt; cynic als Alternative im Spike geprüft. Offener Punkt „Graphql Files“ aus
[pending.md](../pending.md#graphql-files).

## Ausgangslage

Die Abfragen des Frontends liegen in `frontend/src/graphql/authenticated/`, gruppiert nach Seite
oder Komponente, nicht eine pro Datei:

| Datei | Abfragen |
|---|---|
| `netbox-helper.graphql` | 11 |
| `show-device.graphql` | 9 |
| `batch-apply.graphql`, `job.graphql`, `list-devices.graphql` | je 3 |
| `adjust-target.graphql`, `findings.graphql`, `neighbor-findings.graphql` | je 1 |

Jede Abfrage wird in `graphql/authenticated/mod.rs` mit `#[derive(GraphQLQuery)]` und
`query_path` an ihre Datei gebunden. Das Backend hat nur eine, `netbox/fetch_topology.graphql`.
Die Fragmente wiederholen sich: Die Events eines Jobs (`job.graphql`) und die Schritte eines
Plans (`netbox-helper.graphql`) stehen je einmal, aber Teile wie `device{ name }` an vielen Stellen.

Was die Gruppierung kostet: Eine Datei mit elf Abfragen ist schwer zu überblicken. Ein Diff in
einer Abfrage zeigt nicht, welche gemeint ist, und `graphql_client` erzeugt die Typen aller
Abfragen einer Datei, auch wenn das Derive nur eine braucht (das ist billig, aber unübersichtlich
beim Suchen nach einem Typnamen).

## Vorschlag

- **Eine Datei pro Operation**, benannt nach ihr in kebab-case: `netbox-plan.graphql`,
  `netbox-plan-preview.graphql`, `customer-net-options.graphql` usw. Die Verzeichnisse bleiben
  (`authenticated/`, `anonymous/`), darunter optional ein Unterordner pro Bereich
  (`helper/`, `device/`, `job/`), sobald es mehr als etwa 15 Dateien werden.
- **Fragmente in eigene Dateien**, wo sie mehrfach gebraucht werden (`job-event.graphql` für die
  Events, `plan-step.graphql` für die Schritte). `graphql_client` kennt kein `#import`; ein
  Fragment muss in derselben Datei stehen wie die Abfrage. Deshalb entweder
  - die Fragmente bleiben bei ihrer einen Abfrage (heute schon so), oder
  - `frontend/build.rs`, das das Schema ohnehin schreibt, setzt Abfrage und benutzte Fragmente zu
    einer erzeugten Datei in `OUT_DIR` zusammen. Mehr Aufwand; nur, wenn Fragmente wirklich
    mehrfach vorkommen.
- **Namen**: Datei = Operationsname, so findet man von der Rust-Struktur (`NetboxPlan`) direkt zur
  Datei.

## Alternative: cynic statt `.graphql`-Dateien

[cynic](https://cynic-rs.dev) (3.14, crates.io) geht den umgekehrten Weg: Die Abfrage ist ein
Rust-Struct (`#[derive(cynic::QueryFragment)]`), cynic erzeugt daraus den Abfragetext und prüft
jedes Feld beim Kompilieren gegen das Schema. Es gibt dann keine `.graphql`-Dateien mehr, und
damit erledigt sich dieser Punkt.

**Spike** (2026-10-19, im Scratchpad, nicht im Repository): `JobEvents` aus `job.graphql` mit
einem Teil der Union `Event`, gegen unser erzeugtes `schema.graphql`; dazu eine
`device_list`-Abfrage gegen das NetBox-Schema des Backends (`schema.graphqls`, 10 386 Zeilen).

- **Funktioniert:**
  - Variablen und Argumente (`#[arguments(id: $id)]`);
  - unsere Skalare (`ScalarDuration`);
  - die Union als `#[derive(InlineFragments)]` mit einem `#[cynic(fallback)]`-Fall: Ein Event,
    das das Frontend nicht kennt, landet dort, statt das Parsen scheitern zu lassen.

  Erzeugter Abfragetext und Parsen einer Antwort stimmen.
- **Fehler beim Kompilieren:** Ein falscher Feldname (`safety_nett`) ergibt „no field
  `safetyNett` on the GraphQL type `WatchdogKept`. Did you mean `safetyNet`?“, an der Zeile im
  Struct.
- **wasm32** baut. Ein Transport ist nicht nötig: `query_authenticated` schickt die serialisierte
  `Operation` weiter selbst mit reqwest.
- **NetBox-Schema:** Es benennt Felder in snake_case; dafür braucht jedes Struct
  `rename_all = "None"`. Das Schema einzulesen (`register_schema` mit `rkyv` in `build.rs`)
  kostet beim inkrementellen Bauen 2–3 s.

**Vergleich mit heute:**

| | graphql_client (heute) | cynic |
|---|---|---|
| Quelle | `.graphql`-Datei + erzeugte Typen | Rust-Structs, Abfrage erzeugt |
| Abhängigkeit | crates.io 0.16 (bis 2026-10-23 ein Git-Fork, `Chrisss93/graphql-client`) | crates.io, gepflegt |
| Typen | pro Abfrage neu erzeugt (`…TopologyDeviceByIdCustomerNetOptionsParents`), gleiche Daten in verschiedenen Typen (beim IPv6-Kunden-Netz zwei Parent-Typen) | selbst benannt, wiederverwendbar über Abfragen hinweg |
| Fragmente | nur in derselben Datei | gewöhnliche Structs, beliebig geteilt |
| Unbekannte Union-Fälle | Fehler beim Parsen, je nach Einstellung | `fallback` |
| Schema-Änderung im Backend | schlägt beim Kompilieren des Frontends fehl | ebenso |
| Abfrage als Text lesen | direkt in der Datei | nur erzeugt (`operation.query`) |

**Umstieg:** Abfrage für Abfrage, beide Bibliotheken können eine Weile nebeneinander laufen. Für
den Anfang hilft `cynic-querygen` (Crate, auch als Generator auf cynic-rs.dev): Es erzeugt die
Structs aus einer vorhandenen Abfrage und dem Schema.
Zuerst das Frontend (32 Abfragen, das eigene Schema), dann `fetch_topology.graphql` im Backend.
Danach fällt `graphql_client` weg.

Vorschlag: cynic statt Dateien aufzuteilen. Der Gewinn ist grösser (geteilte Typen,
Fehler am Struct), und die Aufteilung der Dateien erübrigt sich.

## Reihenfolge

Mit cynic:

1. cynic ins Frontend, Schema in `frontend/build.rs` registrieren (das dort ohnehin geschrieben
   wird), eine kleine Abfrage umstellen (`FindingsList` o.ä.).
2. Die Job-Events (die grösste Union, mit `fallback`), dann die Helfer, dann der Rest.
3. `fetch_topology.graphql` im Backend; danach `graphql_client` entfernen.

Bleibt es bei graphql_client:

1. `netbox-helper.graphql` und `show-device.graphql` aufteilen (20 der 32 Abfragen). Rein
   mechanisch, prüfbar mit dem Build und den drei Clippy-Läufen.
2. Die übrigen Dateien mit mehreren Abfragen.
3. Fragmente erst, wenn eines in zwei Abfragen gebraucht wird.

## Entschieden (2026-10-23)

- **cynic** statt die Dateien aufzuteilen. Umstieg Abfrage für Abfrage; der Fork von
  graphql_client fällt am Ende weg. Die Crate `netbox-model` wird entfernt, die NetBox-Abfrage
  des Backends neu gegen das aktuelle Schema geschrieben.
