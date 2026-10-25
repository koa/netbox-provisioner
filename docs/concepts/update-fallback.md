# Konzept: Fallback für RouterOS-Updates

Status: Stufe 1 ist umgesetzt: Device-Mode anpassen (siehe unten), in zwei Partitionen
aufteilen und der Ablauf beim Update. Von Stufe 2 ist das Erreichen über den Tunnel zum Nachbarn
da (`connect` fällt auf die Adresse am Kabel zurück, wenn die Primary IP nicht antwortet, siehe
unten), ebenso der MAC-Server auf den Wartungs-Ports und die Bestätigung für ein Update ganz
ohne Weg zurück; offen sind mac-telnet zur Rettung und der Knopf für den Downgrade. Offener Punkt aus [pending.md](../pending.md#fallback) („Es braucht
noch einen Fallback Mechanismus für Updates“).

## Ausgangslage

Das Anwenden einer Konfiguration hat einen Fallback (`device/ros/fallback.rs`): Backup speichern,
Scheduler lädt es nach `apply_fallback_seconds`, der Provisioner nimmt beides weg, sobald er das
Gerät wieder erreicht. Ein Update (`device/update.rs`) hat nichts Vergleichbares. Es erkennt nur,
*dass* etwas schiefging (`NotBackAfterReboot`, `UnexpectedAfterReboot`); danach muss heute
jemand vor Ort hin.

## Was ein Update kaputt machen kann

1. **Das Gerät bootet nicht oder nicht fertig.** Kein Script auf dem Gerät läuft mehr, nur
   RouterBOOT kann noch etwas tun.
2. **Das Gerät bootet, ist aber nicht erreichbar.** Die Konfiguration wird beim Start migriert und
   verhält sich anders: Paket ersetzt, Interface umbenannt, OSPF kommt nicht hoch, eine
   Firewall-Regel greift neu. Das ist der häufige Fall.
3. **Das Gerät ist erreichbar, aber das Netz dahinter nicht** (eine OSPF-Nachbarschaft fehlt).
   Prüft der Provisioner nur das Gerät, sieht das wie ein Erfolg aus.
4. **Firmware-Upgrade:** Dafür gibt es keinen Weg zurück aus RouterOS. Der Backup-Booter von
   RouterBOOT (Reset beim Einschalten) bleibt der Weg vor Ort.

**Erfolg** heisst deshalb für alle Stufen: Das Gerät ist mit der erwarteten Version erreichbar,
*und* die OSPF-Nachbarn, mit denen es vor dem Update benachbart war, sind es wieder
(`/routing/ospf/neighbor` vor dem Update gelesen). Benachbart heisst `Full`, oder `TwoWay`, wenn
DR und BDR zwei andere des Segments sind: Zwei DROther bleiben in `TwoWay`, und nach dem Neustart
wird neu gewählt, so dass einer, der als DR `Full` war, dort landen kann (berghof-stall-haus,
7.23.8). Erst dann gilt das Update als gelungen.

## Stufe 1: zwei Partitionen

Auf Geräten mit genug NAND-Flash hält eine zweite Partition das alte RouterOS samt
Konfiguration. Geprüft nur lesend im Lab und in der MikroTik-Doku
([Partitions](https://help.mikrotik.com/docs/spaces/ROS/pages/328103/Partitions),
[Device-mode](https://help.mikrotik.com/docs/spaces/ROS/pages/93749258/Device-mode)):

- Partitionen gibt es auf RouterBOARDs mit NAND (ARM, ARM64, MIPS, …), seit 7.20 ab 128 MiB
  Flash, mindestens 60 MiB pro Partition, also genau zwei auf 128 MiB.
- Im Lab: lab-core, lab-sxt-a/b, lab-ap1, lab-ap2 haben 128 MiB und eine Partition `part0` mit
  `fallback-to=next`. lab-np1, lab-np2 und lab-wap haben 16 MiB, `/partitions` ist dort leer.
- `copy-to` klont das laufende RouterOS samt Konfiguration auf die andere Partition. `activate`
  wechselt die Partition, von der gebootet wird. Startet die aktive nicht, bootet RouterBOOT von
  selbst die nächste (`fallback-to=next`).
- Seit 7.17 muss der Device-Mode Partitionen erlauben (`partitions=yes`). Das braucht eine
  Bestätigung am Gerät. Im Lab stand überall `partitions=false`; auf lab-core ist es jetzt an.

### Einrichtung (einmal pro Gerät)

1. **Device-Mode anpassen.** *Umgesetzt*, siehe unten: Die Geräteseite zeigt, was fehlt, der Job
   wartet auf die Bestätigung am Gerät.
2. **Neu partitionieren** (`/partitions repartition partitions=2`). *Umgesetzt*
   (`backend/src/device/partitions.rs`): Die Geräteseite zeigt die Partitionen, „In zwei
   Partitionen aufteilen“ fragt nach, der Job `Repartition` vergleicht danach Identität, Dateien
   und die Zahl der Partitionen. Im Lab geprüft (lab-core, 2026-10-15): Nach rund 80 s war das
   Gerät mit `part0` (laufend, 64 MiB) und einer leeren `part1` (64 MiB) zurück. Konfiguration,
   alle Dateien und die OSPF-Nachbarn sind erhalten geblieben. Auf einer CHR geht das nicht, sie
   hat keine `/partitions`.

### Ablauf eines Updates

*Umgesetzt* in `backend/src/device/update_fallback.rs` und `update.rs`, wie unten beschrieben.
Die Frist ist `update_fallback_seconds`, Standard 900. Auf der Geräteseite bietet jede
Partition mit einem RouterOS, die gerade nicht läuft, „Von partN starten“ an (Job
`BootPartition`: `activate` und Neustart). Eine CHR hat keine Partitionen, deshalb lief die
Prüfung im Lab.

Im Lab geprüft (lab-core, 2026-10-15), über die UI:
- **Update 7.23.8 → 7.24.5:** `copy-to` auf `part1` dauerte über die API etwa 7 s und endete von
  selbst. Das Gerät kam nach rund 80 s mit 7.24.5 zurück. Der Scheduler wurde entfernt, alle 6
  OSPF-Nachbarn (v2 und v3) waren sofort wieder `Full`. Danach folgte das Firmware-Upgrade.
- **„Von part1 starten“:** lab-core lief nach rund 75 s mit 7.23.8 samt Konfiguration. Die
  OSPF-Nachbarn waren `Full`, und auf `part1` war kein Scheduler, weil er erst nach der Kopie
  angelegt wird.
- **„Von part0 starten“:** wieder 7.24.5, OSPF `Full`.
- **Zurückschalten durch den Scheduler** (lab-ap1, 2026-10-24): Partitionen per Kaltstart über
  PoE freigeschaltet, in zwei aufgeteilt (95 s, keine Datei verloren), dann das Update 7.23.8 →
  7.25 gestartet und den Provisioner beendet, als das Gerät zum Installieren neu startete. Das
  Gerät lief danach von `part1` mit 7.23.8 (aktiv), `part0` hielt 7.25. Konfiguration ohne Diff,
  die ausgesetzten Watchdogs von lab-np1 waren durch den Scheduler des Sicherheitsnetzes wieder
  da.
- **Nebenbei gefunden:** RouterOS 7.24.5 meldet in `/system/routerboard` kein
  `factory-firmware` mehr (dafür `minimum-firmware`). In mikrotik-model ist das Feld jetzt
  optional. Davor galt lab-core nach dem Update als nicht erreichbar.

1. `copy-to` auf die nicht aktive Partition: Sie hält danach die Version und die Konfiguration
   von vorher.
2. Einen Scheduler `netbox-provisioner-update-fallback` anlegen (`start-time=startup`, Frist
   `update_fallback_seconds`, Vorschlag 15 min). Sein Script ist nur
   `/partitions activate <alte>` und `/system reboot`, ohne Dateien. Er wird *nach* `copy-to`
   angelegt, damit die alte Partition ihn nicht mitbekommt.
3. Das Update wie heute auf der aktiven Partition.
4. **Erfolg:** Der Provisioner entfernt den Scheduler. Die alte Partition bleibt bis zum nächsten
   Update als Rückweg liegen; das nächste Update überschreibt sie.
5. **Kein Erfolg:**
   - Bootet das Gerät nicht (Fall 1), startet RouterBOOT von selbst die alte Partition.
   - Bootet es, ist aber nicht erreichbar (Fall 2), schaltet nach Ablauf der Frist der Scheduler
     zurück.
   - Ist es erreichbar, aber ohne seine Nachbarn (Fall 3), zeigt der Provisioner, was fehlt, und
     bietet „Zurück auf die alte Partition“ an (`activate` und Neustart). Er löst das nicht selbst
     aus.

   In jedem Fall wartet der Provisioner, bis das Gerät mit der alten Version zurück ist, und
   meldet „Update zurückgenommen“ mit dem Grund. Er versucht es nicht noch einmal von selbst.

Die Frist des Schedulers muss länger sein als die Wartezeit des Provisioners (`REBOOT_TIMEOUT`,
10 min), sonst nimmt das Gerät ein langsames, aber gutes Update zurück.

## Stufe 2: Rettung über den Nachbarn

Für Geräte ohne Partitionen (16 MiB) und als Werkzeug, wenn Stufe 1 nicht von selbst greift: Der
Provisioner erreicht das Gerät nicht mehr über das geroutete Netz, aber über einen Nachbarn auf
demselben Kabel.

- **Über den Tunnel:** Kommt nur OSPF nicht hoch, haben die Ports meist noch ihre Adressen aus
  NetBox. Der Provisioner erreicht das Gerät dann über den SSH-Tunnel zum Nachbarn
  (`bootstrap::tunnel`), wie vor dem ersten Apply.
- **Über mac-telnet**, wenn auch die Adressen weg sind: Vom Nachbarn aus eine Adresse auf das Kabel
  setzen (wie bei der Ersteinrichtung), danach weiter über den Tunnel.
- **Dann:** lesen und zeigen, was los ist (Log, OSPF-Nachbarn, Interfaces). Auf Knopfdruck die
  Pakete der alten Version hochladen und `/system package downgrade` aufrufen (16-MiB-Geräte),
  bzw. `activate` auf die alte Partition (Stufe 1). Die alten Pakete müssen dafür nicht vorher auf
  dem Gerät liegen; der Upload geht so wie beim Update selbst.

Grenzen: Das hilft nicht, wenn der Provisioner gar keinen Nachbarn erreicht (beim Gerät des
Management-Proxys selbst) und nicht, wenn das Kabel zum Nachbarn das Problem ist. Für diese Fälle
bleibt der Scheduler aus Stufe 1, und für 16-MiB-Geräte ohne erreichbaren Nachbarn gibt es keinen
Fallback. Das UI zeigt das vor dem Update an und verlangt eine Bestätigung.

*Umgesetzt* (Z23): `DeviceAccess.updateNeighbors` sind die Nachbarn der Ersteinrichtungs-Pläne
(`bootstrap::plans`: ein Port mit IPv4, dessen Kabel zu einem erreichbaren Gerät im selben Netz
geht), beim Gerät des Management-Proxys keine. Hat das Gerät keine zwei Partitionen mit dem
laufenden RouterOS und keinen solchen Nachbarn, zeigt die Geräteseite „Kein Weg zurück nach
einem Update“, und Update und „Pakete anpassen“ gehen erst, wenn „Trotzdem ohne Weg zurück
aktualisieren“ angekreuzt ist (`updateRouteros(withoutFallback: true)`). Der Job prüft dasselbe
selbst, bevor er etwas herunterlädt, und bricht sonst mit `NO_UPDATE_FALLBACK` ab;
`UpdateWithoutFallback` nennt die Nachbarn, über die es weitergeht. Geprüft auf CHR
(`update_older_chr_to_current`): ohne Nachbarn und Bestätigung abgelehnt, ohne dass ein Paket
geladen wurde, mit dem Nachbarn `entry` aktualisiert.

### Voraussetzung: MAC-Server

Gelesen auf vier konfigurierten Lab-Geräten:

| Gerät | `/tool mac-server allowed-interface-list` |
|---|---|
| lab-np1, lab-ap2 | `all` |
| lab-sxt-a, lab-ap1 | `*2000011`, ein Verweis auf eine gelöschte Liste (vermutlich die Werks-Liste `LAN`, die das Anwenden entfernt hat) |

Dort kam mac-telnet vermutlich nicht durch. Das Anwenden entfernt jede Interface-Liste, die
nicht aus NetBox kommt, also auch die Werks-Liste `LAN`.

*Umgesetzt* (`BaseDeviceDataTarget::setup_maintenance_ports`): Die generierte Konfiguration hat
eine Interface-Liste `maintenance` (früher `mac-server`) und setzt `/tool mac-server
allowed-interface-list=maintenance`, ebenso die Neighbor Discovery (`/ip/neighbor
discovery-settings discover-interface-list`, die sonst aus demselben Grund auf `*2000011` stand).
In der Liste stehen:
- Ports mit Kabel zu einem Gerät mit RouterOS in NetBox;
- Radios einer Funkstrecke (Wireless Link);
- alle freien Ethernet-Ports: solche, die NetBox nicht kennt, oder die dort weder Kabel noch
  Adresse, VLAN oder Bridge haben. Vor Ort kann man so ein Notebook an einen freien Stecker
  anschliessen und das Gerät wiederbeleben.

Kunden-Ports bleiben draussen, auch wenn sie in derselben Bridge stecken. Die Liste nimmt den
Port selbst, nicht die Bridge, und RouterOS lässt mac-telnet auch an einem Bridge-Port zu.
Geprüft auf CHR 7.24.5:

| Liste | Port in Bridge | mac-telnet |
|---|---|---|
| leer | nein | abgewiesen |
| `ether2` | nein | verbunden |
| `ether2` | ja | verbunden |
| nur die Bridge | ja | verbunden |

Ein Gerät, dessen Ports alle zu Kunden gehen, hat eine leere Liste und damit kein mac-telnet.
Das ist bewusst so: Lieber gar keins als eins auf allen Ports.

Auf der CHR geprüft (`three_node_ospf_chain_apply_via_entry`): Nach dem Apply enthält die Liste
die beiden verkabelten Ports und die freien Ports, und mac-telnet vom Nachbarn über das Kabel
verbindet.

## Umgesetzt: Device-Mode anpassen

`backend/src/device/device_mode.rs`, geprüft auf einer CHR (7.24.5, Test
`device_mode_change_confirmed_by_a_cold_boot`):

- **Was der Provisioner braucht** (`device_mode::wanted`):
  - `scheduler` überall (Fallback beim Anwenden; im Mode `home` aus);
  - `socks` auf dem Gerät des Management-Proxys;
  - `partitions`, wo `/partitions` mindestens 128 MiB meldet.
- **Geräteseite:** Zeile „Device-Mode“ mit dem Mode und dem, was fehlt, samt Grund. Der Knopf
  „Device-Mode anpassen“ fragt zuerst nach, denn danach verlangt das Gerät eine Bestätigung vor
  Ort und startet neu, mit allem, was es über PoE versorgt.
- **Job `DeviceMode`** (`changeDeviceMode`):
  - Er sendet `/system/device-mode/update` nur mit den fehlenden Features
    (`activation-timeout=10m`) und meldet `DeviceModeConfirm`: am Gerät den Knopf drücken oder
    den Strom unterbrechen.
  - Er hält die Verbindung bis dahin offen, denn RouterOS bricht die Änderung ab, wenn sie
    schliesst.
  - Danach wartet er, bis das Gerät mit den Features läuft, und prüft dabei das Ergebnis, nicht
    den Neustart.
  - Abbrechen schliesst die Verbindung und lässt den Device-Mode, wie er war.
  - Läuft die Frist ab, endet der Job mit `DEVICE_MODE_NOT_CONFIRMED`.
- **Im Lab geprüft** (lab-core, 2026-10-15): Ein kurzer Druck auf den Reset-Knopf bestätigt.
  Das Gerät startet neu (Verbindung weg nach etwa 50 s), ist nach rund 70 s mit
  `partitions=true` zurück, und die Konfiguration und alle OSPF-Nachbarn sind wieder da. Auf der
  CHR geht nur der Kaltstart.
- **Kaltstart über PoE** (*umgesetzt*, `backend/src/device/power_cycle.rs`): Wird das Gerät
  von einem RouterOS-Gerät über PoE gespeist (`pse` am Port mit dem Kabel zu ihm), bietet die
  Geräteseite „Strom aus und ein“ an, mit Rückfrage, auch während der Device-Mode-Job auf die
  Bestätigung wartet. Ein Job des speisenden Geräts schaltet PoE an diesem Port für 5 s aus
  (`/interface/ethernet/poe/power-cycle`). Er wartet, solange das Gerät oder eines, das es
  weiter speist, ein Update hat; deren nächstes Update wartet auf ihn. Im Lab geprüft (lab-ap1
  an lab-np1 `ether2`, 2026-10-24): Der Kaltstart bestätigte die Device-Mode-Änderung, nach
  rund 2 min war `partitions` an.

## Entschieden (2026-10-23)

- Den Kaltstart eines PoE-gespeisten Geräts löst der Provisioner **auf Knopfdruck** aus (PoE am
  speisenden Port aus und wieder ein, mit Bestätigung), nie von selbst.
- Ein Update **ganz ohne Fallback** (16 MiB, kein erreichbarer Nachbar) braucht eine
  ausdrückliche **Bestätigung**.
- Nach einem Update müssen die OSPF-Nachbarn wieder benachbart sein, **die es vorher waren**
  (`Full`, oder `TwoWay` zwischen zwei DROther).
