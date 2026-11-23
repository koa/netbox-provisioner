# Konzept: Automatisches Rollout, offene Teile

Status: zum Teil umgesetzt, beschrieben in [Automatisches Rollout](../auto-rollout.md):
Bereich pro Site, Runden auf Webhook und alle 30 min, eine Transaktion über `applyBatch` mit
dem Internet vorher und nachher, Zurückstellen nicht erreichbarer Geräte samt ihren
Funkpartnern, Anhalten durch manuelle Geräte mit Freigabe, Fehlerzustand pro Site, Zustand in
`state_dir`, Karte auf der Seite der Site, Metriken, die Vorbedingung (Loopbacks, OSPF-Verbindung,
Device-Mode mit `scheduler`), die Prüfung der Geräte ausserhalb nach dem Skript, Anhalten mit
Grund bei Warnungen des Anwendens und Richtfunk. Aufgabe C20 in [todo.md](../todo.md).
Voraussetzung für den [Self-Service](self-service.md).

Was noch fehlt:

## Journal nennt die Änderungen

Was von Hand auf ein automatisches Gerät kam, geht mit der nächsten Runde zurück, spätestens
nach 30 min. Damit nachzulesen ist, was überschrieben wurde, nennt der
[Journal-Eintrag](../protocol.md) eines automatischen Anwendens die Änderungen pro Tabelle
(heute nennt er nur, ob angewendet wurde). Welcher Teil von Hand kam, lässt sich nicht
verlässlich erkennen (ein früher gescheitertes Anwenden sieht gleich aus), darum wird es nicht
versucht.

## Wann automatisch, wann nicht

**Grundsatz:** Automatisch geht, was der Fallback sicher zurücknimmt, niemanden vor Ort
braucht und nichts nach aussen ankündigt. Ob eine Änderung den Weg des Provisioners zu einem
Gerät ändert, wird **nicht vorhergesagt**, sondern nach dem Skript **geprüft**: Den Weg aus
Diff und Topologie zu erkennen ist ungenau (Bridges, VLANs, die Topologie vor und nach der
Änderung) und hielte oft an, wo nichts passiert, gerade bei den Anwohnern, deren „Gerät
anschliessen“, „Anschluss ändern“ und „Gerät entfernen“ alle den Weg ändern. Ein umnummerierter
Link lief im Lab automatisch durch ([Self-Service](../self-service.md)): beide Enden im selben
Batch, der Zugang über den Nachbarn, der Fallback auf jedem Gerät.

Was nicht automatisch gehen soll, hält die Runde an wie heute ein manuelles Gerät mit
geändertem Diff: mit einem typisierten Grund pro Gerät (`HoldReason`), auf der Karte der Site
und im Stand pro Änderung der Anwohner; der Admin gibt frei (für genau diesen Diff, nach
Fingerabdruck; ein weiter geänderter hält wieder an) oder wendet von Hand an. Im Zweifel hält
sie an.

**Vorbedingung:** Die Automatik läuft nur, solange jedes RouterOS-Gerät der automatischen
Sites (ohne die ausgenommenen) in NetBox hat:

- eine Loopback-Adresse IPv4 (`/32`) und eine IPv6 (`/128`) auf `lo`;
- voraussichtlich eine IP-Verbindung zu jedem anderen dieser Geräte: Im Graphen der
  OSPF-Nachbarn aus NetBox (zwei RouterOS-Geräte mit je einem `ospf`-Interface im selben
  IPv4-Netz, wie im Management-Baum) erreicht es jedes andere, auch über Geräte ausserhalb des
  Bereichs. Eine Funkbrücke ohne RouterOS fasst der Provisioner nicht an; für die Automatik ist
  sie wie ein Kabel.

und auf dem Gerät einen Device-Mode mit `scheduler`: Über ihn lädt der Fallback des Anwendens
die Sicherung. Ohne ihn wendet der Provisioner heute trotzdem an (`FallbackUnavailable`, nur
eine Warnung), und ein Gerät, das sich abschneidet, bleibt weg, bis jemand vor Ort ist. Den
Scheduler schaltet nur eine Bestätigung am Gerät ein (Device-Mode auf der Geräteseite). Die
Runde liest den Device-Mode jedes erreichten Geräts mit seinem Diff; ein nicht erreichtes wird
ohnehin zurückgestellt.

Jedes gültige RouterOS-Gerät hat OSPF, auch CAPs, Switches und Brückengeräte, teils nur für
die Erreichbarkeit des Managements. Der Teil aus NetBox wird vor jeder Runde gerechnet, ohne ein
Gerät zu lesen, und fängt ab, was ein versehentliches Löschen in NetBox gefährlich macht: eine
gelöschte Loopback oder die letzte Verbindung eines Geräts. Ist sie verletzt, setzt die ganze
Automatik aus und die Karte zeigt den Fehlerzustand mit dem, was bei welchem Gerät fehlt.
Anders als die Fehlerzustände einer Site braucht er keinen Admin: Ist die Vorbedingung wieder
erfüllt, läuft die Automatik von selbst weiter. Was sie nicht abfängt, ist gelöschtes Kundenzeug
(Netze, Leases, WLANs): Es wird ausgerollt wie eine gewollte Änderung, die Geräte bleiben
erreichbar, und es lässt sich in NetBox wiederherstellen.

**Anhalten:**

1. **Umstecken:** Ein Kabel in NetBox zeigt auf einen anderen Port, als an dem das Gerät live
   gesehen wird (`neighbors::evaluate`, wie `seenUplinks` der Anwohner). Der Fallback nähme
   die Änderung zurück, bevor jemand umgesteckt hat. Löst sich von selbst: Sieht der Nachbar das
   Gerät am neuen Port, geht die nächste Runde.
2. **Nach aussen:** Der Diff ändert OSPF oder Adressen an einem Interface, dessen Gegenstelle
   kein RouterOS-Gerät in NetBox ist (Uplink, `internet`, `nat`), `propagate_default_route`,
   oder kündigt ein Netz ausserhalb der freigegebenen Bereiche an. Das Lab hat einmal eine
   Default-Route in die Produktion exportiert.
3. **Warnungen des Anwendens:** umgesetzt ([Automatisches Rollout](../auto-rollout.md#runden)).
4. **Richtfunk:** umgesetzt, vorerst immer (ohne Wartungsfenster).

**Prüfen nach dem Skript:** umgesetzt ([Gemeinsam anwenden](../jobs.md#gemeinsam-anwenden),
Geräte ausserhalb). Offen nur: wie lange die Anmeldungen bei grossen Sites in das Fenster des
Fallbacks passen (vier zugleich, je höchstens 25 s).

**Automatisch** geht alles andere, vor allem was Anwohner ändern: Kunden-Ports, VLANs und
Bridges, WLAN (SSID, Passwort, ein und aus), Kunden-Netze, DHCP, Leases, DNS, Freigaben,
Labels, PoE an Ports ohne Speisung, Syslog, WebFig mit Zertifikat, Watchdog-Ziele, ein neues
Passwort (`previous_password`), und Änderungen am Weg der Geräte.

**In Kauf genommen:** Was scheitert, fällt bis zum Fallback aus (2–3½ min, mit Funk bis
24 min), danach steht die Site im Fehlerzustand. Bricht eine Änderung am Gerät des
Management-Proxys den SOCKS-Zugang, erreicht der Provisioner keines mehr und alle gehen zurück:
sicher, aber das ganze Netz kurz weg; wer das nicht will, nimmt dieses Gerät aus.

**Offen:**

- Woher die freigegebenen Bereiche für 3 kommen. Vorschlag: die Prefixe mit Pool-Tag und die
  mit dem Scope einer Site in NetBox; alles andere hält an.
- Richtfunk (5) immer anhalten oder in einem Wartungsfenster erlauben (Z19: vorerst keins).
- Was ein Anwohner sieht, wenn seine Änderung anhält: nur den Grund oder auch, wen er fragt.
- Wie lange die Prüfung nach dem Skript braucht: ein Login an jedes Gerät der automatischen Sites muss in
  das Fenster des Fallbacks (90 s) passen, auch bei grossen Sites.

## Selbst angestossen und Stand pro Änderung

- Pro Änderung: wer sie ausgelöst hat (im Changelog von NetBox steht immer der Dienst-Benutzer
  des Provisioners; die Support-Ansicht will „koa (Admin) für Familie Meier“), welche Geräte sie
  betrifft und deren Stand (ausstehend, ausgerollt, wartet, zurückgestellt, gescheitert). Das
  Anwohner-UI (C21) zeigt diesen Stand pro Element. Heute kennt die Automatik nur Runden, keine
  Änderungen.
- Ein Admin, der im Fehlerzustand aufhebt, sieht vorher die Vorschau aller gesammelten
  Änderungen (heute: „Abweichung von NetBox“ der Site von Hand).

## Entschieden (2026-11-17)

- **Bereich:** Custom Field `auto_rollout` an der Site, mit einer Ausnahme pro Gerät.
- **Einschalten** nur mit leeren Diffs; was danach von Hand auf ein automatisches Gerät kommt
  und NetBox widerspricht, geht spätestens nach 30 min zurück, im Journal nachzulesen.
- **Ein manuelles Gerät betroffen:** kein teilweises Rollout, ein Admin gibt frei oder übernimmt den Diff als Stand (2026-11-17); erkannt an
  einem Diff, der sich seit der letzten Runde geändert hat.
- **Vorher nicht erreichbare Geräte** werden übergangen und bekommen die Änderung nachgeholt,
  ebenso das andere Ende ihrer Funkstrecken; nur ein Fehler während der Transaktion nimmt alles
  zurück.
- **Internet:** von jedem Gerät geprüft, nicht aus jedem Netz; IPv4 und IPv6 gleich wichtig.
- **Erkennen:** Webhook aus NetBox, Ruhezeit 20 s.

## Entschieden (2026-11-23)

- **Weg des Provisioners:** nicht vorhersagen und anhalten, sondern nach dem Skript prüfen.
  Die Gründe zum Anhalten (Umstecken, nach aussen, Warnungen, Richtfunk) wie oben.
- **Statt einer Mengenbremse** eine Vorbedingung aus NetBox (Loopback IPv4 und IPv6, eine
  IP-Verbindung zu allen anderen automatischen Geräten); verletzt setzt die Automatik aus, bis
  sie wieder erfüllt ist. Nach dem Skript antwortet jedes RouterOS-Gerät der automatischen
  Sites. Funkbrücken ohne RouterOS zählen wie Kabel.
- **Scheduler:** Ein Fallback beim Anwenden fehlt nur, wo der Device-Mode den Scheduler
  abschaltet; das ist ein Fehler. Ein Device-Mode mit `scheduler` auf jedem Gerät gehört zur
  Vorbedingung. Von Hand angewendet bleibt es die Warnung `FallbackUnavailable`: Dort
  entscheidet ein Mensch.
