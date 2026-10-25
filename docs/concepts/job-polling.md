# Konzept: Jobs über Fehler und Neustarts hinweg verfolgen

Status: Schritte 1 bis 3 umgesetzt (Ende bei unbekanntem Job, Back-off, Job-Nummern pro Lauf,
Liegengebliebenes auf der Geräteseite), Schritt 4 offen; dessen Entwurf ist jetzt der Zustand
pro Gerät ([Gerätezustand](device-state.md)). Offener Punkt „Polling verbessern“ aus [pending.md](../pending.md).
Auslöser: „Wenn ich den Server neustarte hängt alles.“

## Was heute passiert

`components/job.rs` fragt jede Sekunde nach den neuen Events eines Jobs. Ein einzelner Fehler
(Server kurz weg, Token abgelaufen) wird angezeigt, und das Nachfragen geht weiter. Das ist in
Ordnung. Nach einem **Neustart des Servers** geht es dagegen schief:

1. **Der Job ist weg.** Jobs leben nur im Prozess (`job::JOBS`). Der neue Server antwortet auf
   `job(id)` mit `null`. Die Job-Ansicht zeigt dann `MissingData` und hört auf zu fragen, ruft
   aber `on_finished` nie auf. Die Geräteseite hält den Job deshalb weiter für laufend: Alle
   Knöpfe bleiben gesperrt, bis man die Seite neu lädt. Das ist das „hängt“.
2. **Die Nummern fangen wieder bei 1 an** (`Jobs::next`). Ein offenes Browserfenster fragt nach
   Job 3. Startet nach dem Neustart jemand drei Jobs, bekommt es die Events eines *anderen* Jobs,
   womöglich auf einem anderen Gerät.
3. **Das Gerät ist mitten im Vorgang stehen geblieben.** Was der Job auf dem Gerät vorbereitet
   hat, läuft weiter, ohne dass jemand es erfährt:
   - Anwenden: Der Fallback-Scheduler lädt `apply_fallback_seconds` (90 s) nach dem Ende des
     Skripts (spätestens 120 s später) das Backup und
     startet das Gerät neu, auch wenn die Änderung gut war.
   - Update: Der Scheduler `netbox-provisioner-update-fallback` aktiviert nach
     `update_fallback_seconds` (900 s) die alte Partition und startet neu. Das Gerät läuft danach
     wieder mit dem alten RouterOS.
   - Hochgeladene `.npk`-Dateien bleiben liegen und werden beim nächsten Neustart installiert.
   - Device-Mode: Die Änderung wartet auf die Bestätigung.

Punkt 3 ist der gefährliche: Fünfzehn Minuten nach einem Neustart des Servers fällt ein
erfolgreich aktualisiertes Gerät auf die alte Version zurück, und niemand weiss warum.

## Vorschlag

### Server

- **Job-Nummern über Neustarts eindeutig** (umgesetzt). Die Nummern eines Laufs beginnen bei
  1000 je Sekunde des Startzeitpunkts (modulo 1 000 000 Sekunden, passt in `u32` und GraphQL'
  `Int`). Eine Nummer aus einem früheren Lauf findet so keinen fremden Job, ausser nach mehr als
  1000 Jobs je Sekunde zwischen zwei Starts.
- **Antwort für einen Job aus einem früheren Lauf.** Statt `null` ein Job im Zustand `LOST`
  („Server neu gestartet“), sobald die Nummer zu einem früheren Start gehört. Mit den Events, wenn
  sie gespeichert sind (nächster Punkt).
- **Events speichern.** Jeder Job schreibt seine Events zusätzlich als JSON-Zeilen in ein
  Verzeichnis (`state_dir`, Vorgabe neben `config.yaml`). Nach dem Neustart liest der Server die
  Jobs, die beim Beenden noch liefen, als `LOST` mit allen Events ein. So sieht man, wie weit der
  Job gekommen ist: „Skript angewendet, Fallback scharf, dann nichts mehr“.
- **Geordnet beenden.** Bei SIGTERM nimmt der Server keine neuen Jobs mehr an. Laufende Jobs
  bricht er ab wie mit „Abbrechen“ (`Log::check`, an sicheren Stellen), wartet darauf bis zu einer
  Frist (Vorschlag 60 s) und schreibt ein Event „Server beendet“. Was nicht rechtzeitig fertig wird,
  kommt nach dem Start als `LOST` zurück.

### Frontend

- **Ein Ende für jeden Fall.** Bei `LOST`, bei `null` und bei `UNKNOWN_JOB` gilt der Job als vorbei:
  `on_finished` aufrufen, Geräteseite neu laden, Knöpfe frei.
- **Server weg.** Schlägt die Abfrage fehl, zeigt die Job-Ansicht „Server nicht erreichbar, frage
  weiter nach …“ (eine Zeile, nicht jede Sekunde eine neue Fehlermeldung) und fragt mit wachsendem
  Abstand weiter (1 s, 2 s, 5 s, höchstens 10 s). Der Knopf „Abbrechen“ bleibt; dazu kommt „Nicht
  mehr verfolgen“, das nur die Anzeige beendet.
- **Token abgelaufen** (`401`/`403`): einmal neu anmelden lassen, dann weiterfragen, statt bei
  jedem Versuch denselben Fehler zu zeigen.

### Was auf dem Gerät liegen geblieben ist

Die Geräteseite liest beim Laden ohnehin vom Gerät. Dazu kommt, was ein abgebrochener Job
zurücklässt:

- Scheduler `netbox-provisioner-*` (Apply- oder Update-Fallback), mit der verbleibenden Zeit;
  dazu `netbox-provisioner-watchdog`, der ausgesetzte Watchdogs nach 2 h selbst wieder setzt
  (Update, Anwenden mit Funkänderung, [Watchdog](../watchdog.md)), und das Skript
  `netbox-provisioner-apply`. Ein abgebrochenes gemeinsames Anwenden lässt das auf jedem Gerät
  des Batches zurück,
- `.npk`-Dateien im Root,
- eine Device-Mode-Änderung, die auf Bestätigung wartet.

Das zeigt die Seite als Warnung oben an, mit Knöpfen, die der Mensch auslöst, keine automatische
Reparatur:

- Apply-Fallback: „Fallback stoppen“ (die Änderung bleibt) oder „Jetzt zurück“ (Backup laden).
- Update-Fallback: „Fallback stoppen“ (das neue RouterOS bleibt), nachdem die Seite gezeigt hat,
  dass das Gerät mit dem neuen RouterOS läuft und die OSPF-Nachbarn da sind.
- Pakete: „Hochgeladene Pakete entfernen“.

Damit wird ein Neustart des Servers zu einem sichtbaren Zustand statt zu einer Überraschung eine
Viertelstunde später.

*Umgesetzt* (`device::leftovers`, Job `Cleanup` über `clearLeftover`), anders als oben in drei
Punkten:
- Das Skript `netbox-provisioner-apply` bleibt nach jedem Anwenden liegen, das nächste ersetzt
  es (im Lab auf allen Geräten). Ein Rest ist es nur, solange es läuft; dann steht ein Hinweis da,
  ohne Knopf, denn es endet von selbst.
- Eine Device-Mode-Änderung bleibt nie liegen: RouterOS bricht sie ab, wenn die Verbindung ihres
  Jobs schliesst.
- Der Eintrag des Sicherheitsnetzes der Watchdogs hat „Jetzt wiederherstellen“ (`watchdog::restore`
  mit dem, was der Eintrag setzen würde). Hält ihn der laufende Auftrag eines anderen Geräts (das
  Update des überwachten Geräts, ein Anwenden mit Funkänderung), ist er kein Rest: Die Seite zeigt
  ihn getrennt mit diesem Auftrag und ohne Knopf (`heldBy`), und `clearLeftover` lehnt ab
  (`LEFTOVER_HELD`), denn der Watchdog würde das Gerät neu starten, solange das andere weg ist.

„Jetzt zurück“ führt das Skript des Fallback-Eintrags aus (`/execute`: Eintrag weg, Backup laden)
und wartet, bis das Gerät zurück ist. Die verbleibende Zeit ist `next-run`, wie das Gerät es
meldet; nach einem Sprung seiner Uhr stimmt sie nicht, deshalb heisst sie „laut Gerät“. Geprüft
auf CHR (`leftovers_are_found_and_cleared`): Jeder Rest wird gefunden und weggeräumt, ein schon
weggeräumter ergibt `LEFTOVER_GONE`, und nach „Jetzt zurück“ hat das Gerät die Identity von
vorher.

## Reihenfolge

1. Frontend beendet den Job bei `null`/`UNKNOWN_JOB` und zeigt „Server nicht erreichbar“ mit
   Back-off. Klein, behebt das Hängen.
2. Job-Nummern mit Startzeitpunkt.
3. Liegengebliebenes auf dem Gerät anzeigen, mit den Knöpfen dazu.
4. Events speichern, `LOST`, geordnetes Beenden.

## Zu prüfen

Ob „hängt“ auch den Server selbst meint (z.B. dass das Beenden auf die laufenden Jobs oder auf
offene Verbindungen zu Geräten wartet), habe ich nicht nachgestellt. Im Lab lässt sich das prüfen:
während eines Jobs den Provisioner neu starten und dabei UI und Log beobachten.
