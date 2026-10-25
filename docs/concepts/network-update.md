# Konzept: Updates im ganzen Netz

Status: Konzept, nicht umgesetzt. Aufgabe C10 in [todo.md](../todo.md).
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

Für PoE löst `device/power.rs` dasselbe Problem schon: ein speisendes Gerät startet erst neu,
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
3. **Parallel** bis zu `rollout_parallel` Geräte (Vorschlag: 4). Den Rest erledigen die Leases.
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
- **Neustart des Provisioners:** Die Leases leben im Prozess (`power.rs`). Der Rollout ist danach
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

## Downloads

Heute lädt jedes Update seine Pakete selbst herunter (`download`). Für einen Rollout kommt ein
Cache pro Version und Datei im Prozess dazu (`HashMap<(Version, Box<str>), Arc<[u8]>>`). Bei 30
Geräten mit 3 Architekturen sind das wenige Dutzend MB statt einige hundert.

## Backend

- `JobKind::Rollout`, ein Job pro Rollout (über die gewählten Sites), neben den bestehenden Jobs
  pro Gerät. Er startet für jedes
  Gerät einen normalen `JobKind::Update`-Job, sodass die Geräteseite das Update wie heute zeigt
  und abbrechen kann. Die Regel „ein Job pro Gerät“ bleibt, ein Gerät mit laufendem Job wartet im
  Rollout.
- Events des Rollouts: `RolloutPlanned { waves }`, `RolloutDevice { device, job, state }`,
  `RolloutPaused { reason }`, `RolloutDone { updated, skipped, failed }`.
- Mutationen: `planRollout(siteIds, channel)` (nur lesend, gibt den Plan zurück),
  `startRollout(siteIds, channel)`, `continueRollout(jobId)`, `skipRolloutDevice(jobId, deviceId)`,
  `cancelJob` wie bisher.
- `device/power.rs` wird zu `device/dependency.rs`: Leases wie heute, die Prüfung zusätzlich
  über den Wegegraphen.

## Frontend

Eine Seite „Netz aktualisieren“: Sites und Kanal wählen, Plan ansehen (Tabelle: Gerät, Version
jetzt → Ziel, Architektur, Fallback ja/nein, Welle, Grund für die Position), starten. Während er
läuft: dieselbe Tabelle mit Zustand pro Gerät und Link auf den Job des Geräts. Später die
[Netz-Ansicht](network-map.md) mit eingefärbten Knoten.

## Testen

- Unit-Tests für die Regel (Graph mit redundanten Wegen, Ketten, PoE und Weg kombiniert), wie
  `power.rs` sie für PoE hat.
- CHR: die 3-Knoten-Kette `entry -- a -- target` aus `ospf_chain.rs`. Ein Rollout muss `target`
  vor `a` neu starten, und `a` muss warten, solange `target` sein Update macht.
- Lab: ein Rollout auf eine Version, die die Geräte schon haben, nur mit Paketabgleich. Ein echter
  Versionswechsel erst, wenn ein neues `long-term` erscheint.

## Entschieden (2026-10-23)

- Den **Piloten schlägt der Provisioner vor**, im Plan änderbar.
- **Kein Wartungsfenster** vorerst: Start von Hand, unter Aufsicht.
- Ein Rollout gilt für **mehrere Sites** (frei gewählt), nicht nur für eine.
