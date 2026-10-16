# Konzept: Geräteseite

Status: Konzept, nicht umgesetzt. Offener Punkt „Device-Darstellung“ aus [pending.md](../pending.md).

## Heute

`pages/devices/show.rs` zeigt von oben nach unten: „Ziel ändern“ (aufklappbar), Fehler, laufender
Job, dann eine einzige Liste (`<dl>`) mit Name, Board, RouterOS samt Update-Knöpfen,
Device-Mode, Partitionen samt Knöpfen, und am Ende die Änderungen mit „Anwenden“. Was man
liest (Zustand), was man tun kann (Aktionen) und was passiert (Job) ist gemischt. Lange Zeilen
(Pakete, OSPF-Nachbarn im Job) laufen auf schmalen Bildschirmen über.

## Aufbau

Vier Bereiche, auf breiten Bildschirmen in zwei Spalten, auf dem Handy untereinander (PatternFly
`Grid` mit `Card`, schmal je volle Breite):

1. **Kopf** (volle Breite): Name, Site, Board, Erreichbarkeit („über Management-Proxy, Adresse
   …“ bzw. „nicht erreichbar“) und ein Zustands-Badge (in Ordnung / Änderungen offen /
   Befunde / Job läuft).
2. **Was läuft und was zu tun ist** (volle Breite, nur wenn nötig): laufender oder letzter Job,
   Liegengebliebenes auf dem Gerät ([Job-Konzept](job-polling.md)), Befunde des Geräts
   ([Config-Helper](config-helper.md)).
3. **Gerät** (linke Spalte): RouterOS (Version, Pakete, Kanal, „Aktualisieren“),
   Firmware, Device-Mode, Partitionen, „Neu starten“. Jede Karte hat ihre Knöpfe selbst; seltene
   oder gefährliche Aktionen (Partitionieren, Device-Mode, von Partition starten) stehen in einem
   Menü „Weitere Aktionen“ mit Bestätigung, nicht als eigene Knöpfe.
4. **Konfiguration** (rechte Spalte, auf dem Handy nach „Gerät“): Warnungen, das Skript
   (einklappbar, mit Zeilenzahl im Titel) und „Anwenden“. Ist das Skript leer: „Gerät entspricht
   NetBox“.

Lange Werte (Paketliste, Nachbarn) brechen um oder werden gekürzt mit „mehr“. Das Skript scrollt
in seiner Box waagrecht, die Seite selbst nie.

## „Ziel ändern“

Was es heute kann: eine andere Adresse als die Primary IP aus NetBox, und andere Zugangsdaten
(ein anderer benannter Eintrag oder Benutzer/Passwort ad hoc).

Wofür es noch gebraucht wird:

- **Andere Zugangsdaten.** Der Tenant hat kein `mikrotik_credentials`, oder das Gerät hat noch
  ein anderes Passwort (z.B. von Hand eingerichtet). Das kommt vor, solange die Daten so
  unvollständig sind.
- **Andere Adresse.** Die Primary IP in NetBox fehlt oder ist falsch (15 von 41 Geräten haben
  keine). Für neue Geräte ist das nicht mehr nötig, die erreicht die Ersteinrichtung über den
  Nachbarn.

Vorschlag: Nicht mehr als Bereich zuoberst auf jeder Seite, sondern nur dort, wo es hilft. In der
Meldung „Gerät nicht erreichbar“ (sie verweist heute schon darauf) als Formular, und im Kopf als
kleiner Link „Zugang …“. Ist ein anderes Ziel gewählt, zeigt der Kopf das deutlich an
(„Adresse von Hand: …“), damit niemand vergisst, dass er nicht über NetBox zugreift.

Sobald der [Config-Helper](config-helper.md) fehlende Primary IPs und Zugangsdaten am Tenant
behebt, lässt sich der Adress-Teil streichen. Die Zugangsdaten ad hoc bleiben für Geräte, deren
Passwort niemand im Provisioner hinterlegt hat.

## Reihenfolge

1. Aufteilung in Karten und das Raster, ohne die Inhalte zu ändern; danach Screenshots schmal und
   breit.
2. „Ziel ändern“ in die Meldung „nicht erreichbar“ und den Kopf.
3. Bereich 2, sobald es Liegengebliebenes und Befunde gibt.
