# Konzept: Zustand eines Geräts als State-Machine

Status: Entwurf, nichts davon umgesetzt. Beginn von D5 (Jobs über Neustarts, geordnetes Beenden);
ersetzt dort den Teil „Events speichern“ von [Jobs](job-polling.md#vorschlag).

## Warum

Was der Provisioner über ein Gerät weiss, liegt heute an vier Stellen, und keine davon überlebt
einen Neustart des Servers:

- **Jobs** im Speicher (`job::JOBS`): was läuft, mit seinen Events. Nach einem Neustart weg.
- **Leases** in `device::power` (Update, Neustart, Strom aus und ein): wer auf wen wartet. Im
  Speicher.
- **Rückgängig** eines NetBox-Schreibens (`NetboxWritten.before`): im Event des Jobs, also auch
  im Speicher.
- **Was auf dem Gerät von selbst auslöst** (Fallbacks, Sicherheitsnetz der Watchdogs, Pakete): nur auf
  dem Gerät. `device::leftovers` findet es erst, wenn jemand die Seite öffnet, und weiss nicht,
  welcher Auftrag es hinterlassen hat und warum.

Der Ablauf eines Auftrags steckt im Code des Jobs (`update.rs`, `apply`, `batch.rs`), als Folge
von `await`s. Wo er gerade steht, sieht man nur an seinen Events. Bricht der Prozess ab, ist auch
das weg.

Ziel: Pro Gerät **ein Zustand**, der ausdrücklich modelliert ist, gespeichert wird und den nur
klar benannte Übergänge ändern. Ein Job ist dann der Treiber, der Übergänge auslöst, nicht mehr
der Ort, an dem der Zustand lebt.

## Grundsätze

1. **Kein Eintrag heisst Ruhe.** Ein Gerät ohne gespeicherten Zustand ist im Grundzustand: Kein
   Auftrag läuft, auf dem Gerät wartet kein Selbstauslöser des Provisioners, nichts ist in NetBox
   geschrieben und noch nicht angewendet. Das ist der Normalfall für fast alle Geräte.
2. **Ein Eintrag bleibt, solange etwas offen ist.** Er wird erst gelöscht, wenn jeder Teilzustand (siehe
   unten) wieder im Grundzustand ist. Ein Auftrag, der fertig ist, aber einen Fallback auf dem
   Gerät hinterlässt, ist nicht fertig im Sinn des Zustands.
3. **Übergänge nur durch Eingaben.** Es gibt genau zwei Quellen:
   - **Bedienung**: ein Mensch startet, bricht ab oder räumt auf.
   - **Beobachtung**: der Provisioner liest am Gerät (oder an NetBox), was ist: erreichbar, Uptime
     gesunken, Marke des Skripts, Scheduler-Eintrag weg, Version, OSPF-Nachbarn.

   Dazu kommen zwei Ereignisse des Servers selbst: **Start** (gespeicherte Zustände einlesen) und
   **Ablauf einer Frist** (z.B. „nach 25 s keine Antwort“). Beide sind Beobachtungen der Zeit.
4. **Beobachten ist automatisch, Reparieren nicht.** Der Provisioner darf jederzeit lesen und den
   Zustand nachführen. Etwas am Gerät ändern darf nur ein Übergang, den ein Mensch ausgelöst hat,
   oder der laufende Auftrag, den ein Mensch gestartet hat (siehe
   [Offene Fragen](#offene-fragen) zum Weiterführen nach einem Neustart).
5. **Was geschehen ist, steht im Protokoll, nicht im Zustand.** Der Zustand sagt nur, was noch
   läuft oder noch wirken kann. Das Ende eines Auftrags (fertig, fehlgeschlagen, abgebrochen,
   unterbrochen), ein ausgelöster Fallback oder ein teilweise geschriebener Plan sind
   [Protokoll](#protokoll)-Einträge; der Zustand ist danach wieder im Grundzustand.
6. **Gespeichert wird bei jedem Übergang**, bevor die Wirkung beginnt: Erst „Fallback wird
   gesetzt“ schreiben, dann den Scheduler anlegen. So weiss der Server nach einem Absturz,
   dass dort vielleicht ein Scheduler liegt, und schaut nach.

## Teilzustände

Ein Gerät hat drei Dinge, die unabhängig voneinander laufen: Ein Selbstauslöser bleibt, wenn der
Auftrag, der ihn gesetzt hat, längst zu Ende ist, und liegt manchmal auf einem anderen Gerät; was
in NetBox geschrieben ist, wartet auf ein Anwenden, das vielleicht nie kommt. Statt eines Zustands
mit allen Kombinationen hat es **Teilzustände**, jeder mit einer eigenen kleinen State-Machine (in UML
heissen sie orthogonale Regionen). Der Eintrag ist leer, wenn alle Teilzustände im
Grundzustand sind.

| Teilzustand | Grundzustand | Worum es geht |
|---|---|---|
| **Auftrag** | `Frei` | was der Provisioner gerade auf dem Gerät tut |
| **Selbstauslöser** | leer | was auf dem Gerät von selbst handelt, wenn niemand es vorher wegnimmt: Apply-Fallback, Update-Fallback, Sicherheitsnetz der Watchdogs, hochgeladene Pakete |
| **NetBox** | `Gleich` | in NetBox geschrieben, aber noch nicht angewendet, mit den Daten für Rückgängig |

Die Teilzustände sprechen sich nur über Ereignisse ab: Der Auftrag „Anwenden“ legt im Zustand
`FallbackSetzen` einen Eintrag in **Selbstauslöser** an; der Auftrag endet, der Eintrag bleibt, bis eine
Beobachtung ihn wegnimmt.

Nicht unabhängig sind die Leases der PoE-Kette (heute `device::power`) und das gemeinsame
Anwenden: Sie folgen aus den Aufträgen mehrerer Geräte und sind deshalb **Bedingungen an
Übergängen**, kein eigener Teilzustand (siehe [Bedingungen über Geräte
hinweg](#bedingungen-über-geräte-hinweg)).

## Teilzustand „Auftrag“

Höchstens ein Auftrag pro Gerät (wie heute). Ein Auftrag einer Site (`SiteDiff`) liest nur und hat
keinen Gerätezustand. Jede Art (`JobKind`) ist eine **eigene State-Machine**: Die Zustände eines
Updates haben mit denen eines Anwendens nichts gemein. Gemeinsam ist nur der Rahmen.

### Rahmen

```mermaid
stateDiagram-v2
    [*] --> Frei
    Frei --> Art: Start (B)
    state Art {
        [*] --> erster_Zustand
        erster_Zustand --> letzter_Zustand: je Art, siehe unten
    }
    Art --> Frei: Ende (fertig, fehlgeschlagen, abgebrochen)
```

Für jede Art gilt:
- **Start** ist immer eine Bedienung (B). Was beim Start schon entschieden ist, gehört zum Start
  und ist kein Zustand: die Version eines Updates, „ohne Weg zurück bestätigt“ (Z23), die MAC der
  Ersteinrichtung.
- **Ende** ist einer von drei Ausgängen (fertig, fehlgeschlagen, abgebrochen) und schreibt einen
  Protokoll-Eintrag mit dem Zustand, in dem der Auftrag endete. Danach ist er `Frei`.
- **Abbrechen** (B) wirkt nur in Zuständen, die es erlauben (in den Diagrammen *abbrechbar*); in
  den anderen läuft der Zustand zu Ende, dann gilt der Abbruch.
- **Server gestartet**: Für jeden Zustand steht fest, was dann geschieht: *weiterführen* (der
  Zustand liest nur und nimmt höchstens den eigenen Selbstauslöser weg) oder *unterbrochen*
  (Ende mit Protokoll-Eintrag, die Selbstauslöser bleiben und werden beobachtet). Siehe
  [Offene Fragen](#offene-fragen), Frage 1.

Die Übergänge sind mit ihrer Quelle bezeichnet: **(B)** Bedienung, **(G)** Beobachtung am Gerät,
**(Z)** Frist abgelaufen. Eine Bedingung über andere Geräte steht in eckigen Klammern, z.B.
`[kein Speisendes startet neu]`.

### Anwenden

**Wofür:** bringt die Konfiguration des Geräts auf das, was der Generator aus NetBox erzeugt
(`applyCfg`). Was danach im Diff übrig bleibt, ist das, was nicht ging.

**Beachtet:**
- Das Skript läuft **auf dem Gerät** im Hintergrund, damit es fertig wird, auch wenn es den Weg
  der Verbindung ändert. Es hält beim ersten Fehler an.
- Vorher sichert das Gerät ein Backup und plant dessen Laden (Skript-Limit 120 s plus
  `apply_fallback_seconds`). Das Ende des Skripts zieht den Fallback auf die Frist ab dann vor.
- Gezählt wird nach der **Uptime** des Geräts, nicht nach seiner Uhr: Ein NTP-Sprung nach dem
  Anwenden liess einen Fallback sofort fällig aussehen (1og-sw1).
- Das Skript schreibt **Marken** unter `flash/` (Menü begonnen, Ende oder Fehlerzeile, Ping zum
  Provisioner weg/zurück). Sie überdauern den Neustart des Fallbacks und sagen, wo es abriss.
- Innerhalb von 10 s vor dem Auslösen wird der Fallback nicht mehr weggenommen (`DUE`).
- Eine Funkänderung verlängert die Frist um 24 min (DFS-Kanäle hören vorher auf Radar) und setzt
  die Watchdogs der abhängigen Geräte aus.
- Jede Prüfung nach dem Skript hat 25 s, damit eine kurz wieder offene Funkstrecke nicht
  verpasst wird.
- Sperrt der Device-Mode den Scheduler, geht es ohne Fallback (`FallbackUnavailable`).
- Ändert das Anwenden die WLANs eines Controllers, provisioniert es danach die CAPs neu.

**Bewusst nicht:**
- Ein gestartetes Skript wird nie angehalten, auch nicht beim Abbrechen; das entscheidet der
  Mensch.
- Ein Gerät ohne Marken (Datei nicht schreibbar) gilt nach 90 s als durch, wie vor den Marken.
- Die Reihenfolge eines Rollouts (vom Management-Proxy nach aussen, z.B. bei MTU) ist Sache des
  Menschen, nicht des Auftrags.

```mermaid
stateDiagram-v2
    [*] --> Planen
    Planen --> [*]: Diff leer → fertig
    Planen --> FallbackSetzen: Plan steht
    FallbackSetzen --> WatchdogsAussetzen: Backup und Scheduler da (G)
    FallbackSetzen --> WatchdogsAussetzen: Scheduler gesperrt (Device-Mode), ohne Fallback
    WatchdogsAussetzen --> SkriptStarten: nur bei Funkänderung, sonst direkt
    SkriptStarten --> SkriptLäuft: im Script-Job gesehen (G)
    SkriptStarten --> [*]: nach 10 s nicht gesehen (Z) → Fallback weg, fehlgeschlagen
    SkriptLäuft --> SkriptLäuft: Marke gelesen (G)
    SkriptLäuft --> FallbackWegnehmen: Marke Ende oder Fehler, erreicht (G)
    SkriptLäuft --> NichtErreicht: Frist bis Fallback (Z)
    SkriptLäuft --> ZurückGeladen: Fallback-Eintrag weg (G)
    FallbackWegnehmen --> Nachprüfen: entfernt (G)
    FallbackWegnehmen --> NichtErreicht: fällig in < 10 s, bleibt (G)
    NichtErreicht --> ZurückGeladen: erreicht, Eintrag weg, Uptime gesunken (G)
    NichtErreicht --> [*]: nicht zurück nach der Frist (Z) → fehlgeschlagen
    ZurückGeladen --> [*]: fertig, Protokoll „Fallback ausgelöst“ mit Marken
    Nachprüfen --> Nacharbeit: Diff gelesen (G)
    Nacharbeit --> [*]: Watchdogs gesetzt, CAPs neu provisioniert → fertig
```

| Zustand | abbrechbar | Server gestartet | Selbstauslöser |
|---|---|---|---|
| `Planen` | ja, Ende | unterbrochen | – |
| `FallbackSetzen` | ja, Fallback weg | unterbrochen, Fallback beobachten | + Apply-Fallback |
| `WatchdogsAussetzen` | ja, Fallback weg | unterbrochen | + Sicherheitsnetz (bei den Überwachenden) |
| `SkriptStarten` | nein | unterbrochen | + Apply-Skript |
| `SkriptLäuft` | ja, Fallback bleibt | weiterführen | – |
| `FallbackWegnehmen` | nein | weiterführen | − Apply-Fallback |
| `NichtErreicht` | ja, Fallback bleibt | weiterführen | – |
| `ZurückGeladen` | – | weiterführen | − Apply-Fallback (ausgelöst) |
| `Nachprüfen`, `Nacharbeit` | nein | weiterführen | − Sicherheitsnetz |

**Gemeinsam anwenden** ist ein Anwenden pro Gerät, deren Machines über Bedingungen aufeinander
warten (siehe [Bedingungen über Geräte hinweg](#bedingungen-über-geräte-hinweg)).

### Update

**Wofür:** bringt RouterOS auf den neuesten Stand des gewählten Kanals, die Pakete auf das, was
NetBox braucht, und danach die RouterBOARD-Firmware (`updateRouteros`).

**Beachtet:**
- RouterOS 6 geht zuerst auf das neueste v6, dann auf v7.
- Pakete: `routeros` plus das Funkpaket, das NetBox braucht; `wireless` bleibt, wo ein
  Legacy-CAPsMAN läuft, den NetBox nicht kennt. Alles andere wird deinstalliert.
- Alle Pakete eines Schritts werden heruntergeladen, bevor das erste hochgeht, und entfernt,
  wenn der Auftrag vor dem Neustart endet: ein Teil davon darf nicht irgendwann installieren.
- Hochladen über SCP; dafür wird `/ip ssh` kurz auf Passwort-Login gestellt. Danach wird die
  Grösse geprüft (16-MB-Geräte haben den Root im RAM).
- Ein Neustart gilt als erkannt, wenn die Uptime sinkt, nicht wenn die Verbindung abreisst.
- **Erfolg** heisst: erwartete Version *und* alle OSPF-Nachbarn von vorher wieder benachbart
  (`Full`, oder `TwoWay` zwischen zwei DROther).
- Fallback: die alte Partition samt Konfiguration kopieren, ein Scheduler bootet sie, wenn der
  Auftrag sie nicht rechtzeitig wegnimmt.
- Die Watchdogs der Geräte, die dieses überwachen, werden ausgesetzt; ihr Sicherheitsnetz setzt
  sie nach 2 h selbst wieder.
- PoE-Kette: siehe [Bedingungen](#poe-kette).
- Alte Geräte: MD5-Login vor 6.43, v6-SSH-Algorithmen, Board-Namen ohne `RB`.

**Bewusst nicht:**
- Für die Firmware gibt es keinen Weg zurück (nur der Backup-Booter vor Ort).
- RouterOS 6 und Geräte ohne zwei Partitionen haben keinen Fallback; das bestätigt der Mensch
  beim Start (Z23).
- Updates, die nicht dieser Provisioner startet, sieht die PoE-Kette nicht.

```mermaid
stateDiagram-v2
    [*] --> Lesen
    Lesen --> WartetAufKette
    WartetAufKette --> NachbarnMerken: [kein Speisendes startet neu], Schritt nötig
    WartetAufKette --> Firmware: [kein Speisendes startet neu], RouterOS und Pakete stimmen
    NachbarnMerken --> FallbackSetzen
    FallbackSetzen --> Laden: alte Partition kopiert, Scheduler da (G)
    FallbackSetzen --> Laden: ohne Weg zurück (beim Start bestätigt)
    Laden --> Hochladen: Pakete heruntergeladen
    Hochladen --> WartetAufNeustart: alle Pakete auf dem Gerät (G)
    WartetAufNeustart --> Neustart: [kein Gespeistes aktualisiert]
    Neustart --> Laden: Version richtig, weiterer Schritt (v6 → v7)
    Neustart --> FallbackWegnehmen: Version richtig, letzter Schritt (G)
    Neustart --> AltePartition: alte Partition läuft (G)
    Neustart --> [*]: nicht zurück (Z) → fehlgeschlagen
    AltePartition --> [*]: fehlgeschlagen, Protokoll „Update zurückgerollt“
    FallbackWegnehmen --> NachbarnWarten
    NachbarnWarten --> Firmware: alle gemerkten Nachbarn benachbart (G)
    NachbarnWarten --> [*]: Frist (Z) → fehlgeschlagen, fehlende Nachbarn
    Firmware --> [*]: Firmware aktuell → fertig
    Firmware --> WartetAufFirmwareNeustart: Upgrade gesetzt
    WartetAufFirmwareNeustart --> FirmwareNeustart: [kein Gespeistes aktualisiert]
    FirmwareNeustart --> [*]: neue Firmware läuft (G) → fertig
```

| Zustand | abbrechbar | Server gestartet | Selbstauslöser |
|---|---|---|---|
| `Lesen`, `WartetAufKette`, `NachbarnMerken` | ja, Ende | unterbrochen | – |
| `FallbackSetzen` | nein | unterbrochen, Fallback beobachten | + Update-Fallback |
| `Laden`, `Hochladen` | ja, Pakete weg | unterbrochen, Pakete beobachten | + Pakete |
| `WartetAufNeustart` | ja, Pakete weg | unterbrochen | – |
| `Neustart` | nein | weiterführen (wartet auf Uptime) | − Pakete (installiert) |
| `FallbackWegnehmen` | nein | weiterführen | − Update-Fallback |
| `NachbarnWarten` | ja, Ende | weiterführen | – |
| `Firmware`, `WartetAufFirmwareNeustart`, `FirmwareNeustart` | nein | weiterführen | – |

Die Watchdogs, die das Gerät überwachen, setzt der Auftrag vor dem Update auf den **anderen**
Geräten aus (`Watchers`) und danach wieder; deren Sicherheitsnetz ist ein Selbstauslöser bei
**jenen** Geräten, mit Verweis auf diesen Auftrag. Ein Fehler mit gesetztem Fallback, bei dem die
alte Partition nicht läuft, nimmt den Fallback weg (`not_updated`).

### Downgrade

**Wofür:** die Rettung nach einem Update, das zwar läuft, aber Probleme macht: dieselben Pakete
in einer älteren Version (`downgradeRouteros`), vorbelegt mit der Version vor dem letzten Update.

**Beachtet:**
- Hochladen wie beim Update, dann `/system/package/downgrade`, das selbst neu startet.
- Auf die andere Partition wird nichts kopiert: Sie hält vielleicht genau den Weg zurück.
- Ein Gerät ohne zwei Partitionen braucht die Bestätigung beim Start (`check_downgrade`).

**Bewusst nicht:** die Firmware zurückstellen.

```mermaid
stateDiagram-v2
    [*] --> Prüfen
    Prüfen --> [*]: kein Weg zurück → fehlgeschlagen
    Prüfen --> WartetAufKette: zweite Partition oder bestätigt
    WartetAufKette --> Laden: [kein Speisendes startet neu]
    Laden --> Hochladen
    Hochladen --> WartetAufNeustart: Pakete auf dem Gerät (G)
    WartetAufNeustart --> Neustart: [kein Gespeistes aktualisiert], Downgrade ausgelöst
    Neustart --> NachbarnWarten: ältere Version läuft (G)
    Neustart --> [*]: nicht zurück (Z) → fehlgeschlagen
    NachbarnWarten --> [*]: Nachbarn da (G) → fertig
```

Wie das Update ohne Fallback: + Pakete beim `Hochladen`, − beim `Neustart`.

### Neustart, Partitionieren, Partition booten

**Wofür:**
- **Neustart:** „Neu starten“ auf der Geräteseite, mit Rückfrage (`rebootDevice`).
- **Partitionieren:** teilt den Flash in zwei Partitionen, die Voraussetzung für den Fallback
  des Updates (`repartitionDevice`; ab RouterOS 7.20, mindestens 128 MiB, Device-Mode
  `partitions`).
- **Partition booten:** startet von der anderen Partition, z.B. zurück auf die vor dem Update
  (`bootPartition`).

**Beachtet:** Alle drei warten wie ein Update auf die PoE-Kette und erkennen den Neustart an der
Uptime.

**Bewusst nicht:** ein Kaltstart; dafür gibt es „Strom aus und ein“.

```mermaid
stateDiagram-v2
    [*] --> WartetAufKette
    WartetAufKette --> Ausgelöst: [kein Gespeistes aktualisiert]
    Ausgelöst --> [*]: Uptime gesunken, erwartet (G) → fertig
    Ausgelöst --> [*]: nicht zurück (Z) → fehlgeschlagen
```

Partitionieren und Partition booten setzen vor `Ausgelöst` ihren Befehl ab; „erwartet“ heisst dann
zwei Partitionen bzw. die gewählte läuft. Nach einem Start des Servers in `Ausgelöst`:
weiterführen.

### Strom aus und ein

**Wofür:** startet ein über PoE gespeistes Gerät kalt (`powerCycle`): ein hängendes Gerät, oder
die Bestätigung einer Device-Mode-Änderung, die ein warmer Neustart nicht gibt.

**Beachtet:**
- Der Port des Speisenden wird über seinen Werksnamen gefunden und 5 s ausgeschaltet.
- Ein Auftrag des Speisenden, damit er neben dem eigenen Auftrag des Geräts laufen kann (die
  Device-Mode-Änderung wartet ja gerade auf ihn).
- Er wartet, solange das Gerät oder eines, das es speist, Pakete installiert.

**Bewusst nicht:** Der Provisioner startet ihn nie von selbst.

Ein Auftrag des **Speisenden**, der den Strom eines anderen Geräts schneidet. Beim gespeisten
Gerät ist dafür kein eigener Zustand nötig: Ob ihm gerade der Strom geschnitten wird, folgt aus
dem Auftrag seines Speisenden.

```mermaid
stateDiagram-v2
    [*] --> WartetAufKette
    WartetAufKette --> Aus: [weder das Gespeiste noch eines, das es speist, aktualisiert]
    Aus --> WartetAufGerät: Port 5 s aus und wieder ein (G)
    WartetAufGerät --> [*]: Gespeistes antwortet (G) → fertig
    WartetAufGerät --> [*]: Frist (Z) → fehlgeschlagen
```

### Device-Mode

**Wofür:** schaltet Features ein, die der Provisioner braucht und die der Device-Mode sperrt
(`changeDeviceMode`): den Scheduler für den Apply-Fallback, `/ip socks` auf dem Gerät des
Management-Proxys, `partitions` für den Update-Fallback.

**Beachtet:**
- RouterOS übernimmt die Änderung nur nach einer Bestätigung **am Gerät** (Taste oder Kaltstart)
  innerhalb von 10 min; ein warmer Neustart bestätigt nicht.
- Die Verbindung muss offen bleiben, solange das Gerät zählt; schliesst sie, bricht RouterOS ab.
- Gesendet werden nur Features, die aus sind: Auch eine Änderung ohne Wirkung zählt herunter.

**Bewusst nicht:** andere Features des Device-Mode.

```mermaid
stateDiagram-v2
    [*] --> Lesen
    Lesen --> [*]: alles schon an → fertig
    Lesen --> WartetAufKette: Features fehlen
    WartetAufKette --> WartetAufTaste: [kein Gespeistes aktualisiert], Update gesendet, Gerät zählt (G)
    WartetAufTaste --> Neustart: Gerät geht weg (G) — Taste oder Kaltstart (B am Gerät)
    WartetAufTaste --> [*]: „update canceled“ nach 10 min (Z) → fehlgeschlagen
    Neustart --> [*]: Features an (G) → fertig
```

Die Bestätigung ist eine Bedienung **am Gerät**, nicht im Provisioner; der Auftrag beobachtet nur,
dass das Gerät weggeht. `WartetAufTaste` hält die Verbindung offen, sonst bricht RouterOS ab:
Nach einem Start des Servers ist die Änderung deshalb sicher weg, also *unterbrochen*, ohne
Selbstauslöser.

### Ersteinrichtung

**Wofür:** richtet ein Gerät ein, das nicht antwortet (Werkszustand, verlorene Adresse), über
`/tool mac-telnet` von einem Nachbarn aus, der antwortet (`bootstrap`): Passwort, Adresse, API,
eine Firewall-Freigabe.

**Beachtet:**
- Das Gerät wird über die MAC gewählt, die der Nachbar an diesem Port sieht oder die der Mensch
  eintippt, **nie über eine MAC aus NetBox**.
- Die Konsole des Nachbarn läuft über SSH; vorher wird geprüft, dass er SSH-Weiterleitung
  erlaubt, sonst bleibt alles unberührt.
- `mac-telnet` mit und ohne `interface=` (je nach Gerät antwortet nur eine Variante).
- Ein spät antwortendes Gerät lässt Logins scheitern oder Sitzungen abbrechen: Jede Anmeldung
  wird bis zu einer Frist wiederholt, nach einem Abbruch laufen alle Befehle noch einmal (jeder
  tut nichts, wo er schon gilt).
- Endet die Sitzung früh, landen Befehle auf der Konsole des Nachbarn; jeder Befehl tut dort
  nichts (`netboxProvisionerNeighbor`).
- Ein DHCP-Client, der die Adresse hält, wird ausgeschaltet; der CAP eines Heim-APs nur nach
  einem Werks-Login.

**Bewusst nicht:** die eigentliche Konfiguration; die bringt das erste Anwenden.

```mermaid
stateDiagram-v2
    [*] --> NachbarPrüfen
    NachbarPrüfen --> [*]: Weiterleitung aus → fehlgeschlagen
    NachbarPrüfen --> Konsole: SSH zum Nachbarn, mac-telnet (G)
    Konsole --> Konsole: Login versucht (G), bis Frist
    Konsole --> WartetAufApi: Einrichtung durch (G)
    WartetAufApi --> [*]: API antwortet (G) → fertig
    WartetAufApi --> [*]: Frist (Z) → fehlgeschlagen
```

Läuft über ein anderes Gerät (den Nachbarn). In `Konsole` nicht abbrechbar; nach einem Start des
Servers ist die Sitzung weg, also *unterbrochen*. Sie hinterlässt beim Nachbarn nichts, was
selbst auslöst (die Befehle tun dort nichts, `netboxProvisionerNeighbor`).

### Aufräumen

**Wofür:** nimmt einen Selbstauslöser weg, den ein Auftrag nicht zu Ende gebracht hat
(`clearLeftover`): Fallback stoppen oder jetzt laden, Update-Fallback stoppen, Watchdogs jetzt
wiederherstellen, Pakete entfernen.

**Beachtet:** Nichts davon geschieht von selbst; ein schon weggeräumter Eintrag ist ein Fehler
(`LEFTOVER_GONE`), kein stilles Nichtstun.

**Bewusst nicht:** das Apply-Skript, solange es läuft (es endet selbst), und eine
Device-Mode-Änderung (die bleibt nie liegen).

Ein Zustand `Aufräumen`, dann Ende. „Jetzt zurück“ hat zusätzlich `WartetAufGerät` wie ein
Neustart. Wirkt auf **Selbstauslöser** (der gewählte Eintrag weg).

### NetBox schreiben, zurücknehmen

**Wofür:** schreibt den Plan eines Helfers nach NetBox (`writeNetboxPlan`) oder nimmt ein solches
Schreiben zurück (`undoNetboxWrite`).

**Beachtet:**
- Der Plan wird neu berechnet und abgelehnt, wenn sein Fingerabdruck sich geändert hat; vor jedem
  Schritt wird geprüft, dass NetBox noch so ist, wie der Plan es fand.
- Jedes geschriebene Objekt hält seine alten Werte, damit Rückgängig sie rückwärts wieder setzt;
  Gelöschtes wird neu angelegt (mit neuer Id).
- Jede Änderung trägt eine Changelog-Nachricht mit Plan und Benutzer; danach wird die Topologie
  neu gelesen.

**Bewusst nicht:** eine Transaktion. NetBox hat keine über mehrere Objekte, ein Fehler mitten im
Plan lässt den Teil davor geschrieben (`poe_type` an koenig-hegglin).

Ein Zustand pro Schritt des Plans (`Schritt n von m`), dann Ende. Wirkt auf **NetBox**: Jeder
geschriebene Schritt kommt sofort dazu, auch wenn ein späterer fehlschlägt. Nach einem Start des
Servers: unterbrochen, `Geschrieben` hält, was durchging.

## Bedingungen über Geräte hinweg

Zwei Dinge hängen an den Aufträgen **mehrerer** Geräte. Sie sind keine Zustände, sondern
Bedingungen, die ein Übergang über die Auftrag-Zustände der anderen prüft. Gespeichert werden
muss dafür nichts Eigenes: Nach einem Neustart des Servers folgen sie aus den gespeicherten
Aufträgen.

### PoE-Kette

Ein Gerät, das gerade Pakete auf dem Flash hat, kann ein Stromausfall unbrauchbar machen. Heute
schützen das die Leases in `device::power`; als Bedingungen:

| Bedingung | wahr, wenn kein Gerät der Kette … |
|---|---|
| `[kein Gespeistes aktualisiert]` | … unterhalb (das es direkt oder weiter unten speist) in einem Update oder Downgrade nach `WartetAufKette` ist |
| `[kein Speisendes startet neu]` | … oberhalb neu startet (`Neustart`, `Ausgelöst`, `FirmwareNeustart`, Device-Mode `WartetAufTaste` und `Neustart`) oder einem Gerät darunter den Strom schneidet („Strom aus und ein“ `Aus`, `WartetAufGerät`) |
| `[weder das Gespeiste noch eines, das es speist, aktualisiert]` | … ab dem gespeisten Gerät abwärts in einem Update oder Downgrade nach `WartetAufKette` ist |

Wer auf eine Bedingung wartet, bleibt im Zustand `WartetAuf…`; der Übergang geschieht, sobald ein
anderes Gerät seinen Zustand ändert. Prüfen und Übergang geschehen unter einer Sperre, damit sich
zwei Geräte nicht kreuzen (wie heute `acquire`).

### Gemeinsam anwenden

**Wofür:** wendet mehrere Geräte zusammen an (`applyBatch`), vor allem beide Enden einer
Funkstrecke: Einzeln geändert schneidet das erste Ende dem zweiten den Weg ab.

**Beachtet:**
- Ein Fallback für alle, der im selben Moment auslöst; jedes Skript zieht seinen auf etwa
  denselben Zeitpunkt vor.
- Die Skripte starten gleichzeitig und beginnen mit `:delay 15s`, damit jeder Start durchkommt,
  bevor die ersten Änderungen eine Verbindung kappen. Nacheinander gestartet schnitt im Lab der
  AP der 60-GHz-Strecke die Station ab, bevor ihr Skript lief.
- Die Fallbacks gehen nur weg, wenn **jedes** Gerät erreicht und durch ist; sonst laden alle
  zusammen ihre Konfiguration von vorher.
- Abbrechen eines Geräts bricht alle ab.

**Bewusst nicht:** Teilerfolg. Lieber alles zurück als eine halbe Funkstrecke.

Ein Anwenden pro Gerät, alle mit Verweis auf die Gruppe. Drei Übergänge haben eine Bedingung über
die Gruppe:

| Übergang | Bedingung |
|---|---|
| `Planen → FallbackSetzen` | `[alle Geräte der Gruppe haben geplant]` |
| `SkriptStarten` (der Start selbst) | `[alle haben ihr Skript geschrieben]`, dann starten alle gleichzeitig |
| `SkriptLäuft → FallbackWegnehmen` | `[alle sind durch, erreicht und in Reihe]`, sonst gehen alle nach `NichtErreicht` (`BatchIncomplete`) |

Ein Fehler eines Geräts vor dem Start nimmt bei allen den Fallback weg (`BatchStopped`); jedes
schreibt seinen Protokoll-Eintrag.

## Teilzustand „Selbstauslöser“

Ein Selbstauslöser ist etwas, das der Provisioner auf dem Gerät hinterlegt und das **von selbst
wirkt, wenn niemand es vorher wegnimmt**: Der Provisioner muss nichts mehr tun, damit es
geschieht. Gerade deshalb muss der Zustand es kennen. Der Teilzustand ist eine Menge solcher Einträge,
jeder mit eigener kleiner Machine (`Gesetzt`, `Unbekannt`, und kurz `Ausgelöst`, bis das
Protokoll es festhält). Leer ist der
Grundzustand.

```mermaid
stateDiagram-v2
    [*] --> Gesetzt: Auftrag legt an
    Gesetzt --> [*]: weggenommen (Auftrag oder Aufräumen)
    Gesetzt --> Ausgelöst: Beobachtung — Eintrag weg, Uptime gesunken
    Gesetzt --> Unbekannt: Gerät nicht erreichbar
    Unbekannt --> Gesetzt: erreicht, Eintrag da
    Unbekannt --> Ausgelöst: erreicht, Eintrag weg, Uptime gesunken
    Unbekannt --> [*]: erreicht, Eintrag weg, kein Neustart
    Ausgelöst --> [*]: Protokoll-Eintrag geschrieben
```

| Eintrag | Wird angelegt von | Fällig | Weggenommen von | Ausgelöst heisst |
|---|---|---|---|---|
| Apply-Fallback | Anwenden `FallbackSetzen` | Uptime aus der Marke + Frist | Anwenden `FallbackWegnehmen`; Aufräumen „Fallback stoppen“ / „Jetzt zurück“ | Konfiguration von vorher geladen |
| Update-Fallback | Update `FallbackSetzen` | nach `update_fallback_seconds` | Update `FallbackWegnehmen`; Aufräumen | alte Partition gebootet |
| Sicherheitsnetz | Update vor `Lesen`, Anwenden `WatchdogsAussetzen` (bei den überwachenden Geräten) | 2 h | Ende des Auftrags; Aufräumen „Jetzt wiederherstellen“ | Watchdogs vom Gerät selbst gesetzt |
| Pakete | Update, Downgrade `Hochladen` | nächster Neustart | Neustart (installiert), Abbruch, Aufräumen | beim nächsten Neustart installiert |
| Apply-Skript läuft | Anwenden `SkriptStarten` | endet selbst | Beobachtung: Job weg | – |

Ein Eintrag, den der Server nach einem Neustart auf dem Gerät findet, **ohne** dass der Zustand
ihn kennt (andere Instanz, älterer Stand), heisst `Fremd` und wird wie heute in
„Liegengebliebenes“ gezeigt. Ein Eintrag, den der Zustand kennt, das Gerät aber nicht mehr hat,
wird durch die Beobachtung entfernt, nach einem Neustart des Geräts mit Protokoll-Eintrag
„ausgelöst“.

`device::leftovers::read` bleibt die Beobachtung dieses Teilzustands: Es vergleicht nicht mehr nur mit
„leer“, sondern mit dem gespeicherten Soll.

## Teilzustand „NetBox“

```mermaid
stateDiagram-v2
    [*] --> Gleich
    Gleich --> Geschrieben: Helfer schreibt (Bedienung)
    Geschrieben --> Geschrieben: weiterer Helfer schreibt
    Geschrieben --> Gleich: Rückgängig (Bedienung)
    Geschrieben --> Gleich: Anwenden fertig, Diff leer (Beobachtung)
```

`Geschrieben` hält je Schreiben, was vorher in NetBox stand (`NetboxWritten.before`), damit
Rückgängig einen Neustart überlebt. Heute geht es dabei verloren (CLAUDE.md: „lost on a server
restart“). Ein fehlgeschlagenes Schreiben mitten im Plan (wie bei `poe_type` an koenig-hegglin)
ist `Geschrieben` mit dem Teil, der durchging, und einem Protokoll-Eintrag.

## Protokoll

Was geschehen ist, gehört nicht zum Zustand, sondern ins Protokoll: das Ende jedes Auftrags (mit
dem Zustand, in dem er endete, und seinen Events), ein ausgelöster Selbstauslöser (beim Apply mit
den Marken des Skripts), ein teilweise geschriebener Plan, ein beim Start gefundener `Fremd`-Eintrag.
Wie es aussieht, wo es liegt und wie lange es bleibt, beschreibt ein eigenes Konzept (noch zu
schreiben). Dieses Konzept braucht davon nur: Ein Übergang kann einen Eintrag schreiben, und der
Zustand verweist nie auf ihn.

## Eingaben

| Eingabe | Quelle | Wirkt auf |
|---|---|---|
| Start (Anwenden, Update, …) | Bedienung | Auftrag `Frei` → erster Zustand der Art |
| Abbrechen | Bedienung | Auftrag: Ende in einem abbrechbaren Zustand, sonst nach dem laufenden |
| Aufräumen (Fallback stoppen, Jetzt zurück, …) | Bedienung | Selbstauslöser: Eintrag weg |
| NetBox schreiben, Rückgängig | Bedienung | NetBox |
| erreichbar / nicht erreichbar | Beobachtung | Selbstauslöser `↔ Unbekannt`, Zustand des Auftrags |
| Uptime gesunken | Beobachtung | Neustart erkannt; Selbstauslöser `Ausgelöst`, Protokoll |
| anderes Gerät ändert seinen Auftrag-Zustand | Beobachtung | Übergänge, die auf eine Bedingung warten |
| Marke des Skripts | Beobachtung | Anwenden `SkriptLäuft` |
| Scheduler-Eintrag da / weg | Beobachtung | Selbstauslöser |
| Version, OSPF-Nachbarn | Beobachtung | Update `Neustart`, `NachbarnWarten` |
| Frist abgelaufen | Zeit | der Zustand des Auftrags, der auf etwas wartet |
| Taste oder Kaltstart am Gerät | Bedienung am Gerät | Device-Mode `WartetAufTaste` (beobachtet: Gerät geht weg) |
| Server startet | Server | Auftrag je Zustand weiterführen oder unterbrochen, alle Teilzustände nachprüfen |
| Server wird beendet | Server | Abbrechen aller Aufträge, Frist 60 s, dann speichern |

## Speicher

- Ein Verzeichnis `state_dir` (Vorgabe neben `config.yaml`, im Helm-Chart ein Volume).
- Pro Gerät mit Eintrag eine Datei `devices/<netbox-id>.json` mit allen Teilzuständen. Geschrieben bei
  jedem Übergang (temporär schreiben, dann umbenennen). Gelöscht, wenn alle Teilzustände im
  Grundzustand sind.
- Die Events der Aufträge gehören zum Protokoll (eigenes Konzept).

## Was sich für die Bedienung ändert

- Die Geräteseite zeigt alle Teilzustände statt nur des laufenden Jobs: „Update wartet
  auf die Nachbarn“, „Apply-Fallback gesetzt, löst laut Gerät in 40 s aus“, „In NetBox geschrieben, noch nicht
  angewendet“.
- „Liegengebliebenes“ wird zum Teilzustand **Selbstauslöser**: Jeder Eintrag weiss, welcher Auftrag ihn
  angelegt hat. Ein Fund ohne Eintrag heisst `Fremd`.
- Nach einem Neustart des Servers steht im Protokoll jedes betroffenen Geräts „unterbrochen in
  X“ mit den Events bis dahin, statt eines leeren Jobs.
- Die Site-Übersicht kann zeigen, welche Geräte einen Eintrag haben (alle anderen sind ruhig).

## Offene Fragen

1. **Weiterführen nach einem Neustart.** Ein Anwenden in `SkriptLäuft` oder ein Update in
   `NachbarnWarten` könnte der Server nach dem Start weiterführen: Er hat alles, was er braucht
   (Zustand, Fristen in Uptime). Das ist kein Reparieren, sondern der Auftrag, den ein Mensch
   gestartet hat. Ohne Weiterführen lädt der Apply-Fallback auch eine gute Konfiguration zurück,
   wenn der Server in diesem Moment neu startet. Vorschlag: weiterführen nur in Zuständen, die
   nur noch lesen und den eigenen Selbstauslöser wegnehmen (in den Tabellen „weiterführen“),
   alle anderen enden als unterbrochen.
2. **Mehrere Server.** Zwei Instanzen mit demselben `state_dir` sind nicht vorgesehen. Eine
   Sperrdatei verhindert den Start der zweiten.
3. **Gerät aus NetBox gelöscht.** Ein Eintrag zu einer Id, die NetBox nicht mehr kennt, kann
   nicht mehr beobachtet werden (keine Adresse). Vorschlag: Er bleibt und wird auf der
   Site-Übersicht gezeigt, bis jemand ihn verwirft (B); das Verwerfen kommt ins Protokoll.

## Reihenfolge

1. Dieses Konzept abstimmen: zuerst die State-Machines, dann Speicher und Protokoll.
2. Typen der Teilzustände und Übergänge in `backend::device_state`, rein (ohne Gerät), mit Tests der
   Übergangstabellen. Die heutigen Jobs melden ihre Zustände daran, ohne ihren Ablauf zu ändern.
3. Speicher (`state_dir`), Einlesen beim Start, Ende als unterbrochen.
4. Teilzustand **Selbstauslöser** an `leftovers` anschliessen (Soll gegen Ist), Geräteseite zeigt
   die Teilzustände.
5. **NetBox** und Rückgängig über Neustarts.
6. Geordnetes Beenden.
7. Je nach Antwort auf Frage 1: Weiterführen nach dem Start.
