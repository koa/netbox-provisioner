# Updates im ganzen Netz

Ein Knopf für eine Auswahl von Sites (eine, mehrere, alle): „Alle Geräte auf `long-term`
bringen“. Der Provisioner plant die Reihenfolge, zeigt den Plan, führt ihn nach Bestätigung aus
und hält beim ersten Problem an. Pro Gerät läuft ein gewöhnlicher Update-Job; der Rollout
regelt, was *zwischen* den Geräten passiert. Die Seite ist `/rollout` „Netz aktualisieren“ (im
Site-Menü der Breadcrumb).

## Wer schneidet wen ab

Der Provisioner erreicht jedes Gerät über den Management-Proxy und dann geroutet durchs Netz.
Startet ein Gerät neu, sind alle Geräte, deren Weg nur über dieses Gerät führt, für diese Zeit
weg. Läuft auf einem davon gerade ein Upload oder das Warten auf den Neustart, bräche dessen
Update mit `NotBackAfterReboot` oder einem Upload-Fehler ab, obwohl es selbst in Ordnung ist.
Ein speisendes Gerät darf ausserdem nicht neu starten, während ein gespeistes installiert.

### Wegegraph

`device/dependency.rs` (`PowerChain::of`, `Paths`) baut ihn je Update einmal:

- **Knoten:** die Geräte, ausgehend von den Wurzeln des Management-Baums (das Gerät des
  Management-Proxys und die, die die Default-Route ankündigen). Ein Weg über eine Site
  ausserhalb der Auswahl zählt mit, ebenso einer über ein Gerät ohne OSPF (ein Switch, der
  einen Link trägt): Über den sagt die LSDB nichts.
- **Kanten:** Kabel und Funkbrücken (Wireless Links) aus NetBox, ohne die, die laut OSPF-LSDB
  gerade unten sind ([Netz-Ansicht](network-map.md), gleiche Datenquelle): Ein Weg, der in
  NetBox steht, aber tot ist, zählt nicht als Ausweichweg. Antwortet kein Router in 10 s,
  gelten alle Kanten aus NetBox. Ein Gerät, das die Wege aus NetBox gar nicht erreichen, hält
  niemanden auf.

### Regel beim Neustart

Ein Update hält vom Upload bis zum Ende eine Update-Lease, jeder Neustart eine Reboot-Lease,
alle unter einem Lock, damit sich zwei Entscheidungen nicht kreuzen. Das Update wartet
(`WaitingForDevice` mit Gerät und Grund `DeviceWait`):

- `SupplierRebooting`: ein Gerät, das es speist, startet neu; es beginnt, wenn das zurück ist;
- `ConsumerUpdating`: ein Gerät, das es speist, macht ein Update; es startet neu, wenn das
  fertig ist;
- `BehindRebooting`: ein neu startendes Gerät schneidet es ab;
- `BehindUpdating`: ein Gerät, das nur über dieses (und die gerade neu startenden) erreichbar
  ist, macht ein Update.

Die Prüfung ist exakt statt über eine Rangfolge: Bei redundanten Wegen (im Lab erreicht man
lab-sxt-a über lab-np1 *oder* über die Funkbrücke von lab-np2) dürfen beide Seiten parallel
laufen, solange nicht beide gleichzeitig neu starten. Die Leases leben im Prozess und halten
nur Neustarts zurück, die der Provisioner selbst schickt.

### Watchdogs der Speiser

Ein Speiser mit `/system/watchdog watch-address` startet von selbst neu, wenn die Adresse hinter
dem Gerät liegt, das gerade installiert. So hat berghof-burri dem wAP 60G burri-schopf mitten im
Update den Strom abgeschaltet: Danach fand RouterBOOT keinen Kernel mehr und fiel auf Netboot
zurück (BOOTP alle 40 s), erst Netinstall brachte ihn zurück. Die Geräteseite liest deshalb vor
dem Update die Watchdogs, die das Gerät betreffen (`DeviceAccess.watchdogs`), und warnt.
Gewählt, setzt das Update sie für seine Dauer aus und stellt sie danach wieder her, auch nach
einem Fehler oder Abbruch; ein Scheduler auf dem Speiser (`netbox-provisioner-watchdog`) stellt
sie nach 2 h selbst wieder her, falls der Provisioner nicht dazu kommt
([Watchdog](watchdog.md)).

## Plan

`rolloutPlan(siteIds, channel)` liest nur. Pro RouterOS-Gerät, parallel: erreichbar, Version
und Architektur, die Schritte zum Ziel, freier Speicher gegen die Pakete, der Weg zurück
([Update-Fallback](update-fallback.md)) und übrig Gebliebenes. Ausgelassen wird (`RolloutSkip`),
was aktuell ist (`UpToDate`), nicht antwortet (`Unreachable`), Reste eines Jobs hat
(`Leftovers`, bis der Benutzer das geklärt hat), weder eine zweite Partition noch einen
Nachbarn als Weg zurück hat (`NoFallback`) oder weniger freien Speicher als die Pakete (Grösse
per `HEAD` bei MikroTik, `NoSpace`). Freier Speicher heisst `free-hdd-space`, auf Geräten mit
16 MiB Flash aber `free-memory`: Dort liegt das Wurzelverzeichnis im RAM (eine Disk `flash`,
`progress::root_in_ram`), und der Upload belegt RAM (1og-sw2: 11.7 MiB Paket, 2.8 MiB Flash
frei, einzeln problemlos aktualisiert).

Die Reihenfolge (`order`):

1. **Pilot:** je Architektur (`arm`, `arm64`, `mipsbe`, `mmips`, …) ein Gerät, hinter dem auf
   den Wegen niemand hängt und das nichts über PoE speist, das am weitesten aussen. Neue Pakete
   fallen meist pro Architektur oder Board aus. Den Vorschlag kann der Benutzer im Plan ändern.
2. **Danach von aussen nach innen:** nach Abstand zur Wurzel absteigend (abseits der Wege
   zuerst, bei Gleichstand nach Namen). So wartet selten etwas auf die Regel, und ein Problem
   tritt zuerst an einem Gerät auf, hinter dem niemand hängt.
3. **WLAN:** Die generierte Konfiguration setzt am CAPsMAN `upgrade-policy=suggest-same-version`:
   Ein CAP mit anderer Version als der Controller verbindet sich weiter. Ein CAPsMAN-Controller
   kommt nach dem letzten seiner APs dran.

## Rollout

`startRollout(siteIds, channel, pilots)` startet einen Job `JobKind::Rollout` (über die
gewählten Sites, `JobSubject::Network`, einer zur Zeit, `device/rollout/run.rs`). Er plant noch
einmal und startet für jedes Gerät einen gewöhnlichen Update-Job mit ausgesetzten Watchdogs der
Speiser, sodass die Geräteseite das Update wie sonst zeigt und abbrechen kann. Ein Gerät mit
laufendem Job wartet im Rollout. Bis zu `PARALLEL` (4) laufen zugleich, aber nie zwei
verwandte: Ein Gerät startet nicht, solange eines läuft, das hinter ihm hängt, von ihm gespeist
wird oder es speist (`PlannedDevice::related`). Die Leases verhindern nur gefährliche
Neustarts, nicht gleichzeitige Updates; auf Berg 925 liefen switch-mimi und die drei APs
dahinter (zwei davon gespeist) als Piloten gleichzeitig, und ihre Watchdog-Aussetzungen kamen
sich in die Quere (2026-11-07).

- Nach den Piloten hält der Rollout an und wartet auf „Weiter“ (`continueRollout`).
- **Ein Gerät scheitert:** Der Rollout startet keine neuen Geräte mehr, laufende Updates enden
  normal. Die Seite zeigt, welches Gerät warum scheiterte (dasselbe `ErrorInfo` wie beim
  einzelnen Update). Nichts wird von selbst wiederholt; „Weiter“ lässt das Gerät, wie es ist,
  und nimmt die übrigen dran, oder der Benutzer beendet den Rollout.
- `skipRolloutDevice` lässt ein noch nicht gestartetes Gerät aus; `cancelJob` bricht auch die
  laufenden Updates ab, an ihren sicheren Stellen.
- **Neustart des Provisioners:** Der Rollout ist danach weg. Ein neuer überspringt, was schon
  auf der Zielversion ist.

Events: `RolloutPlanned`, `RolloutDevice` (bei jedem neuen Zustand des Update-Jobs, alle 2 s
nachgesehen), `RolloutPaused`, `RolloutContinued`, `RolloutDone`.

**Downloads:** Ein einzelnes Update lädt seine Pakete selbst. Während eines Rollouts teilen sich
die Updates einen Cache pro Version und Datei (`update::DownloadCache`), der auch das neueste
Release jedes Kanals hält. Der Rollout lädt alle Pakete des Plans nach dem Plan und vor dem
ersten Gerät herunter (`rollout::download_all`): Ein Update kann die Verbindung ins Internet
kappen (der Uplink-Router startet neu), und wng-926-sw1 (Berg 926) scheiterte so an der Frage
nach dem neuesten Release.

## Seite

`pages/rollout.rs`: Sites und Kanal wählen, den Plan ansehen (Tabelle: Gerät, Version jetzt →
Ziel, Architektur, Pakete mit Grösse und freiem Speicher, Fallback, Grund für die Position),
Piloten ändern, starten. Während er läuft, dieselbe Tabelle mit dem Stand pro Gerät: die Phase
seines Update-Jobs (Herunterladen, Hochladen, Neustart, …), „offline“, solange es selbst oder
ein Gerät, hinter dem es hängt oder das es speist, neu startet; „Ablauf“ klappt den Job des
Geräts unter der Tabelle auf; „Überspringen“ für die noch nicht gestarteten, bei einer Pause
„Weiter“ und „Rollout beenden“. Die Seite fragt die Events jede Sekunde ab und nimmt beim
Öffnen den neuesten Rollout auf (`rolloutJob`).

Die [Netz-Ansicht](network-map.md) zeigt einen laufenden Rollout als Ring um jeden Knoten, den
er aktualisiert (`NetworkState.rollout`, `device::rollout::status`): grau gestrichelt „kommt
noch dran“, blau „wird aktualisiert“, gestrichelt „offline“, grün „aktualisiert“, rot
„fehlgeschlagen“, im Popup als Zeile. Die Füllung bleibt die Erreichbarkeit (Z16). Im Browser
gegen das Lab mit eingesetzten Zuständen geprüft (2026-11-13; die Lab-Geräte sind alle
aktuell, ein Rollout hätte nichts zu tun).

## Tests

- Unit-Tests der Regel in `dependency.rs`: Kette, zwei Wege, Gerät abseits der Wege, PoE.
- CHR-Test `rollout_chain.rs` mit der Kette `entry -- a -- target` (`a` und `target` aus dem
  älteren Release, über mac-telnet eingerichtet; `target` nur durch einen Tunnel über `a`
  erreicht): Beide machen ein echtes Update zugleich, `a` zuerst gestartet. `a` wartet mit
  `BehindUpdating` auf `target`, `target` startet vor `a` neu, `a` erst nach dem Ende von
  `target`s Update; danach laufen beide auf dem aktuellen Release. Die Wege sind dort von Hand
  gegeben (`Paths::rooted`, `PowerChain::on_paths`), nicht aus der LSDB.
- Lab: ein Rollout auf eine Version, die die Geräte schon haben, nur mit Paketabgleich. Ein
  echter Versionswechsel erst, wenn ein neues `long-term` erscheint.

## Entschieden (2026-10-23)

- Den **Piloten schlägt der Provisioner vor**, im Plan änderbar.
- **Kein Wartungsfenster** vorerst: Start von Hand, unter Aufsicht.
- Ein Rollout gilt für **mehrere Sites** (frei gewählt), nicht nur für eine.
