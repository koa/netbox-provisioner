# Einmal laden pro GraphQL-Operation

Regel (2026-11-07): Innerhalb einer GraphQL-Operation wird jeder Datensatz nur einmal geladen,
ausser es wird darauf gewartet, dass eine Bedingung erfüllt ist. Das soll nicht pro Resolver
von Hand gelöst sein, sondern generisch, wie eine Transaktion pro Operation.

## Heute

- Die Topologie (`TopologyHolder::topo_lock`) wird neu aus NetBox gelesen, sobald sie älter als
  10 s ist, auch mitten in einer Anfrage: Der Rollout-Plan liest alle Geräte mehrerer Sites und
  dauert länger, Teile der Antwort können so auf zwei Topologien beruhen.
- Die Facts (`facts::get`) teilen Gelesenes über Anfragen hinweg, aber jedes nach seinem eigenen
  Schlüssel. Verschiedene Facts derselben Seite lesen dieselben Menüs je für sich:
  `/system/routerboard` (Config und RouterosState), `/system/resource` (RouterosState und die
  Uptime der Leftovers), `/interface` (Config und Interfaces), `/file` und `/system/scheduler`.
- Auch innerhalb einer Funktion wurde doppelt gelesen (Rollout-Plan: `/system/resource`,
  `/system/routerboard`, `/file` je zweimal; Leftovers: `/system/scheduler` zweimal).
- NetBox-REST (Helfer: freie Adressen, Prefixe) und MikroTik-HTTP (neueste Version,
  Paketgrössen) werden pro Aufruf gelesen.

## Vorschlag

Eine Operation ist ein Rahmen (`operation::Scope`), den eine async-graphql-Extension um jede
Ausführung legt (`execute` für Query und Mutation, jede Runde einer Subscription einzeln, wie
`live::rounds` sie schon zählt), getragen als `tokio::task_local!`. Was liest, fragt den Rahmen,
falls einer da ist:

1. **Topologie**: `topo_lock` gibt im Rahmen die Topologie, die der Rahmen beim ersten Zugriff
   genommen hat, und liest NetBox darin nie ein zweites Mal. Ohne Rahmen (Jobs, Hintergrund)
   wie heute.
2. **Gerätemenüs**: ein Lese-Cache auf der Verbindung in mikrotik-api. `MikrotikDevice` bekommt
   optional einen `ReadCache` (Schlüssel: die Geräte-ID des Provisioners, nicht die Verbindung,
   denn `connect_again` öffnet neue). Ein `print` ohne Seiteneffekt (`/…/print`, ohne
   `follow`/`interval`) wird mit seinen Argumenten als Schlüssel gemerkt, die rohen Sätze, die
   bei einem zweiten Aufruf neu geparst werden: so gilt es für die getypten Abfragen von
   mikrotik-model (`DeviceCfg::fetch`, `SingleResource::fetch`) und `device::raw` gleich. Jeder
   andere Befehl auf dem Gerät leert den Cache dieses Geräts (eine Mutation, die schreibt, liest
   danach frisch).
3. **HTTP**: NetBox-REST-GETs und MikroTik-Downloads der Metadaten nach URL im Rahmen.
4. **Warten**: Wer auf eine Bedingung wartet (pollt), liest ausserhalb des Caches
   (`operation::fresh(fut)`). Fast alles Warten läuft in Jobs, die als eigene Tasks gar keinen
   Rahmen haben; im Rahmen bleiben die drei Pings der Watchdog-Ziele, die kein `print` sind.
5. **Facts**: `facts::get` startet das Fragen als eigenen Task; der Rahmen wird dorthin
   mitgegeben, damit zwei Facts derselben Operation sich die Menüs teilen. Schliesst sich eine
   zweite Operation einem laufenden Fragen an, bekommt sie dessen Ergebnis, wie heute.

Danach fallen die Handarbeiten der ersten Runde weg (`TopologyHolder::snapshot` in den
Resolvern, `update::state_from`, `leftovers::read_with_files`, `progress::root_in_ram_of`,
`wanted_packages_for`): Der Rahmen erledigt das für alle Operationen.

## Zu entscheiden

- Cache in mikrotik-api (alle Befehle, getypt und roh; Vorschlag) oder nur in
  `device::raw` (weniger Eingriff, die getypten Abfragen lesen weiter doppelt).
- Ob ein Job, den eine Mutation startet, den Rahmen erbt: Vorschlag nein, ein Job wartet und
  liest frisch.

## Prüfen

Ein Zähler der Befehle pro Gerät und Pfad pro Operation (Log auf `debug` oder Metrik): Die
Geräteseite und der Rollout-Plan im Lab dürfen jeden Pfad pro Gerät nur einmal zeigen.
