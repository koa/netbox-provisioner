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

## Nie automatisch

Diese Änderungen sollen auf einen Admin warten (die Runde hält an wie bei einem manuellen
Gerät), heute gehen sie mit:

- was den Weg des Provisioners zu einem Gerät ändert (Management-Adresse, MTU auf dem Weg);
- was nach aussen etwas ankündigt, das nicht in den freigegebenen Bereichen liegt (OSPF am
  Uplink; das Lab hat einmal eine Default-Route in die Produktion exportiert);
- ein anderer Anschluss eines Geräts, der Umstecken verlangt: Der Fallback nähme die Änderung
  zurück, bevor jemand umgesteckt hat;
- was eine Warnung des Anwendens auslöst (`ApplyWarning`, z.B. `PoeCut`: ein Port hört auf,
  ein Gerät zu speisen).

Offen ist, wie das im Diff erkannt wird.

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
