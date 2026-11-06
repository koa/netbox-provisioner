# GraphQL im Provisioner

Beide Seiten fragen mit [cynic](https://cynic-rs.dev): Eine Abfrage ist ein Rust-Struct
(`#[derive(cynic::QueryFragment)]`), cynic erzeugt daraus den Abfragetext und prüft jedes Feld
beim Kompilieren gegen das Schema. Ein falscher Feldname ist ein Fehler an der Zeile im Struct
(„no field `safetyNett` on the GraphQL type `WatchdogKept`. Did you mean `safetyNet`?“), eine
Schema-Änderung im Backend lässt das Frontend nicht mehr bauen. `.graphql`-Dateien gibt es
keine; den Text einer Abfrage gibt `operation.query`.

## Frontend

`frontend/build.rs` schreibt die Schemas der beiden Endpunkte und registriert sie, ohne Default:

- **authenticated** (`/graphql`, nach dem Login): die Abfragen in
  `frontend/src/graphql/authenticated/<bereich>.rs` (`device`, `job`, `netbox_plan`,
  `helper_options`, `site_diff`, …), jeder Typ mit `schema = "authenticated"`. Typen, die
  mehrere Abfragen brauchen, stehen in `authenticated/types.rs` (`DeviceRef`, `InterfaceRef`,
  `ErrorInfo`, `JobPhase`, `ApplyWarning`, `DeviceVariables`, `JobId`, …), der Input `Helper`
  (ein `@oneOf`, als Enum) in `authenticated/helper.rs`.
- **anonymous** (`/graphql_anonymous`, vor dem Login): nur `Settings` in `graphql/anonymous/`.
  Die beiden bleiben getrennt; kein Typ des einen taugt für das andere.

Gesendet wird mit `graphql::request` (die Daten, ohne Daten die Fehler als
`FrontendError::Graphql`) oder `request_response` (Daten und Fehler nebeneinander, z.B. das
Gerät mit dem Grund, warum es nicht erreichbar ist), anonym mit `request_anonymous`.
`graphql::live::watch` abonniert über die WebSocket des Tabs ([Gerätezustand](device-state.md)),
`watch_data` gibt wie `request` die Daten. Antwort und Fehler sind cynics
(`graphql::Response`, `GraphqlError`, mit den `extensions` als JSON für `code` und `params`);
`path_segment` macht aus dem `path` eines Fehlers Text.

Muster:

- Eine Union ist `#[derive(cynic::InlineFragments)]` mit einem `#[cynic(fallback)]`-Fall: Ein
  Event oder Plan-Schritt, den das Frontend noch nicht kennt, landet dort (die Seite bittet, neu
  zu laden), statt die ganze Antwort scheitern zu lassen. `__typename` lässt sich auf einer
  Union nicht auswählen; wo der Name gebraucht wird, gibt ihn eine Methode
  (`FindingKind::name`).
- Ein Enum hat den Fall `Other(String)`: Er nimmt einen neuen Wert des Backends auf und geht als
  derselbe String zurück, taugt also auch für Inputs.
- Eine Zahl, die über die 32 Bit von GraphQL's `Int` gehen kann (Bytes, Hosts eines
  IPv6-Pools), gibt das Backend als eigenen Skalar `ScalarU64` aus (`graphql::scalars`, Z38),
  das Frontend liest ihn als `types::WideInt`. Kleine Zahlen (`usize`, `u32`) bleiben `Int`.
  Eigene Skalare sind Newtypes (`Mac`, `WideInt`), weil `impl_scalar!` keinen fremden Typ nimmt
  und ein Typ nur einen Skalar trägt (`String` ist schon `String`, `u64` schon
  `ScalarDuration`).
- Geteilte Variablen (`DeviceVariables`) brauchen in einem anderen Modul auch ihren erzeugten
  Typ `…Fields` im Import.

## Backend: die NetBox-Abfrage

`backend/build.rs` registriert NetBox' Schema (`backend/src/netbox/schema.graphqls`) als
Default; die eine Abfrage, aus der die Topologie entsteht, ist `FetchTopology` in
`backend/src/netbox/fetch_topology.rs`. Was sie liest, beschreibt
[netbox-modeling.md](netbox-modeling.md). NetBox benennt Felder in snake_case, deshalb trägt
jedes Struct `rename_all = "None"`; `type` heisst in Rust `type_`. Die Regionen einer Site holt
`Region` rekursiv (`recurse = "5"`, sechs Ebenen). NetBox' Skalare `JSON` (die Custom Fields
und der Config Context, ein Struct für alle), `BigInt` und `Decimal` stehen in
`netbox/mod.rs`.

Die Antwort bleibt als `Topology.source` erhalten: Die Vorschau eines Helfers wendet ihren Plan
auf deren JSON an und liest es wieder ein ([Config-Helper](config-helper.md)). Darum
serialisiert jeder Typ zu dem, was er liest (Unions mit `__typename`), und ein Objekt, das die
Vorschau anlegt, braucht jedes Feld, auch die leeren als `null`: cynic nimmt ein fehlendes Feld
nicht als `None`.
