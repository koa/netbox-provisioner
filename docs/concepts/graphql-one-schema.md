# Konzept: Ein GraphQL-Schema

Status: nicht umgesetzt, Aufgabe B40 in [todo.md](../todo.md).

## Heute

Drei Schemas an drei Endpunkten ([GraphQL](../graphql.md)), mit je eigenem cynic-Schema im
Frontend:

| Schema | Endpunkt | Rechte | Wurzelfelder |
|---|---|---|---|
| authenticated | `/graphql`, `/graphql/ws` | Admin am Eingang (`role::is_admin` in der Middleware von `binary`, im WebSocket noch einmal); dahinter Guards nur fürs Schreiben (`WritesAllowed`, `NetboxWritesAllowed`) | 14 Queries, 26 Mutationen, 3 Subscriptions |
| resident | `/graphql/resident` | Rolle am Eingang (`role::of`), im Kontext; Besitz in den Resolvern, `saveChange` prüft das Schreiben selbst | 6 Queries, 1 Mutation |
| anonymous | `/graphql_anonymous` | keine | `authentication` |

Die Anwohner-Wurzeln führen zu keinem Admin-Typ: Gemeinsam sind nur Verweise (`DeviceRef`,
`VlanRef`, …), Planschritte und `ErrorInfo`.

## Vorschlag

- **Ein Schema an `/graphql`** (und `/graphql/ws`), Token optional. Die Rolle bestimmt
  `role::of` einmal pro Anfrage und legt sie in den Kontext, ohne Token `Nobody`; auch der
  WebSocket.
- **Rolle an jedem Wurzelfeld:** ein Guard `Admin` oder `Resident` (Anwohner oder Admin),
  `authentication` ohne. Was hinter einem Wurzelfeld liegt, ist nur über dieses erreichbar und
  braucht keine eigene Prüfung. `WritesAllowed`, die Prüfung in `saveChange` und die eigene
  von `allow_apply` in `auto_rollout` werden ebenfalls Guards.
- **Besitz am Objekt:** Was einem Anwohner gehört, prüfen die Anwohner-Resolver wie heute.
- **Absicherung:** Ein vergessener Guard gäbe einem Anwohner Admin-Daten; heute verhindert das
  der Eingang für alles. Ein Test ruft jedes Wurzelfeld als Anwohner und ohne Token auf und
  erwartet die Ablehnung, ausser für eine ausdrückliche Liste. Wie er an die Argumente jedes
  Felds kommt, ist offen.
- **Frontend:** ein cynic-Schema statt drei (`frontend/build.rs`), die Wurzeltypen umbenannt
  (`QueryAuthenticated`, `QueryResident`, `QueryAnonymous` → `Query`).

## Was es bringt

- Ein Schema, ein Endpunkt, eine Rollenbestimmung statt `is_admin` am Eingang, im WebSocket und
  `role::of` für die Anwohner.
- Anwohner bekommen Subscriptions, die der Stand pro Änderung (Z51, C20) braucht.
- Die Regel „wer was darf, wird an der Grenze einmal entschieden“ (CLAUDE.md) wird wahr, als
  „Rolle am Wurzelfeld, Besitz am Objekt“; heute weicht der Code an vier Stellen ab.

## Aufwand

Mittel, das meiste mechanisch: die Wurzeln als `MergedObject`, rund 50 Guards, die Routen in
`binary/src/main.rs`, im Frontend rund 480 Angaben `schema = "…"`, die Tests der Anwohner mit
der neuen Erzeugung des Schemas, der Rechte-Test. Abfragen von aussen (Lab-Skripte, `local/`)
bekommen den neuen Pfad.
