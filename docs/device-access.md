# Zugriff auf die Geräte

Wie der Provisioner mit den Geräten spricht: jeder Befehl ein Typ mit seiner Wirkung, jede
Verbindung hinter einer Fassade mit Rechten, und gespeichert nur, was ein Gerät antwortete. Gebaut
in B36 und C17 (2026-11-13 bis 2026-11-16).

## Befehle: was sie bewirken (mikrotik-api)

Ein `Command` hat seinen Pfad, seine Attribute, den Typ, mit dem er seine Antworten liest
(`Reply`), und seine Wirkung als Typ (`Effect`):

- `Read`: liest Konfiguration oder Zustand, ohne Seiteneffekt (`print`).
- `Observe`: liest Live-Werte, ohne Seiteneffekt (`monitor once`, `/ping`, `/tool/fetch` ohne
  Ergebnis).
- `Write`: ändert das Gerät (`add`, `set`, `remove`, `move`, `/execute`, Reboot, Power-Cycle).
- `SessionBound`: wirkt nur, solange seine Verbindung lebt (`/system/device-mode/update`).

Eine `Connection` ist ein Login über einen Transport (TCP, SOCKS, SSH-Tunnel) und führt Befehle
jedes Typs aus (`Connection::run`): die Antworten gehen roh an den Befehl, der sie mit seinem
`Reply` liest. Sie verbindet nie neu; endet der Transport, antwortet jeder Befehl
`ConnectionClosed`. Jeder `Write`- und `SessionBound`-Befehl erhöht die `Generation` des Geräts,
die alle seine Verbindungen teilen (was vorher gelesen wurde, kann veraltet sein; C17 baut
darauf). Wie lange ein Befehl ohne Antwort bleiben darf, sagt er selbst
(`Command::reply_timeout`, sonst `REPLY_TIMEOUT`).

## Die Befehle (mikrotik-model)

- **Tabellen**: `print` liest als Modell der Tabelle (`collect_resource`, `SingleResource::fetch`,
  `KeyedResource::fetch`), `monitor once` als sein Monitor-Modell (`collect_monitor_once`).
  Geändert wird über Mutationen: `calculate_create` (ohne die leeren Felder),
  `generate_single_update`/`calculate_update` (nur, was sich unterscheidet),
  `ResourceMutation::remove`, angewendet mit `apply_mutation`. Eine Tabelle, die gebraucht wird,
  bekommt ihr Modell (`ros_model/*.txt`), mit einem Test, der eine echte Antwort eines Geräts
  liest (`src/model/test/device_reads.rs`).
- **Aktionen**: was kein `print`/`add`/`set`/`remove`/`move` einer Tabelle ist, ist ein Typ im
  Katalog `action.rs`, mit seinen Parametern getypt: Reboot, Shutdown, Ping, die Monitore ohne
  Modell, Power-Cycle, Partitionen, Device-Mode, Pakete, Zertifikat-Import, `/execute`,
  `/tool/fetch`, ein Passwort, ein Skript-Job.
- **`RawCommand`**: ein Befehl ohne eigenen Typ, mit ausdrücklicher Wirkung. Er ist ein
  Code-Smell (Benutzer, 2026-11-13): jede Verwendung ist ein Befehl, der in den Katalog gehört.
  Vermehren sie sich, trägt das Konzept nicht mehr, und es ist zuerst neu zu überlegen. Es gibt
  zwei begründete Ausnahmen: `examples/print_device.rs` zeigt, was eine beliebige Tabelle antwortet,
  auch eines ohne Modell, und der CHR-Test `wifi_country` prüft, dass RouterOS eine Schreibweise
  des Landes ablehnt, die das getypte Modell gar nicht schreiben kann.

Ein Befehl in einem Skript, das auf dem Gerät läuft (Apply, Fallback, Ersteinrichtung über
mac-telnet), ist kein API-Befehl und bleibt Text.

## Fassaden mit Rechten (Provisioner)

`device::access`:

- `Reader` liest nur (`Reads`: `Read` und `Observe`).
- `Session` darf auch schreiben (`Writes`) und Befehle halten, die an ihrer Verbindung hängen
  (`hold`). Der Provisioner nimmt sie nur mit der Sperre des Geräts (`job::start*`); seine Jobs
  öffnen ihre eigenen Verbindungen, weil deren Lebensdauer zählt (Device-Mode-Countdown, ein
  Apply schneidet seinen Weg ab, Reboot).
- `act` (Write) und `observe` schicken eine Aktion und geben ihre Zeilen; ein `!trap` wird
  `Error::Refused` mit dem Befehl. `collect_if_present` liest eine Tabelle, die ein Paket bringt,
  ohne das Paket als leer (`is_missing_table`).

Funktionen, die nur lesen, nehmen `&impl Reads`, solche, die schreiben, `&impl Writes`: was
eine Funktion darf, prüft der Compiler.

Ein Login, der eben ging, dient wieder (Fact `Login`, 60 s); bevor er weitergegeben wird, prüft
ihn ein `/system/identity`, ausser das Gerät hat auf der Verbindung vor weniger als 2 s
geantwortet (`LOGIN_CHECKED`; jede Antwort zählt, `Reader::answered`): eine Seite holt den Login
eines Geräts mehrmals (den Weg, dann den Login), und jede Netzkarten-Abfrage schickte so zwei
Identity-Prints an jedes Gerät. Prüfung und Identität eines neuen Logins gehen über die Facts
(`Since` der letzten Antwort): Prüfungen zugleich teilen sich eine, und die Seite nimmt die
Identität danach von dort.

## Basis-Fakten

Gespeichert wird nur, was gemessen wurde (Benutzer, 2026-11-13): die Antworten eines Geräts auf
einen Befehl, der nichts ändert (Print, Monitor einmal, Ping; Fact `Table`), ein Login-Versuch
(`Login`, auch keine Antwort oder eine Absage) und die neueste Version bei MikroTik
(`NewestVersion`). Alles darüber wird bei jedem Zugriff daraus gerechnet.

- mikrotik-api gibt die Antworten roh heraus (`Reply`, `Replies::unparsed`) und liest
  gespeicherte mit jedem Typ wieder (`Replies::kept`); `Prepared` ist ein Befehl als seine
  Wörter, ohne seinen Typ.
- Ein `Reader` mit Alter (`Reader::at(freshness)`) schickt einen Befehl nur, wenn kein
  passender Fakt da ist; sein Schlüssel sind die Zugangsdaten und die Wörter des Befehls (eigene
  Zugangsdaten sehen vielleicht weniger). `as_of` sagt, wie alt das Älteste ist, das er las:
  der Stand dessen, was daraus gerechnet wird. Ohne Alter liest er live (`Session::reader`, die
  Jobs, das Log, die PoE-Beobachtung). `Reader::stored` schaut nur nach (`knownDiff`, der
  Portname der Ersteinrichtung, wenn der Nachbar nicht antwortet); eine fehlende Tabelle ist dann
  eine abgebrochene Verbindung.
- Antworten, die eine abgebrochene Verbindung sind, gelten nicht; eine fehlende Tabelle (Paket
  nicht installiert) ist eine Antwort.
- Ungültig wird eine Tabelle durch jeden schreibenden API-Befehl an das Gerät, auf jeder Verbindung
  (`access::generation`: ein Leser verwirft die Tabellen, wenn seit seinem letzten Lesen
  geschrieben wurde), durch das Ende jedes Jobs auf dem Gerät (`JobKind::invalidates`), durch
  „Neu laden“, „Nochmals lesen“ und „Erneut prüfen“, und durch das Alter, das der Leser zulässt.
  Ein Job holt an seinem Ende nichts und legt nichts ab; eine Änderung in NetBox verwirft nichts.
- Eine Rechnung über mehrere Tabellen muss aushalten, dass sie verschieden alt sind: Lesen über
  Tabellen war nie atomar.

Ein Befehl, der dieselbe Tabelle mit Filter fragt, wäre ein eigener Fakt. Deshalb fragt
mikrotik-model auch eine Tabelle mit Schlüssel ganz (`PrintKeyed`) und lässt die Zeilen, die es
nicht will (anderer Schlüssel, `dynamic`, `builtin`), selbst weg: `/ip/service` ging so vorher
zweimal an jedes Gerät, mit `?name=` und ohne.

## Der Rahmen pro Operation

Innerhalb einer GraphQL-Operation wird jeder Datensatz einmal geladen, ausser es wird auf eine
Bedingung gewartet (Regel 2026-11-07). `operation` legt um jede Operation (Query, Mutation, jede
Runde einer Subscription) einen Rahmen (`tokio::task_local!`), der nichts selbst lädt: er merkt
sich, was die Operation zuerst bekam, und gibt ihr für den Rest genau das. Wird etwas mittendrin
ungültig, sieht die Operation weiter ihr eines Bild; die nächste Operation oder Runde liest neu,
die Subscription erfährt es über `facts::Change`.

- Topologie: `TopologyHolder::current` gibt die, die die Operation zuerst bekam
  (`operation::topology`); ausserhalb (Jobs) wird sie neu aus NetBox gelesen, sobald sie älter
  als 10 s ist.
- HTTP: die NetBox-REST-GETs eines Readers und die Paketgrössen bei MikroTik nach URL
  (`operation::netbox`, `operation::package_size`); ein Writer liest immer neu, er liest, was er
  gleich ändert.
- Facts: von jeder Tabelle und neuesten Version der erste Stand (`facts::Pins`, in den Metriken
  `pinned`); einer älter als ein `Since` wird neu gefragt, `invalidate` und „Neu laden“ werfen
  die des Geräts weg. Logins und Wege nicht: ein Login wird bei jedem Gebrauch geprüft.
- Wo eine Funktion eine Tabelle mehrfach braucht, liest sie über einen Facts-Reader
  (`Reader::at`; der Rollout-Plan mit `Freshness::Since` seinem Beginn): jede Tabelle einmal
  gefragt, keine Zeilen von Hand weitergereicht.

Eine Mutation ohne Job, die schreibt und danach liest, bräuchte frische Stände; es gibt keine
(`publishLayout` liest nach dem Schreiben nichts, die NetBox-Schreiber laufen als Jobs). Jobs
laufen ausserhalb jedes Rahmens und lesen neu.

Einen Pool von Lese-Verbindungen pro Gerät gibt es nicht: als solcher dient der Fact `Login`
(eine Verbindung, die vor höchstens 60 s ging). Gemessen 2026-11-16 im Lab: ein zweiter Browser
auf derselben Geräteseite schickt 1 Befehl statt 54, ein Login dient beiden. Ein Pool (eine
Verbindung pro Gerät mit Leerlaufzeit, ein Semaphor für die Reads) kommt erst, wenn eine Messung
ihn verlangt.

## Befehle pro Operation zählen

`operation` legt um jede GraphQL-Operation (Query, Mutation, jede Antwort einer
Subscription) einen Rahmen (`tokio::task_local!`); `Reader` trägt darin jeden Befehl mit Gerät
und Pfad ein, auch aus dem Task, den `facts::get` zum Fragen startet, aus `job::run` und den
Lesern des Rollout-Plans. Am Ende schreibt die Operation auf `debug` (`backend::operation`), wie
viele Befehle sie an wie viele Geräte schickte, und jeden Pfad, der mehr als einmal an
dasselbe Gerät ging; gezählt wird der Pfad, Pings an verschiedene Adressen sind also mehrere
`/ping`. Jobs laufen ausserhalb und zählen nicht. Messung im Lab
(`LOG_LEVEL=info,backend::operation=debug`): die Netzkarte schickt jeden Pfad pro Gerät einmal
(`NetworkMap` 1 Befehl, `NetworkMapDetails` 6 an 6 Geräte), der Rollout-Plan über die Site Lab
53 an 6 Geräte ohne Doppel. Die Geräteseite (`DeviceOverview`, lab-hex) schickte vor den
Basis-Fakten 67 Befehle an 4 Geräte, zehn Tabellen zwei- oder dreimal; mit ihnen 56, mit den
ganzen Listen 53, doppelt nur noch `/system/identity` (der Login und seine Prüfung, ob die
Verbindung noch lebt, wenn sie seit `LOGIN_CHECKED` nicht antwortete).

## Der CHR-Harness

Die CHR-Tests (`backend/tests/chr_network/`) erreichen ihre Knoten auf demselben Weg
(`node.rs`: Login, `add`, `set_single`, `set_keyed`, Ping, Skripte), auch beim Aufsetzen ihrer
Netze, wo sie den Betreiber spielen (Adressen, Routen, DHCP, Bridges, SOCKS). So prüfen sie die
getypten Befehle gegen echte Geräte mit. Fehlt einem Modell etwas, das der Harness braucht,
bekommt das Modell es (Benutzer, 2026-11-13); so kamen dazu: `/routing/ospf/interface` wie
lab-np2 es meldet, die IPv4-Adressliste, statische IPv6-Routen, Verzeichnisse in `/file`, der
Modus beim Device-Mode-Update und `/tool/fetch`.
