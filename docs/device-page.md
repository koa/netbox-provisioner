# Geräteseite

Die Seite eines Geräts (`frontend/src/pages/devices/show.rs`) trennt, was man liest (Zustand),
was man tun kann (Aktionen) und was gerade passiert (Job). Sie besteht aus Karten, auf breiten
Bildschirmen teils in zwei Spalten, auf dem Telefon untereinander. Lange Werte (Pakete, OSPF-Nachbarn)
brechen um; ein Skript scrollt in seiner Box waagrecht, die Seite selbst nie.

## Aufbau

Von oben nach unten:

1. **Sperre:** Ist Schreiben auf Geräte gesperrt (`allow_apply`), sagt ein Hinweis, wo es
   freigegeben wird.
2. **Kopf:** Name, Board, „Verbunden mit“ (Adresse, Nachbar, Identity; eine Warnung, wenn die
   Identity nicht der Name in NetBox ist) und „Zugang“ (siehe unten).
3. **Liegengebliebenes:** eine Warnung, wenn ein Auftrag etwas auf dem Gerät zurückgelassen hat
   (`components::leftovers`, `AccessibleDevice.leftovers`): Fallback des Anwendens mit „Fallback
   stoppen“ und „Jetzt zurück“, Fallback des Updates, ausgesetzte Watchdogs, ein noch laufendes
   Apply-Skript, hochgeladene Pakete. Jeder Rest hat seinen Knopf, nichts wird von selbst
   weggeräumt. Während ein Job auf dem Gerät läuft, sind die Knöpfe gesperrt; Watchdogs, die ein
   laufender Auftrag ausgesetzt hält, gelten nicht als liegengeblieben
   ([Jobs](jobs.md#was-auf-dem-gerät-liegen-bleibt)).
4. **Job:** der laufende oder letzte Job des Geräts mit seinem Zustand und seinen Events.
5. **Interfaces:** die [Interfaces nach Aufgabe](interface-view.md) mit Prüfungen, Befunden und
   den Helfern pro Port. Die Befunde des Geräts stehen dort an den Zeilen, die sie betreffen (die
   über das ganze Gerät beim Loopback), nicht in einer eigenen Liste.
6. **Gerät** und **Änderungen**, nebeneinander:
   - **Gerät:** RouterOS (Version, Pakete, Kanal, „Aktualisieren“, „Downgrade“, vor einem Update
     die Warnung zu den Watchdogs, die währenddessen auslösen könnten, siehe
     [Watchdog](watchdog.md#updates)), Device-Mode, Partitionen, „Neu starten“ und „Strom aus und
     ein“ ([Update-Fallback](update-fallback.md)). Seltene oder gefährliche Aktionen
     (Partitionieren, Device-Mode anpassen, von einer Partition starten) brauchen eine
     Bestätigung.
   - **Änderungen:** die Warnungen zum Anwenden, das Skript und „Anwenden“, daneben „Gemeinsam
     anwenden“ für die Geräte einer Funkstrecke. Ist das Skript leer: „Gerät entspricht NetBox“.
7. **Log des Geräts:** die Einträge aus `/log`, eingeklappt.

## Nicht erreichbar

Antwortet das Gerät nicht, steht statt der Änderungen die Meldung „Gerät nicht erreichbar“ mit
dem Grund. Darin:

- die **Ersteinrichtung** über mac-telnet vom Nachbarn, mit den MACs, die der Nachbar an seinem
  Port sieht, oder einer eingegebenen (auch zur Rettung eines Geräts, das nach einem Update seine
  Adresse verloren hat), oder warum es keine gibt;
- das Formular für einen anderen **Zugang**.

## Zugang

Statt der Primary IP aus NetBox eine andere Adresse, und statt der Zugangsdaten des Tenants
(`mikrotik_credentials`) ein anderer benannter Eintrag oder Benutzer und Passwort ad hoc. Das
braucht es, solange in NetBox eine Primary IP fehlt oder falsch ist, oder ein Gerät noch ein
anderes Passwort hat. Das Formular steht in der Meldung „nicht erreichbar“ und, für ein
erreichbares Gerät, aufklappbar im Kopf. Der Kopf zeigt immer, welcher Zugang gilt, damit niemand
vergisst, dass er nicht über NetBox zugreift.
