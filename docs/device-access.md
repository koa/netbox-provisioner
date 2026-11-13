# Zugriff auf die Geräte

Wie der Provisioner mit den Geräten spricht: jeder Befehl ein Typ mit seiner Wirkung, jede
Verbindung hinter einer Fassade mit Rechten, und gespeichert nur, was ein Gerät antwortete. Gebaut
in B36 und C17 (2026-11-13), nach dem Konzept [Zugriff auf die Geräte](concepts/device-access.md);
was davon offen ist (Rahmen pro Operation, ganze Listen statt Filter), steht dort.

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

- **Menüs**: `print` liest als Modell des Menüs (`collect_resource`, `SingleResource::fetch`,
  `KeyedResource::fetch`), `monitor once` als sein Monitor-Modell (`collect_monitor_once`).
  Geändert wird über Mutationen: `calculate_create` (ohne die leeren Felder),
  `generate_single_update`/`calculate_update` (nur, was sich unterscheidet),
  `ResourceMutation::remove`, angewendet mit `apply_mutation`. Ein Menü, das gebraucht wird,
  bekommt sein Modell (`ros_model/*.txt`), mit einem Test, der eine echte Antwort eines Geräts
  liest (`src/model/test/device_reads.rs`).
- **Aktionen**: was kein `print`/`add`/`set`/`remove`/`move` eines Menüs ist, ist ein Typ im
  Katalog `action.rs`, mit seinen Parametern getypt: Reboot, Shutdown, Ping, die Monitore ohne
  Modell, Power-Cycle, Partitionen, Device-Mode, Pakete, Zertifikat-Import, `/execute`,
  `/tool/fetch`, ein Passwort, ein Skript-Job.
- **`RawCommand`**: ein Befehl ohne eigenen Typ, mit ausdrücklicher Wirkung. Er ist ein
  Code-Smell (Benutzer, 2026-11-13): jede Verwendung ist ein Befehl, der in den Katalog gehört.
  Vermehren sie sich, trägt das Konzept nicht mehr, und es ist zuerst neu zu überlegen. Es gibt
  zwei begründete Ausnahmen: `examples/print_device.rs` zeigt, was ein beliebiges Menü antwortet,
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
  `Error::Refused` mit dem Befehl. `collect_if_present` liest ein Menü, das ein Paket bringt,
  ohne das Paket als leer (`is_missing_menu`).

Funktionen, die nur lesen, nehmen `&impl Reads`, solche, die schreiben, `&impl Writes`: was
eine Funktion darf, prüft der Compiler.

Ein Login, der eben ging, dient wieder (Fact `Login`, 60 s); bevor er weitergegeben wird, prüft
ihn ein `/system/identity`, ausser die Verbindung hat vor weniger als 2 s geantwortet
(`LOGIN_CHECKED`): eine Seite holt den Login eines Geräts mehrmals (den Weg, dann den Login),
und jede Netzkarten-Abfrage schickte so zwei Identity-Prints an jedes Gerät.

## Basis-Fakten

Gespeichert wird nur, was gemessen wurde (Benutzer, 2026-11-13): die Antworten eines Geräts auf
einen Befehl, der nichts ändert (Print, Monitor einmal, Ping; Fact `Menu`), ein Login-Versuch
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
  Portname der Ersteinrichtung, wenn der Nachbar nicht antwortet); ein fehlendes Menü ist dann
  eine abgebrochene Verbindung.
- Antworten, die eine abgebrochene Verbindung sind, gelten nicht; ein fehlendes Menü (Paket
  nicht installiert) ist eine Antwort.
- Ungültig wird ein Menü durch jeden schreibenden API-Befehl an das Gerät, auf jeder Verbindung
  (`access::generation`: ein Leser verwirft die Menüs, wenn seit seinem letzten Lesen
  geschrieben wurde), durch das Ende jedes Jobs auf dem Gerät (`JobKind::invalidates`), durch
  „Neu laden“, „Nochmals lesen“ und „Erneut prüfen“, und durch das Alter, das der Leser zulässt.
  Ein Job holt an seinem Ende nichts und legt nichts ab; eine Änderung in NetBox verwirft nichts.
- Eine Rechnung über mehrere Menüs muss aushalten, dass sie verschieden alt sind: Lesen über
  Menüs war nie atomar.

Ein Befehl, der dasselbe Menü mit Filter fragt, ist ein eigener Fakt (`/ip/service` mit
`?name=` neben dem ganzen Print): mikrotik-model soll die ganze Liste lesen und selbst filtern
(offen, im Konzept).

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
Basis-Fakten 67 Befehle an 4 Geräte, zehn Menüs zwei- oder dreimal; mit ihnen 56, doppelt nur
noch `/ip/service` (mit und ohne Filter, siehe oben) und `/system/identity` (der Login und
seine Prüfung, ob die Verbindung noch lebt, wenn sie seit `LOGIN_CHECKED` nicht antwortete).

## Der CHR-Harness

Die CHR-Tests (`backend/tests/chr_network/`) erreichen ihre Knoten auf demselben Weg
(`node.rs`: Login, `add`, `set_single`, `set_keyed`, Ping, Skripte), auch beim Aufsetzen ihrer
Netze, wo sie den Betreiber spielen (Adressen, Routen, DHCP, Bridges, SOCKS). So prüfen sie die
getypten Befehle gegen echte Geräte mit. Fehlt einem Modell etwas, das der Harness braucht,
bekommt das Modell es (Benutzer, 2026-11-13); so kamen dazu: `/routing/ospf/interface` wie
lab-np2 es meldet, die IPv4-Adressliste, statische IPv6-Routen, Verzeichnisse in `/file`, der
Modus beim Device-Mode-Update und `/tool/fetch`.
