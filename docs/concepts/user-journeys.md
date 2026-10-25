# Konzept: User Journeys Rollout und Datenkorrektur

Status: Konzept. Die User Journeys „Rollout“ und „Datenkorrektur“ aus
[pending.md](../pending.md#user-journeys): was davon heute schon geht, und was fehlt. Wie
`pending.md` verlangt, kein Wizard entlang der Journeys, nur die Funktionen so verteilt, dass sie
gut gehen. Die übrigen Journeys haben eigene Konzepte: [Überwachung](monitoring.md),
[Firewall](firewall.md), [Self-Service](self-service.md).

## Rollout

> Neue Geräte im Feld verbinden und fortlaufend mit dem Mobiltelefon konfigurieren; sie kommen
> laufend online und lassen sich gleich testen.

| Schritt im Feld | heute | fehlt |
|---|---|---|
| Gerät in NetBox anlegen | Helfer „Neues Gerät“ am freien Port des Nachbarn ([Config-Helper](config-helper.md#neues-gerät)): Device Type nach dem gesehenen Board vorgewählt, mit Loopback und Verbindung | |
| Kabel zum Nachbarn | „Nachbarn lesen“ → Befund → „Kabel aus Befund übernehmen“ | |
| Loopback, Verbindung, Kunden-Netz, PoE, Funkbrücke | Helfer auf der Geräteseite | IP-Range des Kunden-Netzes |
| Erste Konfiguration | Ersteinrichtung über mac-telnet vom Nachbarn, dann Apply | |
| Funkstrecke beidseitig | „Gemeinsam anwenden“, Watchdogs währenddessen ausgesetzt | |
| RouterOS aktuell | Update mit Fallback (Partitionen) | [Updates im ganzen Netz](network-update.md) |
| Testen | Geräteseite: erreichbar, Konfiguration „entspricht NetBox“, OSPF-Nachbarn im Job | ein kurzer Test „Strecke ok“ (Ping/Bandbreite zum Nachbarn), Ansicht der Strecke |

**Telefon:** Die Geräteseite ist auf schmalen Bildschirmen einspaltig ([Geräteseite](../device-page.md)).
Offen ist, die Helfer-Formulare und die Vorschau dort zu prüfen (lange Skripte, Selects). Ein
Telefon im Feld hat oft schlechtes Netz: Jobs laufen im Server weiter, auch wenn die Seite
zwischendurch weg ist ([Jobs](../jobs.md)).

**Fehlt am meisten:** eine Ansicht „Was fehlt diesem Gerät noch“ (Befunde des Geräts mit den Helfern als
Knopf), die der Reihe nach durch Loopback, Verbindung usw. führt, ohne Wizard zu sein.

## Datenkorrektur

> NetBox und die Realität sind auseinandergelaufen. Abweichungen erkennen und korrigieren;
> anfangs alle Geräte einmal „syncen“.

| Abweichung | heute | fehlt |
|---|---|---|
| NetBox in sich (fehlende Loopback, Prefix, `ospf` an einem Ende, MTU …) | Befunde auf Site- und Geräteseite | Generatorfehler gesammelt als Befund, PoE-Prüfung, WLAN-Gruppen |
| Kabel | „Nachbarn lesen“ (MNDP/LLDP gegen NetBox) | |
| Adressen auf dem Gerät, die NetBox nicht kennt | Live-Prüfung der Interfaces: „Nur auf dem Gerät“ an der Zeile, mit „<Adresse> übernehmen“ ([Config-Helper](config-helper.md#adresse-übernehmen)); in der Übersicht der Site als entfernte Adressen | |
| Statische DHCP-Leases, die NetBox nicht kennt | Live-Prüfung der Interfaces: „Leases nur auf dem Gerät“ an der Zeile des Servers, mit „Lease <Adresse> übernehmen“ ([Config-Helper](config-helper.md#lease-übernehmen)); in der Übersicht der Site als entfernte statische Leases | |
| PoE | „Vom Gerät ablesen“ im Helfer PoE | als Befund für alle Geräte einer Site |
| Konfiguration allgemein | Diff auf der Geräteseite; „Abweichung von NetBox“ auf der Site-Seite: je Gerät die Änderungen eines Anwendens nach Menü | Diff in Worten |

**Erstmal alle syncen:** Für jedes Gerät gilt dieselbe Schleife: Diff ansehen, was davon ist
gewollt (dann NetBox korrigieren) und was falsch (dann anwenden). Was fehlt, ist der Überblick:

- **Site-Übersicht mit Diff-Grösse** je Gerät (umgesetzt, „Abweichung von NetBox“ auf der
  Site-Seite): ein Job der Site (`diffSite`, `device::site_diff`) liest die RouterOS-Geräte, vier
  zugleich, und meldet je Gerät, was ein Anwenden je Menü hinzufügt, ändert und entfernt (wie
  das Skript: Updates ohne Felder zählen nicht), oder warum es nicht lesbar war. Die Karte zeigt
  den Zwischenstand, die meisten Änderungen zuerst, hebt entfernte Adressen, OSPF und
  Bridge-Ports hervor und zählt, wer schon „entspricht NetBox“. Er sperrt kein Gerät, ein Apply
  daneben geht; ein zweiter Start gibt den laufenden zurück, und die Seite übernimmt beim
  Öffnen den letzten Job der Site.
- **Diff in Worten** zusätzlich zum Skript: „entfernt 2 Adressen, ändert die SSID“, die
  heiklen Teile hervorgehoben (Adressen, OSPF, Bridge-Ports).
- **Übernehmen statt wegwerfen:** Für Adressen und Kabel, die nur auf dem Gerät existieren, ein
  Knopf, der sie nach NetBox schreibt (Plan, Vorschau, Rückgängig wie die Helfer).

Historische Reste in echten Konfigurationen (deaktivierte oder verwaiste Einträge) sind nicht
automatisch Anforderungen; der Mensch entscheidet beim Übernehmen.

## Reihenfolge

1. Site-Übersicht mit Diff-Grösse (nur lesen, hilft sofort beim Syncen; umgesetzt).
2. Helfer „Neues Gerät“.
3. Befund und „Übernehmen“ für Adressen nur auf dem Gerät.
4. Diff in Worten.
5. Strecken-Test im Feld.

## Entschieden (2026-10-23)

- Die Abweichung aller Geräte einer Site rechnet ein **Job im Hintergrund**, die Seite zeigt den
  Zwischenstand.
