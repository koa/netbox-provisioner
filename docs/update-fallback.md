# Fallback für RouterOS-Updates

Ein Update (`backend/src/device/update.rs`) soll ein Gerät nicht so zurücklassen, dass jemand vor
Ort hin muss. Dafür gibt es drei Wege zurück: eine zweite Partition mit dem alten RouterOS, die
Rettung über einen Nachbarn auf demselben Kabel und den Downgrade auf Knopfdruck. Ein Update
ganz ohne Weg zurück braucht eine Bestätigung.

## Was ein Update kaputt machen kann

1. **Das Gerät bootet nicht oder nicht fertig.** Kein Script auf dem Gerät läuft mehr, nur
   RouterBOOT kann noch etwas tun.
2. **Das Gerät bootet, ist aber nicht erreichbar.** Die Konfiguration wird beim Start migriert und
   verhält sich anders: Paket ersetzt, Interface umbenannt, OSPF kommt nicht hoch, eine
   Firewall-Regel greift neu. Das ist der häufige Fall.
3. **Das Gerät ist erreichbar, aber das Netz dahinter nicht** (eine OSPF-Nachbarschaft fehlt).
   Prüfte der Provisioner nur das Gerät, sähe das wie ein Erfolg aus.
4. **Firmware-Upgrade:** Dafür gibt es keinen Weg zurück aus RouterOS. Der Backup-Booter von
   RouterBOOT (Reset beim Einschalten) bleibt der Weg vor Ort.

**Erfolg** heisst deshalb: Das Gerät ist mit der erwarteten Version erreichbar, *und* die
OSPF-Nachbarn, mit denen es vor dem Update benachbart war, sind es wieder (`/routing/ospf/neighbor`
vor dem Update gelesen). Benachbart heisst `Full`, oder `TwoWay`, wenn DR und BDR zwei andere des
Segments sind: Zwei DROther bleiben in `TwoWay`, und nach dem Neustart wird neu gewählt, so dass
einer, der als DR `Full` war, dort landen kann (berghof-stall-haus, 7.23.8).

## Zwei Partitionen

Auf RouterBOARDs mit NAND hält eine zweite Partition das alte RouterOS samt Konfiguration
([Partitions](https://help.mikrotik.com/docs/spaces/ROS/pages/328103/Partitions)):

- Seit 7.20 ab 128 MiB Flash, mindestens 60 MiB pro Partition, also genau zwei auf 128 MiB. Im
  Lab haben lab-core, lab-sxt-a/b, lab-ap1 und lab-ap2 128 MiB; lab-np1, lab-np2 und lab-wap
  haben 16 MiB und keine `/partitions`. Eine CHR hat keine.
- `copy-to` klont das laufende RouterOS samt Konfiguration auf die andere Partition, `activate`
  wechselt die Partition, von der gebootet wird. Startet die aktive nicht, bootet RouterBOOT von
  selbst die nächste (`fallback-to=next`).
- Seit 7.17 muss der Device-Mode Partitionen erlauben (siehe [Device-Mode](#device-mode)).

**Einrichten:** Die Geräteseite zeigt die Partitionen; „In zwei Partitionen aufteilen“ fragt nach
und startet den Job `Repartition` (`backend/src/device/partitions.rs`, `/partitions repartition
partitions=2`). Er vergleicht danach Identität, Dateien und die Zahl der Partitionen. Auf lab-core
dauerte das rund 80 s; Konfiguration, Dateien und OSPF-Nachbarn blieben erhalten.

**Von einer Partition starten:** Jede Partition mit einem RouterOS, die gerade nicht läuft,
bietet „Von partN starten“ an (Job `BootPartition`: `activate` und Neustart).

### Ablauf eines Updates mit Fallback

`backend/src/device/update_fallback.rs` und `update.rs`:

1. `copy-to` auf die nicht aktive Partition: Sie hält danach die Version und die Konfiguration
   von vorher.
2. Ein Scheduler `netbox-provisioner-update-fallback` (`start-time=startup`, Frist
   `update_fallback_seconds`, Vorgabe 900). Sein Script ist nur `/partitions activate <alte>`
   und `/system reboot`. Er wird *nach* `copy-to` angelegt, damit die alte Partition ihn nicht
   mitbekommt.
3. Das Update auf der aktiven Partition, Schritt für Schritt (von RouterOS 6 erst auf 7).
4. **Mit der erwarteten Version zurück:** Der Provisioner entfernt den Scheduler, dann wartet er
   auf die OSPF-Nachbarn von vorher. Die alte Partition bleibt bis zum nächsten Update als
   Rückweg liegen.
5. **Nicht zurück:**
   - Bootet das Gerät nicht (Fall 1), startet RouterBOOT von selbst die alte Partition.
   - Bootet es, ist aber nicht erreichbar (Fall 2), schaltet nach Ablauf der Frist der Scheduler
     zurück.
   - Fehlen danach Nachbarn (Fall 3), endet der Job mit `OSPF_NEIGHBORS_MISSING`. Zurück geht
     es mit „Von partN starten“; der Provisioner löst das nicht selbst aus.

   Läuft das Gerät danach von der alten Partition, meldet der Job „Update zurückgenommen“
   (`UPDATE_ROLLED_BACK`). Er versucht es nicht noch einmal von selbst. Endet ein Update sonst
   mit einem Fehler, nimmt er den Scheduler weg, ausser das Gerät war nicht zurück oder hat gar
   nicht neu gestartet: Dann bleibt der Scheduler für die hochgeladenen Pakete, die beim
   nächsten Neustart installieren.

Die Frist des Schedulers ist länger als die Wartezeit des Provisioners (`REBOOT_TIMEOUT`, 10 min),
sonst nähme das Gerät ein langsames, aber gutes Update zurück. Ein Neustart des Provisioners
während des Updates führt es weiter ([Jobs und Gerätezustand](jobs.md)).

Im Lab geprüft: lab-core 7.23.8 → 7.24.5 (`copy-to` etwa 7 s, zurück nach rund 80 s, alle sechs
OSPF-Nachbarn sofort `Full`), „Von partN starten“ in beide Richtungen, und das Zurückschalten durch
den Scheduler auf lab-ap1 (Provisioner beendet, während das Gerät zum Installieren neu startete:
danach lief `part1` mit 7.23.8 ohne Diff, die ausgesetzten Watchdogs von lab-np1 waren durch das
Sicherheitsnetz wieder da).

## Rettung über den Nachbarn

Für Geräte ohne Partitionen, und wenn der Scheduler nicht von selbst greift: Der Provisioner
erreicht das Gerät nicht mehr über das geroutete Netz, aber über einen Nachbarn auf demselben
Kabel.

- **Über den Tunnel:** Kommt nur OSPF nicht hoch, haben die Ports meist noch ihre Adressen aus
  NetBox. `connect` fällt dann auf die Adresse am Kabel zurück und erreicht das Gerät über den
  SSH-Tunnel zum Nachbarn (`bootstrap::tunnel`), wie vor dem ersten Anwenden.
- **Über mac-telnet**, wenn auch die Adressen weg sind: Das ist die Ersteinrichtung
  (`device::bootstrap`). Die Geräteseite bietet sie an, sobald das Gerät nicht erreichbar ist.
  Nach dem Werkslogin (abgewiesen bei einem konfigurierten Gerät) meldet sie sich mit den
  Zugangsdaten aus der Konfiguration an, setzt die Adresse aus NetBox auf den Port, öffnet die
  API und legt eine Firewall-Regel für den Nachbarn an. Die CAP schaltet sie nur nach einem
  Werkslogin ab (Reset in den CAPs-Modus); ein konfigurierter AP bedient seine Clients weiter.
- **Dann** zeigt die Geräteseite, was los ist: die Interfaces mit ihren OSPF-Nachbarn und das Log
  des Geräts (`AccessibleDevice.log`, die letzten 30 Einträge ohne die Logins). Zurück geht es
  mit dem Downgrade oder „Von partN starten“, dann mit Anwenden.

Geprüft auf CHR (`three_node_ospf_chain_apply_via_entry`): `target` verliert seine Adresse am
Kabel, `a` rettet es über mac-telnet, danach antwortet es über einen Tunnel zu `a`.

### MAC-Server auf den Wartungs-Ports

mac-telnet kommt nur an Ports durch, die `/tool mac-server allowed-interface-list` erlaubt. Das
Anwenden entfernt jede Interface-Liste, die nicht aus NetBox kommt, auch die Werks-Liste `LAN`;
danach zeigte die Einstellung auf eine gelöschte Liste (`*2000011`). Die generierte Konfiguration
hat deshalb eine eigene Liste `maintenance` (`BaseDeviceDataTarget::setup_maintenance_ports`) für
den MAC-Server und die Neighbor Discovery. Darin stehen:

- Ports mit Kabel zu einem Gerät mit RouterOS in NetBox;
- Radios einer Funkstrecke (Wireless Link);
- alle freien Ethernet-Ports: solche, die NetBox nicht kennt, oder die dort weder Kabel noch
  Adresse, VLAN oder Bridge haben. Vor Ort kann man so ein Notebook an einen freien Stecker
  anschliessen und das Gerät wiederbeleben.

Kunden-Ports bleiben draussen, auch wenn sie in derselben Bridge stecken: Die Liste nimmt den Port
selbst, und RouterOS lässt mac-telnet auch an einem Bridge-Port zu (auf CHR 7.24.5 geprüft). Ein
Gerät, dessen Ports alle zu Kunden gehen, hat eine leere Liste und damit kein mac-telnet: lieber
gar keins als eins auf allen Ports.

## Downgrade

Auf der Geräteseite steht unter dem Update ein Feld für eine ältere Version, vorbelegt mit der,
die das Gerät vor einem Update dieses Laufs des Provisioners hatte (`RouterOsVersions.previous`).
„Downgrade“ fragt nach und startet den Job `Downgrade` (`update::downgrade`,
`downgradeRouteros`): die Pakete, die das Gerät hat, in der älteren Version hochladen wie beim
Update, dann `/system/package/downgrade`, das neu startet. Nur RouterOS 7 auf ein älteres
RouterOS 7 (`NO_DOWNGRADE`). Er kopiert nichts auf die andere Partition. Watchdogs aussetzen und
die OSPF-Nachbarn danach wie beim Update; die Firmware bleibt. Im Lab (lab-np2, 16 MiB) brauchte
der Neustart rund 4,5 min.

## Ohne Weg zurück

Der Rettungsweg hilft nicht, wenn der Provisioner keinen Nachbarn erreicht (beim Gerät des
Management-Proxys selbst) oder das Kabel zum Nachbarn das Problem ist. `DeviceAccess.updateNeighbors`
sind die Nachbarn der Ersteinrichtungs-Pläne (`bootstrap::plans`: ein Port mit IPv4, dessen Kabel
zu einem erreichbaren Gerät im selben Netz geht). Hat ein Gerät keine zwei Partitionen mit dem
laufenden RouterOS und keinen solchen Nachbarn, zeigt die Geräteseite „Kein Weg zurück nach einem
Update“; Update und „Pakete anpassen“ gehen erst, wenn „Trotzdem ohne Weg zurück aktualisieren“
angekreuzt ist (`updateRouteros(withoutFallback: true)`). Der Job prüft dasselbe, bevor er etwas
herunterlädt, und bricht sonst mit `NO_UPDATE_FALLBACK` ab; `UpdateWithoutFallback` nennt die
Nachbarn, über die es weitergeht.

## Device-Mode

`backend/src/device/device_mode.rs`
([Device-mode](https://help.mikrotik.com/docs/spaces/ROS/pages/93749258/Device-mode)):

- **Was der Provisioner braucht** (`device_mode::wanted`): `scheduler` überall (Fallback beim
  Anwenden; im Mode `home` aus), `socks` auf dem Gerät des Management-Proxys, `partitions`, wo
  `/partitions` mindestens 128 MiB meldet.
- **Geräteseite:** die Zeile „Device-Mode“ mit dem Mode und dem, was fehlt, samt Grund. „Device-Mode
  anpassen“ fragt zuerst nach, denn danach verlangt das Gerät eine Bestätigung vor Ort und startet
  neu, mit allem, was es über PoE versorgt.
- **Job `DeviceMode`** (`changeDeviceMode`): sendet `/system/device-mode/update` nur mit den
  fehlenden Features (`activation-timeout=10m`) und meldet `DeviceModeConfirm`: am Gerät den Knopf
  drücken oder den Strom unterbrechen. Er hält die Verbindung bis dahin offen, denn RouterOS bricht
  die Änderung ab, wenn sie schliesst. Danach wartet er, bis das Gerät mit den Features läuft, und
  prüft dabei das Ergebnis, nicht den Neustart. Abbrechen schliesst die Verbindung und lässt den
  Device-Mode, wie er war; läuft die Frist ab, endet er mit `DEVICE_MODE_NOT_CONFIRMED`.

Ein kurzer Druck auf den Reset-Knopf bestätigt (lab-core: zurück nach rund 70 s mit
`partitions=true`, Konfiguration und OSPF-Nachbarn da).

## Strom aus und ein

Wird ein Gerät von einem RouterOS-Gerät über PoE gespeist (`pse` am Port mit dem Kabel zu ihm),
bietet die Geräteseite „Strom aus und ein“ an (`backend/src/device/power_cycle.rs`), mit Rückfrage,
auch während der Device-Mode-Job auf die Bestätigung wartet. Ein Job des speisenden Geräts
schaltet PoE an diesem Port für 5 s aus (`/interface/ethernet/poe/power-cycle`; RouterOS schaltet
selbst wieder ein). Er wartet, solange das Gerät oder eines, das es weiter speist, ein Update hat,
und deren nächstes Update wartet auf ihn, bis das Gerät wieder antwortet. Der Provisioner löst das
nur auf Knopfdruck aus, nie von selbst.
