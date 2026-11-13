# Zugriff auf die Geräte

Stand 2026-11-13, besprochen mit dem Benutzer. Fasst B36 (Raw-`print` durch Modelle ersetzen)
und C17 (einmal laden pro GraphQL-Operation, früher `operation-cache.md`) zusammen: beide
hängen am selben Umbau.

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

- `MikrotikDevice<D>` (mikrotik-api) ist Verbindung und Parser in einem: eine Verbindung liest
  alle Antworten mit demselben Typ. Wer roh und getypt lesen will, braucht zwei Logins; daher
  die 68 Raw-`print`s auf `SimpleResult`-Verbindungen (B36).
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

## Befehle: was sie bewirken

```rust
pub enum Effect {
    /// liest Konfiguration oder Zustand, ohne Seiteneffekt; cachebar (`print`)
    Read,
    /// liest Live-Werte, ohne Seiteneffekt; nie gecacht (`monitor once`, `/ping`)
    Observe,
    /// ändert das Gerät (`add`, `set`, `remove`, `move`, `/execute`, `reboot`, `power-cycle`)
    Write,
    /// wirkt nur, solange seine Verbindung lebt (`device-mode/update`, `monitor` ohne `once`)
    SessionBound,
}

pub trait Command {
    const EFFECT: Effect;
    type Reply: ParsedMessage;
    fn build(&self, builder: CommandBuilder) -> CommandBuilder;
}
```

Dazu je Effekt ein Marker-Trait (`ReadCommand`, `ObserveCommand`, `WriteCommand`,
`SessionCommand`), damit eine Fassade nur die Befehle annimmt, die sie darf.

- mikrotik-model erzeugt die Befehle jedes Menüs: `print` (`Read`), `add`, `set`, `remove`,
  `move` (`Write`).
- Aktionen sind Typen eines Katalogs in mikrotik-model, nicht Strings: `Ping`,
  `PoeMonitorOnce`, `EthernetMonitorOnce` (`Observe`), `PowerCycle`, `Reboot`, `Execute`,
  `PackageUninstall`, `PartitionsCopyTo` (`Write`), `DeviceModeUpdate` (`SessionBound`), … —
  was `device::raw` heute schickt.
- Was (noch) keinen Typ hat, geht als `RawCommand { path, attributes, effect }`: auch dort ist
  die Wirkung ausdrücklich, unklassifizierte Befehle gibt es nicht.
- Fehlende Menüs bekommen ihr Modell (Stand 2026-11-13, CHR 7.24.5 gelesen): `/file`,
  `/system/scheduler`, `/system/device-mode`, `/partitions`, `/ip/neighbor`, `/system/script`,
  `/system/script/job`, `/user/ssh-keys`, `/user/active`, `/log`, `/ipv6/route`.

Idempotent und nicht idempotent (`set` gegenüber `add`) wird nicht unterschieden: nichts
reagiert darauf, automatische Wiederholung nach Verbindungsverlust ist nicht gewollt (ob ein
Befehl ankam, weiss dann niemand). Eine Retry-Regel wäre später eine Variante von `Write`.

## Verbindung und Fassaden (mikrotik-api)

- `Connection`: ein Login über einen Transport (TCP, SOCKS, SSH-Tunnel); Dispatch-Task, Tags,
  Reply-Timeout, Abbruch. Unveränderlich: kein Reconnect, kein Wechsel von Adresse oder Weg.
  Endet der Transport, antworten alle Befehle `ConnectionClosed` und `is_closed()` ist wahr.
  Ob und wohin neu verbunden wird, entscheidet der Provisioner (Wege, Passwörter, Identität,
  `WrongDevice`); ein Abbruch ist für Apply, Update und Device-Mode ein Signal.
- Die Antworten gehen roh an den Befehl (`Reply`), geparst wird mit seinem `Reply`-Typ: eine
  Verbindung trägt Befehle jedes Typs.
- Die Connection trägt den Generationszähler ihres Geräts (`Arc<AtomicU64>`, vom Provisioner
  beim Verbinden mitgegeben, mehrere Verbindungen desselben Geräts teilen ihn) und erhöht ihn
  bei jedem `Write`- und `SessionBound`-Befehl, gesteuert vom `EFFECT`, nicht vom Pfad.
- Fassaden halten die Verbindung stark (`Arc`); keine Registry mit Weak-Referenzen in der
  Bibliothek: wer die Fassade hat, hat die Verbindung, ohne zusätzlichen Fehlerfall.

Die bisherigen Methoden (`MikrotikDevice::<D>::connect`, `send_command`,
`send_simple_command`, `send_command_with_reply_timeout`, `typed`) werden `#[deprecated]`,
sobald es den Ersatz gibt, und entfernt, sobald kein Aufrufer mehr bleibt (Ziel: im selben
Durchgang, Schritt 3).

## Fassaden mit Rechten (Provisioner)

```rust
/// aus dem Pool, für Operationen und Facts: nur, was nichts ändert
pub struct Reader { … }
impl Reader {
    pub async fn read<C: ReadCommand>(&self, command: &C, freshness: Freshness) -> …; // über die Menü-Facts
    pub async fn observe<C: ObserveCommand>(&self, command: &C) -> …;              // immer live
}

/// die eigene Verbindung eines Jobs, nur gegen die Job-Sperre des Geräts
pub struct Session { … }
impl Session {
    pub fn reader(&self) -> Reader;                                    // liest frisch, ohne Cache
    pub async fn write<C: WriteCommand>(&self, command: &C) -> …;
    pub async fn hold<C: SessionCommand>(&mut self, command: &C) -> …;
}
```

- Der Pool gibt nur `Reader` heraus: eine GraphQL-Operation schreibt nie, ohne einen Job zu
  starten.
- `Session` gibt es nur mit der Sperre des Geräts (`job::start*`); schreiben ist exklusiv und
  erhöht immer die Generation. Jobs öffnen weiter eigene Verbindungen (`connect_again`): die
  Lebensdauer ihrer Verbindung hat Bedeutung (Device-Mode-Countdown, ein Apply schneidet
  seinen Weg ab, Reboot).
- `device::raw` fällt weg; was es tut, sind Befehle des Katalogs.
- `Reader` und `Session` gehören in den Provisioner, nicht in mikrotik-api: sie kennen Pool,
  Job-Sperre und Facts. Die Bibliothek gibt die Bausteine.

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

1. **mikrotik-api**: `Connection` (`is_closed`, Generationszähler), Antworten roh an den Befehl,
   `Effect`, `Command` mit den Marker-Traits, `RawCommand`; die alten Methoden `#[deprecated]`.
2. **mikrotik-model**: `Command` für `print` und die Mutationen jedes Menüs, die fehlenden
   Modelle, der Aktionskatalog; seine eigenen Zugriffe (`collect_resource`, `SingleResource`,
   das Anwenden) auf `Command`.
3. **Provisioner**: `Reader` und `Session`, jeder Zugriff umgestellt, `device::raw` und die
   alten Methoden entfernt (B36). Ab hier liest `Reader` noch ohne Cache.
4. **Provisioner**: Pool, Menü-Facts, Generation, Operationsrahmen, HTTP im Rahmen (C17).

## Prüfen

- Unit-Tests der Bibliotheken mit einem falschen Gerät über `tokio::io::duplex` (wie
  `mikrotik-api/tests/`): Befehle verschiedener Typen auf einer Verbindung, Generation steigt
  nur bei `Write`/`SessionBound`.
- CHR-Tests wie bisher (sie gehen danach über `Session`).
- Ein Zähler der Befehle pro Gerät und Pfad (Log auf `debug` oder Metrik): Geräteseite,
  Netzkarte und Rollout-Plan im Lab zeigen jeden Pfad pro Gerät und Operation einmal, und zwei
  Browser auf derselben Geräteseite einen Login und ein Lesen pro Menü.
