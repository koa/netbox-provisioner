# Automatisches Rollout

Was sich in NetBox ändert, erreicht die Geräte der automatischen Sites ohne Anwenden von Hand:
als ein [Gemeinsam anwenden](jobs.md#gemeinsam-anwenden) mit dem Internet als Bedingung, das
der Provisioner selbst startet (`backend::auto_rollout`). Was noch fehlt, steht im
[Konzept](concepts/auto-rollout.md) und als Aufgabe C20 in [todo.md](todo.md).

## Bereich

- Das Custom Field `auto_rollout` an der Site macht ihre RouterOS-Geräte automatisch; dasselbe
  Feld mit `false` an einem Gerät nimmt es aus ([Custom Fields](custom-fields.md#auto_rollout-site-und-device-boolean)).
- Nur mit `allow_apply`; ohne startet die Automatik nicht.
- **Einschalten:** Sieht eine Runde eine Site zum ersten Mal automatisch und hat eines ihrer
  Geräte einen Diff, geht die Site in den Fehlerzustand „nicht wie in NetBox“ mit diesen
  Geräten; angewendet wird bei ihr nichts. Sonst ginge mit der ersten Änderung alles mit, was
  sich vorher angesammelt hat. Der Admin sieht die Diffs („Abweichung von NetBox“) und hebt den
  Fehler auf, dann rollt die nächste Runde sie aus, oder er wendet von Hand an.

## Runden

Eine Runde (`auto_rollout::round`) liest NetBox neu und den Diff jedes automatischen Geräts
der Sites ohne Fehler, dazu den der manuellen Geräte daneben (`related`: das andere Ende eines
Kabels oder einer Funkstrecke, Controller und APs derselben WLAN-Gruppe, die Geräte mit einem
VLAN, das über VXLAN zu einem automatischen geht), vier Geräte zugleich, mit Tabellen frisch
ab dem Beginn der Runde. Der Diff ist immer der ganze Unterschied zu NetBox, also auch, was von
Hand auf ein Gerät kam: das geht mit der nächsten Runde zurück. Ob ein Diff neu ist, sagt sein
Fingerabdruck (`GeneratedCfg::fingerprint`: SHA-256 des Skripts und ob das Passwort oder das
WebFig-Zertifikat fehlt).

Wann eine Runde läuft:

- **Webhook:** NetBox meldet jede Änderung an `/netbox-webhook` (Event Rule, signiert mit
  `netbox_webhook_secret`, [Betrieb](deployment.md)). Die Runde beginnt, wenn 20 s lang keine
  weitere kam (`QUIET`): was ein Plan oder ein Admin nacheinander schreibt, geht zusammen.
- **Alle 30 min** (`EVERY`), damit eine Änderung von Hand spätestens dann zurückgeht.
- **Nach 5 min** (`RETRY`), wenn die letzte ein Gerät zurückstellte, das Internet fehlte, ein
  Gerät beschäftigt oder NetBox nicht lesbar war.
- Gleich nach dem Start (nach 20 s) und nach „Fehler aufheben“ oder „Freigeben“.

Entschieden wird ohne Gerät (`auto_rollout::decide`, getestet in `decide/test.rs`):

- Jedes automatische Gerät mit Diff geht in die Transaktion.
- **Nicht erreichbar:** Ein Gerät, dessen Diff sich nicht lesen liess, wird zurückgestellt,
  ebenso das andere Ende jeder seiner Funkstrecken, auch über eine Kette von Strecken: eine
  Änderung an nur einer Seite kann die Strecke trennen. Die übrigen gehen ohne sie.
- **Manuelles Gerät daneben:** Hat es einen anderen Diff als in der Runde zuvor (seine
  Grundlinie), hält es die ganze Runde an, kein teilweises Rollout. Es hält an, bis es von Hand
  angewendet ist (leerer Diff) oder ein Admin es freigibt; dann geht es mit den automatischen in
  dieselbe Transaktion. Einen Diff, den es schon hatte, behält es als Grundlinie: was ein
  Operator dort liegen liess, hält nichts an.

Die Transaktion ist ein Gemeinsam anwenden (`job::start_all`, `batch::apply` mit Internet),
im Journal „Vom automatischen Rollout gestartet“. Ist eines der Geräte mit einem anderen
Auftrag beschäftigt, startet keiner (`Busy`), die Runde kommt nach 5 min wieder. Erreicht eines
vorher das Internet nicht über IPv4 und IPv6, wird nichts angewendet (`NoInternet`); das ist
kein Fehlerzustand. Scheitert die Transaktion, gehen alle zurück, und jede Site mit einem
Gerät darin geht in den Fehlerzustand „gescheitert“ mit den Geräten, die scheiterten.

## Fehlerzustand

Pro Site: Eine Site im Fehler wird in den Runden übergangen, die anderen laufen weiter.
NetBox bleibt, wie es ist. „Fehler aufheben und ausrollen“ (`clearAutoRolloutError`, braucht
`allow_apply`) setzt sie auf bereit und startet gleich eine Runde, die ihre Diffs ausrollt.

Metriken an `/metrics`, für den Alarm im Alertmanager:

- `auto_rollout_site_error{site}`: 1, solange die Site im Fehler ist;
- `auto_rollout_held_back`: 1, wenn die letzte Runde von einem manuellen Gerät angehalten wurde;
- `auto_rollout_deferred_devices`: die Geräte, die die letzte Runde zurückstellte.

## Zustand

In `state_dir/auto_rollout.json` (`saved_job::state_file`), damit ein Neustart nichts
vergisst: der Zustand jeder automatischen Site (bereit, Fehler mit Grund, Geräten und
Zeitpunkt), die Grundlinien der manuellen Geräte, die freigegebenen Geräte und die letzte
Runde (Ergebnis, angewendete, zurückgestellte und anhaltende Geräte).

## Auf der Seite der Site

Die Karte „Automatisches Rollout“ (`components::site::auto_rollout`, nur bei einer Site mit
`auto_rollout`; `SiteAccess.autoRollout`) zeigt, ob die Site bereit ist oder im Fehler, die
letzte Runde, die zurückgestellten Geräte und die anhaltenden manuellen mit „Freigeben und mit
ausrollen“ (`releaseAutoRollout`). Sie abonniert den Zustand: jede Änderung daran meldet
`facts::Change::AutoRollout`.

## Im Lab geprüft (2026-11-17)

Site „Lab“ automatisch, lab-np2 (der Management-Proxy) ausgenommen. Die erste Runde stellte die
Site in den Fehler „nicht wie in NetBox“ (lab-wap, lab-crs226, lab-sxt-a/b: ND, www,
Multicast-Querier, WebFig-Zertifikat). Nach dem Aufheben rollte die Runde die vier gemeinsam
aus, Internet vorher und nachher über IPv4 und IPv6 erreicht. Ein Label an einem freien Port
von lab-hex in NetBox: 20 s nach dem Webhook die Runde, 53 s nach der Änderung war der
umbenannte Port auf dem Gerät; das Label gelöscht, ebenso zurück.
