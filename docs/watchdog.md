# Watchdog auf jedem Router

Bei einem Crash oder einem hängenden Router soll niemand manuell eingreifen müssen. Dafür setzt
die generierte Konfiguration auf jedem Gerät Watchdogs, deren Ziele aus der Topologie kommen,
und Updates und Anwenden setzen sie aus, solange sie ein Gerät absichtlich abschneiden.

## Drei Mechanismen

**1. Hardware-Watchdog** (`/system/watchdog watchdog-timer=yes`): fängt harte Abstürze ab,
also einen hängenden Kernel oder ein Gerät, das gar nicht mehr reagiert. Er hat keine
Nebenwirkungen und ist auf jedem Gerät an, dazu `automatic-supout=yes` für die Diagnose
(`auto-send-supout=no`, es gibt keinen Mailversand). NetBox braucht dafür keine Daten.

**2. Ping-Watchdog** (`watch-address`): fängt „läuft, ist aber kaputt“ ab, etwa ein
hängendes Interface oder einen hängenden Routing-Stack. RouterOS startet neu, sobald die
Adresse `ping-timeout` lang nicht antwortet. Gemessen auf CHR 7.24.5: Es ist **ein**
Zeitraum ohne Antwort, nicht mehrere; mit 1 min und mit 3 min startete die CHR rund eine
Minute bzw. drei Minuten nach der letzten Antwort neu. `ping-timeout` geht von 10 s bis
10 min.

**3. PoE-Watchdog am Port** (`power-cycle-ping` an `/interface/ethernet`): schaltet einen
Port aus und wieder ein, wenn das gespeiste Gerät nicht mehr antwortet. Für eine hängende
Funkbrücke ist das besser als ein Neustart des Routers: Nur die Brücke startet neu, die
anderen Kunden am Router merken nichts.

## Was überwacht wird

Die Adresse kommt aus der Topologie, nicht von Hand (`topology::management`):

- **Wurzeln** sind die Geräte mit `propagate_default_route` und das Gerät des
  Management-Proxys (`management_proxy` in der `config.yaml`). Sie überwachen nichts.
- **Nachbarn** sind zwei RouterOS-Geräte, die beide ein `ospf`-Interface mit einer
  IPv4-Adresse im selben Netz haben. Ein Funkbrücken-Segment (Router-Port, beide Brücken,
  ferner Router-Port, alle im selben /29) macht alle vier zu Nachbarn.
- **Parent** eines Geräts ist sein Nachbar, der einer Wurzel am nächsten ist: nach Hops, dann
  einer, zu dem es ein Kabel hat, dann nach NetBox' Namensreihenfolge. Ohne die Kabelregel
  überwachte lab-sxt-b lab-np1 über die eigene Funkstrecke statt lab-np2 am Kabel, denn beide
  sind einen Hop von lab-core entfernt. Der Ping-Watchdog überwacht die primäre IPv4 des
  Parents (`device::ros::watchdog`).

Der direkte Nachbar statt eines fernen Ziels (z.B. der Core) beantwortet die Frage „bin ich
noch angebunden?“. Fällt ein fernes Ziel aus, starten alle Router im Netz gleichzeitig neu.
Fällt ein Parent aus, trifft es nur die Ebene darunter: Deren Neustart dauert etwa 1 min und
bleibt unter dem `ping-timeout` der nächsten Ebene, die Kette läuft nicht weiter.

| Feld | Wert | Grund |
|---|---|---|
| `ping-timeout` | 10 min | RouterOS' Maximum; der Neustart des Parents und ein Umweg über OSPF müssen hineinpassen |
| `ping-start-after-boot` | 15 min | Zeit für OSPF und Funkstrecken nach dem eigenen Neustart |
| `power-cycle-ping-timeout` | 5 min | länger als der Neustart des gespeisten Geräts |

**PoE-Regel.** Ein Gerät überwacht keinen Parent, den es nur über Ports erreicht, die das
Gerät dahinter speisen (alle verkabelten Ports des Interfaces, bei einer Bridge ihre Ports).
Ein Neustart würde dem gespeisten Gerät den Strom nehmen, vielleicht mitten in einem Update.
Das hat berghof-burri mit burri-schopf getan. Das betrifft auch den fernen Router einer
Funkbrücke, die er speist: Er teilt das Segment mit dem nahen Router, erreicht ihn aber nur
durch die eigene Brücke. Stattdessen überwacht sein PoE-Port die Brücke.

**PoE-Port.** Ein Port mit `poe_mode=pse`, der ein RouterOS-Gerät speist, pingt dessen
IPv4 am Kabel (am Port oder seiner Bridge, sonst die primäre). Diese setzt schon die
Ersteinrichtung, die Loopback erst das erste Anwenden. Die anderen Ports bekommen
`power-cycle-ping-enabled=no`. Boards ohne
Power-Cycling melden die Felder nicht und bleiben unberührt (`hw_facts`). Nicht-RouterOS-Geräte
(Kameras, fremde APs) werden nicht überwacht: Deren Updates kennt der Provisioner nicht.

**Nur was antwortet.** Vor dem Erzeugen pingt das Gerät jede Adresse, die ein Watchdog
überwachen soll (`DeviceCfg::generate_live`, 3 Pings). Antwortet eine nicht, bleibt der
Watchdog, wie er auf dem Gerät ist, und die Geräteseite warnt (`WatchTargetSilent`). Sonst
würde ein Router ein gespeistes Gerät, das gerade eingerichtet wird, alle 5 min stromlos
schalten, oder ein Gerät neu starten, dessen Parent noch nicht eingerichtet ist. Weil das
Behalten auch für die Prüfung nach dem Anwenden gilt, macht ein Ziel, das kurz nicht
antwortet, kein Rest-Diff.

## Updates

Ein Update schneidet das Gerät ab, solange es installiert. In dieser Zeit können drei
Watchdogs auslösen (`device::watchdog::watchers`):

1. **die Speiser:** ihr PoE-Port auf eine Adresse des Geräts (Strom weg mitten in der
   Installation) und ihr Ping-Watchdog, falls er auf etwas hinter dem Gerät zeigt (alte,
   von Hand gesetzte Watchdogs);
2. **die Kinder:** Geräte, deren Parent das Gerät ist;
3. **das Segment:** Geräte, die ihren Parent in einem Netz erreichen, in dem auch das Gerät
   ist (die Station einer Funkbrücke erreicht den nahen Router durch den AP). NetBox sagt
   nicht, welchen Weg ein Segment nimmt, so wird eher zu viel ausgesetzt.

Die Geräteseite liest vor dem Update, was diese Geräte tatsächlich überwachen
(`DeviceAccess.watchdogs`), und zeigt es als Warnung mit dem Häkchen „Watchdogs während des
Updates aussetzen“ (Standard an). Das Update setzt sie dann aus (`watch-address=none`,
`power-cycle-ping-enabled=no`) und stellt sie danach wieder her, auch nach einem Fehler oder
Abbruch. Ein Scheduler `netbox-provisioner-watchdog` auf jedem Gerät stellt sie nach 2 h selbst
wieder her, falls der Provisioner nicht dazu kommt. Ein liegengebliebener Scheduler ist, wo
`read` die Werte findet. Adresse und Frist des PoE-Pings setzt das Wiederherstellen mit, denn
ob RouterOS sie beim Ausschalten behält, ist nicht geprüft.

Ist ein Speiser nicht erreichbar oder lässt er sich nicht aussetzen, startet kein Update
(`WATCHDOG_NOT_SUSPENDED`). Bei jedem anderen Gerät geht das Update weiter
(`WatchdogLeft`): Schlimmstenfalls startet es einmal unnötig neu.

## Anwenden mit Funkänderung

Ein AP auf einem DFS-Kanal prüft nach einer Änderung 10 min oder länger auf Radar
(`RADIO_RESTART`), so lang wie `ping-timeout`. Ohne Pause startet die Station neu, und wer seinen
Parent über die Strecke erreicht; ihr Fallback-Scheduler zählt nach dem Neustart von vorne.
Ändert der Plan einen Funk, setzt das Anwenden deshalb ohne Nachfrage aus
(`device::radio_hold`):

- die `watch-address` des Geräts selbst; Plan und Prüfung danach behalten, was das Gerät dort
  hat, so macht die Pause kein Rest-Diff;
- die `watch-address` der Geräte, die von ihm abhängen (`depending_on`, wie beim Update).

Die PoE-Pings bleiben: Ein Port pingt nur das Gerät an seinem Kabel, und das bleibt erreichbar.
Dieselben Scheduler wie beim Update stellen nach 2 h wieder her. Erreicht der Provisioner die
Geräte wieder (oder ist nichts angewendet), stellt er sie selbst her, die eigene so, wie NetBox
sie hat, wenn die Adresse antwortet, sonst wie vorher. Solange der Fallback noch auslösen kann
(nicht erreicht, nach dem Skript abgebrochen), bleiben sie ausgesetzt (`WatchdogKept`): Die alte
Konfiguration kann ihren Funk wieder prüfen lassen. Beim gemeinsamen Anwenden hält jedes Gerät
mit Funkänderung seine eigene und die der Abhängigen ausserhalb des Batches, jedes nur einmal.
Ein Gerät im Batch ohne eigene Funkänderung, das von einem anderen abhängt, wird nicht
ausgesetzt.

## Grenzen

- **Neustart-Schleifen:** Fällt eine Verbindung dauerhaft aus (Funkstrecke bei Schnee), startet
  ein Gerät, das seinen Parent über sie erreicht, alle `ping-start-after-boot` + `ping-timeout` +
  Bootzeit neu, also etwa alle 26 min. Jedes Mal bricht sein lokales Netz kurz weg. Das ist
  bewusst so, es ist einfach und bleibt in RouterOS. Stören die Schleifen, ginge ein vom
  Provisioner verwaltetes Scheduler-Skript mit wachsenden Abständen statt `watch-address`
  (z.B. 20 min, 1 h, 6 h; der Zähler in einer Datei, ein erfolgreicher Ping setzt ihn zurück).
- **Neustart und Partitionswechsel** setzen nichts aus: Ein Neustart dauert etwa 1 min, kürzer
  als jede Frist.
- **L2-only-Funkbrücken** ohne Adresse und `ospf` sind keine Nachbarn im Baum. Ein Update von
  ihnen setzt nur ihre Speiser aus, nicht die Geräte jenseits der Funkstrecke.
- **Ein neu eingerichtetes Gerät** bekommt seinen Ping-Watchdog erst mit dem zweiten Anwenden:
  Beim ersten steht sein OSPF noch nicht, wenn das Anwenden prüft, ob der Parent antwortet
  (lab-wap, 2026-10-25).
- **Ersatz eines gespeisten Geräts:** Der Port des Speisers pingt weiter die Adresse des alten
  Geräts. Das neue im Werkszustand antwortet nicht und wird alle 5 min stromlos geschaltet, bis
  die Ersteinrichtung ihm die Adresse am Kabel gibt (E2).
