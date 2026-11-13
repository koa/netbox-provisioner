# Zugriff auf die Geräte

Wie der Provisioner mit den Geräten spricht: jeder Befehl ein Typ mit seiner Wirkung, jede
Verbindung hinter einer Fassade mit Rechten. Gebaut in B36 (2026-11-13), nach dem Konzept
[Zugriff auf die Geräte](concepts/device-access.md); was davon offen ist (geteilte
Lese-Verbindung, Menü-Facts, Rahmen pro Operation), steht dort als C17.

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

## Der CHR-Harness

Die CHR-Tests (`backend/tests/chr_network/`) erreichen ihre Knoten auf demselben Weg
(`node.rs`: Login, `add`, `set_single`, `set_keyed`, Ping, Skripte), auch beim Aufsetzen ihrer
Netze, wo sie den Betreiber spielen (Adressen, Routen, DHCP, Bridges, SOCKS). So prüfen sie die
getypten Befehle gegen echte Geräte mit. Fehlt einem Modell etwas, das der Harness braucht,
bekommt das Modell es (Benutzer, 2026-11-13); so kamen dazu: `/routing/ospf/interface` wie
lab-np2 es meldet, die IPv4-Adressliste, statische IPv6-Routen, Verzeichnisse in `/file`, der
Modus beim Device-Mode-Update und `/tool/fetch`.
