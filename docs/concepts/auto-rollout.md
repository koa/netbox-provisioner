# Konzept: Automatisches Rollout, offene Teile

Status: zum Teil umgesetzt, beschrieben in [Automatisches Rollout](../auto-rollout.md):
Bereich pro Site, Runden auf Webhook und alle 30 min, eine Transaktion über `applyBatch` mit
dem Internet vorher und nachher, Zurückstellen nicht erreichbarer Geräte samt ihren
Funkpartnern, Anhalten durch manuelle Geräte mit Freigabe, Fehlerzustand pro Site, Zustand in
`state_dir`, Karte auf der Seite der Site, Metriken. Aufgabe C20 in [todo.md](../todo.md).
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

**Anhalten:**

1. **Kein Fallback** (1a): Ein Mitglied, das keinen Fallback bekäme (Device-Mode ohne
   `scheduler`, heute `FallbackUnavailable`), hält an. Ohne Fallback bleibt ein Gerät, das sich
   abschneidet, weg, bis jemand vor Ort ist; heute wendet die Automatik es trotzdem an.
2. **Umstecken:** Ein Kabel in NetBox zeigt auf einen anderen Port, als an dem das Gerät live
   gesehen wird (`neighbors::evaluate`, wie `seenUplinks` der Anwohner). Der Fallback nähme
   die Änderung zurück, bevor jemand umgesteckt hat. Löst sich von selbst: Sieht der Nachbar das
   Gerät am neuen Port, geht die nächste Runde.
3. **Nach aussen:** Der Diff ändert OSPF oder Adressen an einem Interface, dessen Gegenstelle
   kein RouterOS-Gerät in NetBox ist (Uplink, `internet`, `nat`), `propagate_default_route`,
   oder kündigt ein Netz ausserhalb der freigegebenen Bereiche an. Das Lab hat einmal eine
   Default-Route in die Produktion exportiert.
4. **Warnungen des Anwendens:** `PoeCut`, `CapOffOutsideWlanGroup`, `ApiClientsOutside`: Nach
   dem Anwenden fehlt etwas, das die Prüfung nicht als Fehler sieht.
5. **Richtfunk:** Der Diff ändert ein Radio mit RF-Rolle (`PlannedApply::radio_wait` an einer
   Strecke, nicht an einem AP): bis zu 24 min ohne Verbindung (DFS), mit allem dahinter.
6. **Mengenbremse:** Eine Runde würde auf mehr als N Geräten etwas entfernen oder auf einem
   Gerät mehr als M Adressen oder statische Leases (`table_changes`, `removes_leases`): ein
   versehentliches Löschen in NetBox soll nicht das halbe Netz abräumen.

**Prüfen nach dem Skript** (1b): Bevor die Fallbacks weggehen, antwortet jedes RouterOS-Gerät,
das vor dem Batch erreichbar war und im Management-Baum hinter einem Mitglied hängt; sonst
gehen alle zurück (wie `BatchIncomplete`). Heute prüft der Batch nur seine Mitglieder: Bricht
der Weg zu einem Gerät ohne eigenen Diff dahinter (etwa über einen Switch ohne RouterOS),
meldet er Erfolg, und das Gerät bleibt weg, ohne Fallback. Das gilt für jedes Gemeinsam
anwenden, auch von Hand. Damit ist auch die Reihenfolge bei einer MTU-Änderung kein Fall für den
Menschen mehr: Beide Enden sind im selben Batch, und die Prüfung sieht, was dahinter hängt.

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
- N und M der Mengenbremse. Vorschlag: 3 Geräte, 5 Einträge.
- Richtfunk (5) immer anhalten oder in einem Wartungsfenster erlauben (Z19: vorerst keins).
- Was ein Anwohner sieht, wenn seine Änderung anhält: nur den Grund oder auch, wen er fragt.
- Wie lange die Prüfung 1b braucht: ein Ping an jedes Gerät hinter den Mitgliedern muss in das
  Fenster des Fallbacks (90 s) passen, auch bei einer grossen Site.

## Selbst angestossen und Stand pro Änderung

- Schreibt der Provisioner selbst (Anwohner, Helfer), stösst er die Runde direkt an, statt auf
  den Webhook zu warten.
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

- **Weg des Provisioners:** nicht vorhersagen und anhalten, sondern nach dem Skript prüfen
  (1b); dafür nie automatisch ohne Fallback (1a). Die übrigen Gründe zum Anhalten (Umstecken,
  nach aussen, Warnungen, Richtfunk, Mengenbremse) wie oben.
