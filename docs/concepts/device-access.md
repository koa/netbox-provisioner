# Zugriff auf die Geräte

Stand 2026-11-13, besprochen mit dem Benutzer. Die Schritte 1–3 (B36: Befehle als Typen mit
ihrer Wirkung, `Connection`, Aktionskatalog, `Reader`/`Session`) sind gebaut und in
[device-access.md](../device-access.md) beschrieben; offen ist Schritt 4 (C17: einmal laden pro
GraphQL-Operation, früher `operation-cache.md`).

## Ziel

- Jeder Befehl an ein Gerät sagt durch seinen Typ, was er bewirkt; die Fassade, über die er
  geht, sagt, wer ihn schicken darf. Beides prüft der Compiler, nicht eine Konvention.
- Eine Verbindung (ein Login) dient beliebig vielen Fassaden: Rohes, Getyptes, Aktionen.
- Lesen wird geteilt: über parallele GraphQL-Operationen und viele Benutzer hinweg werden
  Verbindungen und Gelesenes weiterverwendet, solange sie ausreichen. Innerhalb einer Operation
  wird jeder Datensatz einmal geladen, ausser es wird auf eine Bedingung gewartet (Regel
  2026-11-07).
- Schreiben bleibt bei den Jobs, auf eigenen Verbindungen; was sie schreiben, macht das
  Gelesene des Geräts ungültig, auch über eine Verbindung, die offen bleibt.

## Heute

- Jeder Befehl ist ein Typ mit seiner Wirkung, jede Verbindung hinter `Reader` oder `Session`
  ([device-access.md](../device-access.md)); `Reader` liest noch ohne Cache.
- Die Topologie (`TopologyHolder::topo_lock`) wird neu aus NetBox gelesen, sobald sie älter als
  10 s ist, auch mitten in einer Anfrage (Rollout-Plan über mehrere Sites).
- Die Facts (`facts::get`) teilen Gelesenes über Anfragen hinweg, aber jedes nach seinem eigenen
  Schlüssel: Config und RouterosState lesen beide `/system/routerboard`, RouterosState und die
  Leftovers `/system/resource`, Config und Interfaces `/interface`, dazu `/file` und
  `/system/scheduler` mehrfach. Wo eine Funktion doppelt las, reicht sie die Zeilen heute von
  Hand weiter (`update::state_from`, `leftovers::read_with_files`, `progress::root_in_ram_of`,
  `wanted_packages_for`).
- Jede Abfrage öffnet ihre Verbindung selbst; NetBox-REST und MikroTik-HTTP werden pro Aufruf
  gelesen.

## Was Schritt 4 auf B36 baut

- Die `Generation` jeder `Connection` steigt bei jedem `Write`- und `SessionBound`-Befehl,
  gesteuert von seinem `Effect`, nicht vom Pfad; alle Verbindungen eines Geräts teilen sie.
- Der Pool gibt nur `Reader` heraus: eine GraphQL-Operation schreibt nie, ohne einen Job zu
  starten. `Session` gibt es nur mit der Sperre des Geräts.
- `Reader` liest künftig über die Menü-Facts (`read` mit `Freshness`), `observe` immer live;
  `Session::reader` liest frisch, ohne Cache.
- `Reader` und `Session` bleiben im Provisioner, nicht in mikrotik-api: sie kennen Pool,
  Job-Sperre und Facts.

## Geteilte Lese-Verbindung (Pool)

- Prozessweit, pro Geräte-ID eine Verbindung für Reads, die alle Operationen, Facts und Seiten
  teilen; ein neuer Login nur, wenn keine da ist oder `is_closed()`.
- Ungenutzt geschlossen nach einer Leerlaufzeit (60 s): die schwachen Geräte sollen nicht
  dauernd Sessions offen haben.
- Verworfen bei einer Topologie-Änderung des Geräts (Adresse, Weg, Credentials), am Ende jedes
  Jobs auf dem Gerät (Apply und Update können den Weg ändern, ein Reboot beendet sie) und bei
  `WrongDevice` oder einem Login-Fehler.
- Ein Semaphor pro Gerät hält die Reads eines Geräts nacheinander (schwache CPU und RAM), für
  alle Operationen gemeinsam.

## Gelesenes: Menü-Facts

Statt eines zweiten Caches ist jedes gelesene Menü ein Fact (`FactKind::Menu { path, query }`),
dessen Wert die rohen Sätze sind; jeder Leser parst sie mit seinem Typ. So gelten, was die
Facts schon können: Alter nach Nutzer (`MaxAge`, `Since`), Mitlaufen bei einer laufenden
Anfrage, Metriken, Live-Updates (`LiveExtension` notiert gelesene Facts). Die heutigen Facts
(Config, InterfaceLive, RouterosState, …) bauen darauf auf, so liest eine Seite, die
`/system/resource` zweimal braucht, es einmal, auch bei vielen Benutzern.

Ungültig wird ein Menü-Fact:

1. **durch Schreiben, auf jeder Verbindung**: es merkt sich die Generation, bei der es gelesen
   wurde; ist der Zähler des Geräts seither gestiegen, gilt es nicht mehr. Ein Read, der
   während eines Schreibens lief, wird nicht gespeichert. Ein Apply, das über seine offene
   Verbindung `set`s schickt, macht die Facts damit sofort ungültig, ohne etwas zu melden.
2. **während ein Job das Gerät hält**: ein Apply-Script läuft nach `/execute` minutenlang ohne
   API-Befehl, dazu Fallback, Reboot, Paketinstallation beim Booten. Solange die Sperre
   besteht, lesen `Reader` frisch über den Pool und speichern nichts; am Ende des Jobs steigt
   die Generation noch einmal und der Pool-Eintrag geht (wie heute `JobKind::invalidates`).
3. **durch das Alter**: was niemand meldet (Winbox, Leases, Nachbarn, OSPF), begrenzt nur das
   Alter, das der Nutzer zulässt (höchstens 15 min, wie heute).

`Observe` wird nie gespeichert.

## Der Rahmen pro Operation

Eine async-graphql-Extension legt um jede Ausführung (Query, Mutation, jede Runde einer
Subscription, wie `live::rounds` sie zählt) einen `operation::Scope` (`tokio::task_local!`).
Er hält nichts selbst und lädt nichts: er merkt sich, welchen Stand jedes Menü-Facts und welche
Topologie die Operation zuerst bekam, und gibt ihr für den Rest genau den. Wird ein Fact
mittendrin ungültig, sieht die Operation weiter ihr eines Bild; die nächste Operation oder
Runde liest neu, die Subscription erfährt es über `facts::Change`.

- **Topologie**: `topo_lock` gibt im Rahmen die Topologie, die er beim ersten Zugriff nahm
  (heute `TopologyHolder::snapshot` von Hand in den Resolvern).
- **HTTP**: NetBox-REST-GETs und MikroTik-Metadaten nach URL im Rahmen.
- **Warten**: wer auf eine Bedingung wartet, liest frisch (`operation::fresh(fut)`). Fast alles
  Warten läuft in Jobs, die keinen Rahmen haben.
- **Facts**: `facts::get` startet das Fragen als eigenen Task; der Rahmen geht mit, damit zwei
  Facts derselben Operation dieselben Stände sehen.
- Eine Mutation ohne Job, die schreibt und danach liest, braucht `operation::fresh`; beim Umbau
  prüfen, ob es eine gibt (Kandidaten: `publishLayout`, die NetBox-Schreiber, die nur NetBox
  schreiben).

Danach fallen die Handarbeiten weg (`snapshot` in den Resolvern, `update::state_from`,
`leftovers::read_with_files`, `progress::root_in_ram_of`, `wanted_packages_for`).

## Offen

- `DeviceCfg::fetch` (mikrotik-model) liest viele Menüs intern, vor allem in Jobs (frisch,
  nicht betroffen) und für die Diff-Ansicht. Über die Menü-Facts ginge es nur mit einem
  Lese-Haken in mikrotik-model (generisch, kein `dyn`); erst einbauen, wenn die Messung zeigt,
  dass es nötig ist.

## Schritte

1.–3. gebaut (B36, [device-access.md](../device-access.md)).
4. **Provisioner**: Pool, Menü-Facts, Generation, Operationsrahmen, HTTP im Rahmen (C17).
   Gebaut ist der Zähler aus „Prüfen“ (`operation`, [device-access.md](../device-access.md)),
   die Generation pro Gerät (`access::generation`), und als Pool dient der Fact `Login`, der
   eine Verbindung 60 s wieder gibt. Die Messung (2026-11-13) zeigt Doppel nur noch auf der
   Geräteseite: 10 Menüs des Geräts zwei- oder dreimal bei 67 Befehlen, je Laden der Seite.

## Prüfen

- CHR-Tests wie bisher.
- Ein Zähler der Befehle pro Gerät und Pfad (Log auf `debug` oder Metrik): Geräteseite,
  Netzkarte und Rollout-Plan im Lab zeigen jeden Pfad pro Gerät und Operation einmal, und zwei
  Browser auf derselben Geräteseite einen Login und ein Lesen pro Menü.
