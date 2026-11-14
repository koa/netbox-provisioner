# Zugriff auf die Geräte

Stand 2026-11-13, besprochen mit dem Benutzer (Basis-Fakten und Ableitungen am selben Tag
nach der Messung). Gebaut und in [device-access.md](../device-access.md) beschrieben sind die
Befehle als Typen mit ihrer Wirkung, `Connection`, Aktionskatalog, `Reader`/`Session` (B36) und
die Basis-Fakten mit den Ableitungen und die ganzen Listen statt Filter (C17). Offen sind der
Rahmen pro Operation, der Pool und die Punkte unter „Offen“.

## Ziel

- Jeder Befehl an ein Gerät sagt durch seinen Typ, was er bewirkt; die Fassade, über die er
  geht, sagt, wer ihn schicken darf. Beides prüft der Compiler, nicht eine Konvention.
- Eine Verbindung (ein Login) dient beliebig vielen Fassaden: Rohes, Getyptes, Aktionen.
- Lesen wird geteilt: über parallele GraphQL-Operationen und viele Benutzer hinweg werden
  Verbindungen und Gelesenes weiterverwendet, solange sie ausreichen. Innerhalb einer Operation
  wird jeder Datensatz einmal geladen, ausser es wird auf eine Bedingung gewartet (Regel
  2026-11-07).
- Schreiben bleibt bei den Jobs, auf eigenen Verbindungen; was sie schreiben, macht das
  Gelesene des Geräts ungültig, auch über eine Verbindung, die offen bleibt.

## Heute

- Gespeichert sind nur Basis-Fakten (Menüs, Logins, neueste Version), die Ableitungen werden bei
  jedem Zugriff gerechnet ([device-access.md](../device-access.md#basis-fakten)).
- Als Pool dient der Fact `Login`, der eine Verbindung 60 s wieder gibt.
- Die Topologie (`TopologyHolder::topo_lock`) wird neu aus NetBox gelesen, sobald sie älter als
  10 s ist, auch mitten in einer Anfrage; die Resolver nehmen dafür von Hand einen
  `TopologyHolder::snapshot`.
- Wo eine Funktion ein Menü mehrfach brauchte, reicht sie die Zeilen noch von Hand weiter
  (`update::state_from`, `leftovers::read_with_files`, `progress::root_in_ram_of`,
  `wanted_packages_for`); mit den Basis-Fakten wäre das nicht mehr nötig.
- NetBox-REST und die Paketgrössen bei MikroTik werden pro Aufruf gelesen.

## Geteilte Lese-Verbindung (Pool)

Heute dient der Fact `Login` als Pool (eine Verbindung, die vor höchstens 60 s ging). Offen:

- Prozessweit, pro Geräte-ID eine Verbindung für Reads, die alle Operationen, Facts und Seiten
  teilen; ein neuer Login nur, wenn keine da ist oder `is_closed()`.
- Ungenutzt geschlossen nach einer Leerlaufzeit (60 s): die schwachen Geräte sollen nicht
  dauernd Sessions offen haben.
- Verworfen bei einer Topologie-Änderung des Geräts (Adresse, Weg, Credentials), am Ende jedes
  Jobs auf dem Gerät (Apply und Update können den Weg ändern, ein Reboot beendet sie) und bei
  `WrongDevice` oder einem Login-Fehler.
- Ein Semaphor pro Gerät hält die Reads eines Geräts nacheinander (schwache CPU und RAM), für
  alle Operationen gemeinsam.

## Der Rahmen pro Operation

Eine async-graphql-Extension legt um jede Ausführung (Query, Mutation, jede Runde einer
Subscription, wie `live::rounds` sie zählt) einen `operation::Scope` (`tokio::task_local!`).
Er hält nichts selbst und lädt nichts: er merkt sich, welchen Stand jedes Basis-Fakts und welche
Topologie die Operation zuerst bekam, und gibt ihr für den Rest genau den. Wird ein Fact
mittendrin ungültig, sieht die Operation weiter ihr eines Bild; die nächste Operation oder
Runde liest neu, die Subscription erfährt es über `facts::Change`.

- **Topologie**: `topo_lock` gibt im Rahmen die Topologie, die er beim ersten Zugriff nahm
  (heute `TopologyHolder::snapshot` von Hand in den Resolvern).
- **HTTP**: NetBox-REST-GETs und MikroTik-Metadaten nach URL im Rahmen.
- **Warten**: wer auf eine Bedingung wartet, liest frisch (`operation::fresh(fut)`). Fast alles
  Warten läuft in Jobs, die keinen Rahmen haben.
- **Facts**: `facts::get` startet das Fragen als eigenen Task; der Rahmen geht mit, damit zwei
  Facts derselben Operation dieselben Stände sehen.
- Eine Mutation ohne Job, die schreibt und danach liest, braucht `operation::fresh`; beim Umbau
  prüfen, ob es eine gibt (Kandidaten: `publishLayout`, die NetBox-Schreiber, die nur NetBox
  schreiben).

Danach fallen die Handarbeiten weg (`snapshot` in den Resolvern, `update::state_from`,
`leftovers::read_with_files`, `progress::root_in_ram_of`, `wanted_packages_for`).

## Offen

- Die Metriken `device_query_*` zählen nach Fact-Art, also alle Menüs als `menu`; nach Pfad wären
  es viele Reihen: Pfade zu Gruppen zusammenfassen oder nur Art (Print, Monitor, Ping) als Label.
- Die Rechenzeit der Ableitungen messen (der Diff jeder Geräteseite, der Site-Diff über alle
  Geräte), jetzt wo sie nicht mehr gespeichert sind.
- Der Login prüft eine Verbindung, die seit `LOGIN_CHECKED` nicht antwortete, mit einem
  `/system/identity`: auf der Geräteseite der einzige echte Doppel-Befehl. Zählte jede Antwort
  auf der Verbindung als Prüfung, fiele er weg.
- Nach einem Apply zeigt „Abweichung von NetBox“ „seit der Prüfung verändert“, bis jemand das
  Gerät liest (früher legte das Apply seinen Befund ab, `keep_in_line`).

## Schritte

1. Provisioner: Rahmen pro Operation mit Topologie und HTTP, die Handarbeiten weg.
2. Pool mit Leerlaufzeit und Semaphor pro Gerät, falls die Messung es noch braucht.

## Prüfen

- CHR-Tests wie bisher.
- Ein Zähler der Befehle pro Gerät und Pfad (Log auf `debug` oder Metrik): Geräteseite,
  Netzkarte und Rollout-Plan im Lab zeigen jeden Pfad pro Gerät und Operation einmal, und zwei
  Browser auf derselben Geräteseite einen Login und ein Lesen pro Menü.
