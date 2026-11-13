# Zugriff auf die Geräte

Stand 2026-11-13, besprochen mit dem Benutzer (Basis-Fakten und Ableitungen am selben Tag
nach der Messung). Die Schritte 1–3 (B36: Befehle als Typen mit
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
- `Reader` liest künftig über die Basis-Fakten (`read` und `observe` mit `Freshness`);
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

## Basis-Fakten und Ableitungen

Gespeichert wird nur, was gemessen wurde: ein **Basis-Fakt** ist eine Frage an etwas draussen
mit ihrer Antwort, auch einer Absage oder keiner. Alles darüber wird bei jedem Zugriff daraus
abgeleitet und nicht gespeichert.

Basis-Fakten, jeder unter dem Schlüssel dessen, was gefragt wurde:

- der **Print eines Menüs**, immer die ganze Liste, ohne Filter und `.proplist`: wer nur einen
  Teil braucht, filtert im gespeicherten Ergebnis. Ausnahme sind Menüs, deren ganze Liste zu
  gross ist (`/ip/route` mit BGP, `/log`, Host-Tabelle der Bridge, Leases grosser Server): dort
  gehört der Filter zum Schlüssel. Ein Menü, das fehlt (Paket nicht installiert,
  `collect_if_present`), ist auch eine Antwort;
- ein **Monitor als Single Shot** (`monitor … once`, PoE, Ethernet): strukturell dasselbe wie ein
  Print;
- ein **Ping** von einem Gerät an eine Adresse (Funkbrücken der Interface-Ansicht,
  Watchdog-Ziele in `generate_live`);
- ein **Login-Versuch** an einer Adresse auf einem Pfad mit Credentials: ging, verweigert,
  keine Antwort, anderes Gerät. Auch die Unerreichbarkeit ist gemessen und gilt, solange die
  `Freshness` des Lesers reicht (wie heute `judge_login`);
- **HTTP zu MikroTik** (neueste Version eines Kanals, Paketgrössen), nach URL; sie darf in der
  Regel länger gelten als eine Messung am Gerät.

Wer liest, gibt die `Freshness` an (`MaxAge`, `Since`), wie heute; Mitlaufen bei einer
laufenden Frage, Metriken und Live-Updates (`LiveExtension` notiert gelesene Fakten) gelten für
Basis-Fakten wie heute für die Facts. Nicht gespeichert wird, was einen Vorgang über die Zeit
beobachtet (der Countdown des Device-Mode, `observe` beim Warten auf eine Bedingung).

**Ableitungen** sind die heutigen Facts, die aus Prints, Monitoren und Pings bestehen: Config
(`DeviceCfg::fetch`), RouterosState, Leftovers, Partitions, DeviceMode, Interfaces, Neighbors,
InterfaceLive, Lsdb, DeviceMapData, DeviceMetrics, Route (der erste Login, der geht). Sie lesen
über `Reader` und rechnen; so liest eine Seite, die `/system/resource` für drei Ableitungen
braucht, es einmal, auch bei vielen Benutzern. Die Abfragen über mehrere Menüs waren nie
atomar: eine Ableitung muss Inkonsistenzen zwischen Menüs verschiedenen Alters aushalten
(heute zwischen Menüs, die kurz nacheinander gelesen wurden).

Ungültig wird ein Basis-Fakt:

1. **durch Schreiben, auf jeder Verbindung**: er merkt sich die Generation, bei der er gelesen
   wurde; ist der Zähler des Geräts seither gestiegen, gilt er nicht mehr. Ein Read, der
   während eines Schreibens lief, wird nicht gespeichert.
2. **durch das, was ein Job oder eine Mutation bewirkt**: sie holen danach nichts von sich aus
   und legen nichts ab (`facts::put` und `keep_in_line` fallen weg), sie machen aber an jeder
   Stelle ungültig, die etwas verändert haben kann. API-Befehle tun das über die Generation von
   selbst; was ohne API-Befehl wirkt (das Apply-Script nach `/execute`, Fallback, Reboot,
   Paketinstallation beim Booten, Power-Cycle des Speisers), erhöht die Generation dort, wo es
   ausgelöst wird, und am Ende des Jobs noch einmal (wie heute `JobKind::invalidates`). Wer
   danach liest, der Check des Applys oder die Geräteseite, fragt neu, und nur, was er braucht.
3. **durch das Alter**: was niemand meldet (Winbox, Leases, Nachbarn, OSPF), begrenzt nur das
   Alter, das der Leser zulässt (höchstens 15 min, wie heute).

Eine Topologie-Änderung in NetBox macht keinen Basis-Fakt ungültig: was aus NetBox kommt,
rechnen die Ableitungen beim nächsten Zugriff neu (heute `invalidate_everywhere` für Config).

## Der Rahmen pro Operation

Eine async-graphql-Extension legt um jede Ausführung (Query, Mutation, jede Runde einer
Subscription, wie `live::rounds` sie zählt) einen `operation::Scope` (`tokio::task_local!`).
Er hält nichts selbst und lädt nichts: er merkt sich, welchen Stand jedes Basis-Fakts und welche
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

- Welche Typen von mikrotik-model mit `.proplist` oder Filter lesen (dann zwei Schlüssel für
  dasselbe Menü): beim Umbau prüfen und auf die ganze Liste umstellen.
- `DeviceCfg::fetch` liest seit B36 über `Reads`, also künftig auch über die Basis-Fakten;
  prüfen, ob es Menüs mit Filter liest.
- Die Metriken `device_query_*` zählen heute nach Fact-Art; nach Menü wären es viele Reihen:
  Pfade zu Gruppen zusammenfassen oder nur Art (Print, Monitor, Ping, Login, HTTP) als Label.
- Die Rechenzeit der Ableitungen (der Diff jeder Geräteseite, der Site-Diff über alle Geräte)
  messen, wenn sie nicht mehr gespeichert sind.

## Schritte

1.–3. gebaut (B36, [device-access.md](../device-access.md)).
4. **Provisioner**: Pool, Basis-Fakten, Generation, Operationsrahmen, HTTP im Rahmen (C17).
   Gebaut ist der Zähler aus „Prüfen“ (`operation`, [device-access.md](../device-access.md)),
   die Generation pro Gerät (`access::generation`), und als Pool dient der Fact `Login`, der
   eine Verbindung 60 s wieder gibt. Die Messung (2026-11-13) zeigt Doppel nur noch auf der
   Geräteseite: 10 Menüs des Geräts zwei- oder dreimal bei 67 Befehlen, je Laden der Seite.
   Reihenfolge: (a) mikrotik-api gibt die rohen Antworten heraus und baut aus gespeicherten
   wieder `Replies`; (b) Prints als Basis-Fakten im `Reader`, die Geräteseite messen; (c)
   Monitor, Ping, Login und HTTP als Basis-Fakten; (d) die Ableitungen aus dem Store nehmen,
   Jobs nur noch ungültig machen lassen (`put`, `keep_in_line` weg).

## Prüfen

- CHR-Tests wie bisher.
- Ein Zähler der Befehle pro Gerät und Pfad (Log auf `debug` oder Metrik): Geräteseite,
  Netzkarte und Rollout-Plan im Lab zeigen jeden Pfad pro Gerät und Operation einmal, und zwei
  Browser auf derselben Geräteseite einen Login und ein Lesen pro Menü.
