# Konzept: Updates im ganzen Netz

Status: umgesetzt: die Regel beim Neustart (`device/dependency.rs`), der Plan (`rolloutPlan`,
`device/rollout.rs`) und der Rollout (`startRollout`, `device/rollout/run.rs`), auf der Seite
`/rollout` „Netz aktualisieren“ (im Site-Menü der Breadcrumb). Offen: der CHR-Test der Kette
und die Netz-Ansicht mit eingefärbten Knoten. Aufgabe C10 in [todo.md](../todo.md).
Baut auf dem Server-Log (Events, Jobs) auf, das erledigt ist, und profitiert vom
[Update-Fallback](../update-fallback.md).

## Ziel

Ein Knopf für eine Auswahl von Sites (eine, mehrere, alle): „Alle Geräte auf `long-term`
bringen“.
Der Provisioner plant die Reihenfolge, zeigt den Plan, führt ihn nach Bestätigung aus und hält
beim ersten Problem an. Pro Gerät läuft das bestehende `update::update`; neu ist nur, was
*zwischen* den Geräten passiert.

## Die eigentliche Schwierigkeit: wer schneidet wen ab

Der Provisioner erreicht jedes Gerät über den Management-Proxy und dann geroutet durchs Netz.
Startet ein Gerät neu, sind alle Geräte, deren Weg nur über dieses Gerät führt, für diese Zeit
weg. Läuft auf einem davon gerade ein Upload oder das Warten auf den Neustart, bricht dessen
Update mit `NotBackAfterReboot` oder einem Upload-Fehler ab, obwohl es selbst in Ordnung ist.

Für PoE löste `device/power.rs` dasselbe Problem schon: ein speisendes Gerät startet erst neu,
wenn keines der gespeisten mehr ein Update macht. Der Vorschlag verallgemeinert das. Neben
„speist“ gibt es die Beziehung „liegt auf dem Weg zu“.

Die Leases halten nur Neustarts zurück, die der Provisioner selbst schickt. Ein Speiser mit
`/system/watchdog watch-address` startet von selbst neu, wenn die Adresse hinter dem Gerät liegt,
das gerade installiert. So hat berghof-burri dem wAP 60G burri-schopf mitten im Update den Strom
abgeschaltet: Danach fand RouterBOOT keinen Kernel mehr und fiel auf Netboot zurück (BOOTP alle
40 s), erst Netinstall brachte ihn zurück. Die Geräteseite liest deshalb vor dem Update die
Watchdogs der Speiser (`supplierWatchdogs`) und warnt. Gewählt, setzt das Update sie für seine
Dauer aus (`watch-address=none`, `device/watchdog.rs`) und stellt sie danach wieder her, auch
nach einem Fehler oder Abbruch. Ein Scheduler auf dem Speiser (`netbox-provisioner-watchdog`)
stellt die Adresse nach 2 h selbst wieder her, falls der Provisioner nicht dazu kommt; geprüft
auf CHR 7.24.5 (`watchdog_is_restored_or_restores_itself`).

### Wegegraph

- **Knoten:** die Geräte mit RouterOS der gewählten Sites, dazu der Management-Proxy als Wurzel.
  Ein Weg über eine Site ausserhalb der Auswahl zählt mit, deren Geräte werden aber nicht
  aktualisiert.
- **Kanten:** Kabel und Funkbrücken (Wireless Links) zwischen Ports mit OSPF, aus NetBox, wie
  sie `bootstrap::plans` und `PowerChain::of` schon durchlaufen. Dazu nur die Kanten, die laut
  OSPF gerade wirklich oben sind (siehe [Netz-Ansicht](network-map.md), gleiche Datenquelle). Ein
  Weg, der in NetBox steht, aber gerade tot ist, darf nicht als Ausweichweg zählen.

### Regel beim Neustart

Bevor ein Gerät X neu startet (`RebootLease`), prüft der Provisioner: Bleibt jedes Gerät, das
gerade ein Update macht (`UpdateLease`), von der Wurzel aus erreichbar, wenn man X und alle
anderen gerade neu startenden Geräte aus dem Graph nimmt? Wenn nicht, wartet X. Das Event
`WaitingForPower` wird zu `WaitingForDevice { device, reason }` mit den Gründen
`SupplierRebooting`, `ConsumerUpdating` (wie heute) und neu `BehindRebooting` bzw.
`BehindUpdating`.

Die Prüfung ist exakt statt über eine Vorab-Rangfolge: Bei redundanten Wegen (im Lab erreicht man
lab-sxt-a über lab-np1 *oder* über die Funkbrücke von lab-np2) dürfen beide Seiten parallel laufen,
solange nicht beide gleichzeitig neu starten. Die Prüfung steckt in `acquire` unter demselben
Lock wie die PoE-Leases, damit sich zwei Entscheidungen nicht kreuzen.

## Reihenfolge und Wellen

Die Regel oben verhindert Schaden, sie legt aber noch keine gute Reihenfolge fest. Der Plan:

1. **Pilot:** zuerst je ein Gerät pro Architektur (`arm`, `arm64`, `mipsbe`, `mmips`, …), und
   davon das, hinter dem nichts hängt. Neue Pakete fallen meist pro Architektur oder Board aus.
   Nach dem Pilot hält der Rollout an und wartet auf „Weiter“ im UI.
2. **Danach von aussen nach innen:** Geräte nach Abstand zur Wurzel absteigend sortiert (Blätter
   zuerst, Management-Proxy zuletzt). So wartet selten etwas auf die Regel, und ein Problem tritt
   zuerst an einem Gerät auf, hinter dem niemand hängt.
3. **Parallel** bis zu `PARALLEL` (4) Geräte, aber nie zwei verwandte: Ein Gerät startet nicht,
   solange eines läuft, das hinter ihm hängt, von ihm gespeist wird oder es speist
   (`PlannedDevice::related`), und umgekehrt. Die Leases verhindern nur gefährliche Neustarts,
   nicht gleichzeitige Updates; auf Berg 925 liefen switch-mimi und die drei APs dahinter (zwei
   davon gespeist) als Piloten gleichzeitig, ihre Watchdog-Aussetzungen kamen sich in die Quere
   (2026-11-07).
4. **WLAN:** Die generierte Konfiguration setzt am CAPsMAN `upgrade-policy=suggest-same-version`.
   Ein CAP mit anderer Version als der Controller verbindet sich also weiter. Controller und CAPs
   müssen deshalb nicht im Gleichschritt laufen. Der Controller kommt nach seinen CAPs dran.

## Abbruch und Fehler

- **Erstes fehlgeschlagenes Gerät:** Der Rollout startet keine neuen Geräte mehr. Laufende
  Updates enden normal. Das UI zeigt, welches Gerät warum fehlschlug (dasselbe `ErrorInfo` wie
  beim Einzel-Update). Kein automatisches Wiederholen und kein Überspringen; der Benutzer
  entscheidet: „Gerät überspringen und weiter“ oder „Rollout beenden“.
- **Abbrechen:** wie bei einem Einzel-Update, an den sicheren Stellen. Nicht gestartete Geräte
  bleiben, wie sie sind.
- **Neustart des Provisioners:** Die Leases leben im Prozess (`dependency.rs`). Der Rollout ist danach
  weg, aber er ist idempotent: `next_step` gibt für ein Gerät auf der Zielversion `None` zurück.
  Ein neuer Rollout überspringt also, was schon fertig ist.

## Vorab-Prüfung (vor dem Plan, nur lesend)

Pro Gerät, parallel:

- erreichbar, Version und Architektur (`read_state`), Ziel-Schritte (`next_step`);
- freier Speicher reicht für die Pakete (`free-hdd-space`; im Lab haben die 16-MB-Geräte 1–3 MB);
- Fallback möglich oder nicht (siehe [update-fallback.md](../update-fallback.md));
- übrig gebliebener Apply- oder Update-Fallback auf dem Gerät (Scheduler `netbox-provisioner-*`):
  Das Gerät wird nicht aktualisiert, bis der Benutzer das geklärt hat.

Nicht erreichbare Geräte stehen im Plan rot und werden ausgelassen. Der Rollout startet trotzdem,
wenn der Benutzer das bestätigt.

Umgesetzt lässt der Plan aus, was aktuell ist (`UpToDate`), nicht antwortet (`Unreachable`),
Reste eines Jobs hat (`Leftovers`), weder eine zweite Partition noch einen Nachbarn als Weg
zurück hat (`NoFallback`) oder weniger freien Speicher als die Pakete (Grösse per `HEAD` bei
MikroTik, `NoSpace`). Freier Speicher heisst `free-hdd-space`, auf Geräten mit 16 MiB Flash aber
`free-memory`: Dort liegt das Wurzelverzeichnis im RAM (eine Disk `flash`,
`progress::root_in_ram`), und der Upload belegt RAM (1og-sw2: 11.7 MiB Paket, 2.8 MiB Flash frei,
einzeln problemlos aktualisiert). Die Reihenfolge (`order`): je Architektur ein Pilot, von den Geräten,
hinter denen auf den Wegen niemand hängt und die nichts über PoE speisen, das am weitesten
aussen; dann die übrigen nach Abstand zur Wurzel absteigend (abseits der Wege zuerst, bei
Gleichstand nach Namen), ein CAPsMAN-Controller nach dem letzten seiner APs.

## Downloads

Ein einzelnes Update lädt seine Pakete selbst herunter (`download`). Solange ein Rollout läuft,
hält er einen Cache pro Version und Datei im Prozess (`update::DownloadCache`, je Datei ein
`OnceCell`, so lädt auch ein gleichzeitiges zweites Update nicht noch einmal). Bei 30 Geräten
mit 3 Architekturen sind das wenige Dutzend MB statt einige hundert; mit dem Rollout endet der
Cache. Der Rollout lädt alle Pakete des Plans nach dem Plan und vor dem ersten Gerät herunter
(`rollout::download_all`, die Seite zeigt „Lädt die Pakete aller Geräte herunter“), und der
Cache hält auch das neueste Release jedes Kanals: Ein Update kann die Verbindung ins Internet
kappen (der Uplink-Router startet neu), die übrigen brauchen dann keines mehr (wng-926-sw1,
Berg 926: sein Update scheiterte an der Frage nach dem neuesten Release).

## Backend

- `JobKind::Rollout`, ein Job pro Rollout (über die gewählten Sites, `JobSubject::Network`,
  einer zur Zeit), neben den Jobs pro Gerät. Er plant beim Start noch einmal und startet für
  jedes Gerät einen normalen `JobKind::Update`-Job, mit ausgesetzten Watchdogs der Speiser,
  sodass die Geräteseite das Update wie sonst zeigt und abbrechen kann. Die Regel „ein Job pro
  Gerät“ bleibt, ein Gerät mit laufendem Job wartet im Rollout (`Waiting`). Bis zu `PARALLEL` (4)
  zugleich; den Rest regeln die Leases.
- Events des Rollouts: `RolloutPlanned { plan }`, `RolloutDevice { device, job, state, error }`,
  `RolloutPaused { reason, device }`, `RolloutContinued`, `RolloutDone { updated, skipped,
  failed }`.
- Abfragen und Mutationen: `rolloutPlan(siteIds, channel)` (nur lesend), `rolloutJob` (der
  neueste), `startRollout(siteIds, channel, pilots)`, `continueRollout(jobId)`,
  `skipRolloutDevice(jobId, deviceId)`, `cancelJob` wie bisher (bricht auch die laufenden Updates
  ab). Nach einem fehlgeschlagenen Gerät heisst „Weiter“: es bleibt, wie es ist, die übrigen
  kommen dran.
- `device/power.rs` ist zu `device/dependency.rs` geworden: Leases wie zuvor, die Prüfung
  zusätzlich über den Wegegraphen (`Paths`, je Update einmal gebaut; ohne Antwort eines Routers
  in 10 s gelten alle Kanten aus NetBox). Ein Gerät, das die Wege aus NetBox gar nicht erreichen,
  hält niemanden auf.

## Frontend

Eine Seite „Netz aktualisieren“ (`pages/rollout.rs`): Sites und Kanal wählen, Plan ansehen
(Tabelle: Gerät, Version jetzt → Ziel, Architektur, Pakete mit Grösse und freiem Speicher,
Fallback, Grund für die Position), Piloten ändern, starten. Während er läuft: dieselbe Tabelle
mit dem Stand pro Gerät: die Phase seines Update-Jobs (Herunterladen, Hochladen, Neustart, …,
alle 2 s nachgeführt), „offline“, solange es selbst oder ein Gerät, hinter dem es hängt oder das
es speist, neu startet, „Ablauf“ klappt den Job des Geräts unter der Tabelle auf; „Überspringen“
für die noch nicht gestarteten, bei einer Pause „Weiter“ und „Rollout beenden“. Die Seite fragt die Events
jede Sekunde ab, wie die Abweichung einer Site, und nimmt beim Öffnen den neuesten Rollout auf.
Später die [Netz-Ansicht](network-map.md) mit eingefärbten Knoten.

## Testen

- Unit-Tests für die Regel (Graph mit redundanten Wegen, Ketten, PoE und Weg kombiniert), wie
  `dependency.rs` sie für PoE hat (erledigt: Kette, zwei Wege, Gerät abseits der Wege).
- CHR: die 3-Knoten-Kette `entry -- a -- target` aus `ospf_chain.rs`. Ein Rollout muss `target`
  vor `a` neu starten, und `a` muss warten, solange `target` sein Update macht.
- Lab: ein Rollout auf eine Version, die die Geräte schon haben, nur mit Paketabgleich. Ein echter
  Versionswechsel erst, wenn ein neues `long-term` erscheint.

## Entschieden (2026-10-23)

- Den **Piloten schlägt der Provisioner vor**, im Plan änderbar.
- **Kein Wartungsfenster** vorerst: Start von Hand, unter Aufsicht.
- Ein Rollout gilt für **mehrere Sites** (frei gewählt), nicht nur für eine.
