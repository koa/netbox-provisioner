# Gerätezustand

Was der Provisioner über ein Gerät herausfindet, merkt er sich, solange es stimmt, und jede
Anzeige sieht es, sobald es nicht mehr stimmt:

- Teure Erkenntnisse (alles, was ein Gerät oder MikroTik fragt) liegen in einem zentralen Cache
  (`backend/src/facts.rs`). Billige (was aus NetBox und gecachten Erkenntnissen folgt) werden
  jedes Mal neu gerechnet.
- Eine Aktion, die eine Erkenntnis ändern kann, verwirft sie, sobald sie fertig ist.
- Jede Erkenntnis hat einen Stand. Wer sie braucht, sagt, wie alt sie sein darf; ist sie älter,
  wird neu gefragt. Der Cache hält nichts länger als 15 Minuten.
- Auch Fehlschläge sind Erkenntnisse, etwa ein verweigerter Login beim Durchprobieren von
  Zugangsdaten und Adressen.
- Eine teure Abfrage läuft nie zweimal gleichzeitig. Wer sie braucht, während sie läuft, hängt
  sich an; wer danach kommt, nimmt sie aus dem Cache. Vorausgerechnet wird nichts.
- Anzeigen, die sich durch Aktionen ändern können, abonnieren sie und aktualisieren sich selbst,
  egal in welchem Browser die Aktion ausgelöst wurde.
- Dauer und Erfolg der Abfragen gehen an Prometheus, mit einem Grafana-Dashboard.

## Begriffe

- **Erkenntnis** (Fakt): ein Ergebnis, das der Provisioner von aussen holt, zum Beispiel
  „sat-switch antwortet über 1og-sw2 an 172.17.0.87“ oder „groove-38 läuft mit 7.23.8 und hat
  diese Pakete“. Ein Fehlschlag ist auch eine Erkenntnis („admin an 172.17.0.105: Login
  verweigert“).
- **Schlüssel** (`FactKey`): Art (`FactKind`) plus Gerät (oder global) plus die Parameter, die
  das Ergebnis ändern, etwa Adresse, Weg und Zugangsdaten eines Logins. Der Weg, über den ein
  Fakt gefragt wurde (`path`), ist nur ein Label der Metriken und gehört nicht dazu, welcher
  Fakt es ist.
- **Stand:** der Zeitpunkt, an dem die Abfrage **begann**. Was danach auf dem Gerät geschah, kann
  sie verpasst haben.
- **Alter:** wie lange der Stand her ist. Nur das Alter geht zwischen Server und UI hin und her.
- **Generation:** ein Zähler pro Gerät, der am Ende jeder Aktion steigt, die das Gerät verändern
  kann, und einer für die Topologie, der bei jeder Änderung in NetBox steigt.

## Zeit: absolut speichern, relativ übertragen

- **Im Speicher** ist jeder Zeitpunkt absolut auf der monotonen Uhr des Servers
  (`tokio::time::Instant`). Eine verstellte Systemuhr oder NTP ändern daran nichts.
- **Persistiert** wird ein Zeitpunkt als Zeit seit der Epoche (`SystemTime`), wie bei
  `saved_job`. Der Cache selbst wird nicht persistiert; nach einem Neustart ist er leer.
- **Zwischen UI und Server** nur relativ: Das UI schickt `maxAge` (Sekunden), der Server
  antwortet mit `age` (Sekunden, zum Zeitpunkt der Antwort). Die Uhren von Browser und Server
  müssen so nicht übereinstimmen. Die Seite rechnet daraus die Uhrzeit und zeigt sie als
  „Stand 14:03:12“; eine neue Antwort innerhalb von 1,5 s derselben Uhrzeit behält die alte, damit
  die Anzeige nicht um eine Sekunde springt (`same_reading`).

## Der Cache

```rust
pub enum Freshness {
    /// höchstens so alt; `MaxAge(ZERO)` fragt neu (hängt sich aber an eine eben begonnene an)
    MaxAge(Duration),
    /// begonnen nach diesem Zeitpunkt, z.B. dem Start des eigenen Jobs
    Since(Instant),
}

pub async fn get<T>(key: FactKey, freshness: Freshness, ask: impl Future<Output = Result<T, Error>>) -> Fact<T>;
```

Ein Eintrag ist **fertig** (Wert oder Fehlschlag, Stand) oder **laufend** (gemeinsames Future,
Beginn, Generation bei Beginn). `get` geht so vor:

1. Ein fertiger Eintrag, der zu `freshness` passt, kommt sofort zurück, auch ein Fehlschlag.
2. Läuft eine Abfrage, deren Beginn passt, hängt sich der Aufrufer an. Passt sie nicht (bei
   `Since(t)` vor `t` begonnen), wartet er ihr Ende ab und startet dann eine neue. Zwei Abfragen
   mit demselben Schlüssel laufen nie gleichzeitig.
3. Sonst startet `get` die Abfrage auf dem Runtime des Provisioners, nicht im Request: Schliesst
   der erste Verwender die Seite, bekommen die angehängten trotzdem ihr Ergebnis.
4. Gespeichert wird nur, wenn die Generation noch dieselbe ist wie beim Beginn. Ist dazwischen
   eine Aktion fertig geworden, bekommen die Wartenden das Ergebnis, der Cache bleibt leer.

Eine Lebensdauer pro Art gibt es nicht; wie alt eine Antwort sein darf, weiss nur der Verwender.
Die obere Grenze von 15 Minuten (`LIMIT`) setzt ein Aufräumer jede Minute durch, und keine
Anfrage bekommt etwas Älteres, auch mit grösserem `maxAge`.

`get_judged` lässt den Verwender zusätzlich über einen gefundenen Wert urteilen (der Login, der
seine Verbindung nur gibt, wenn sie noch antwortet). `peek` und `peek_newest` schauen nach, ohne
zu fragen; `dropped` sagt, seit wann die Erkenntnisse eines Geräts verworfen sind. `put` legt
einen Fakt ab, den ein Job ohne eigene Abfrage kennt (siehe
[Abweichung von NetBox](#abweichung-von-netbox)).

### Fehlschläge

Fehlschläge werden gespeichert wie Werte, nach ihrer Ursache getrennt, weil die Ursachen
verschieden endgültig sind: ein verweigerter Login bleibt, bis jemand die Zugangsdaten ändert;
keine Antwort kann eine Sekunde später anders sein; ein falsches Gerät bleibt, bis jemand Gerät
oder NetBox ändert. Ob ein gespeicherter Fehlschlag gilt, entscheidet der Verwender mit seinem
Alter.

### Login und Durchprobieren

`DeviceAccess::connect` probiert Adressen, Wege über Nachbarn und Zugangsdaten als einzelne
Login-Erkenntnisse. Ein Durchlauf überspringt jede Kombination, die eben fehlschlug, und probiert
die zuerst, die zuletzt ging. So kostet ein totes Gerät die 15 s (`CONNECT_TIMEOUT`) nur einmal,
und ein falsches Passwort wird nicht bei jedem Seitenaufruf wieder probiert (es zählt gegen die
SSH-Blacklist von RouterOS).

- Das ganze Durchprobieren ist selbst ein Fakt (`FactKind::Route`): Es läuft für ein Gerät und
  einen Zugang nur einmal gleichzeitig, wird aber nicht gespeichert (`MaxAge(ZERO)`).
- Ein Login, der vor höchstens einer Minute ging, gibt seine Verbindung wieder. Vorher prüft ein
  `/system/identity` sie; eine tote wird verworfen und neu aufgebaut.
- `working_plan` und `tunnel` lesen den Login der Nachbarn aus dem Cache: Hat die Seite des
  Nachbarn ihn gerade erreicht, kostet es nichts mehr.
- Andere Zugangsdaten ändern den Schlüssel; ein alter Fehlschlag gilt dann gar nicht erst.
  Ad-hoc-Zugangsdaten stehen als Hash im Schlüssel, und was mit ihnen gelesen wurde, trägt den
  Zugang ebenfalls im Schlüssel.
- Die Vorschau eines Helfers (`generating_for`) hat eigene Schlüssel und teilt nichts mit dem
  echten Gerät.

### Was gecacht wird

| Erkenntnis (`FactKind`) | Schlüssel | verworfen nach |
|---|---|---|
| `Login`: geht, oder warum nicht | Gerät, Adresse, Weg, Zugangsdaten | jedem Job auf dem Gerät, auch über es; Topologie-Generation |
| `Route`: das Durchprobieren | Gerät, Zugang | wird nicht gespeichert |
| `Config`: die generierte Konfiguration mit Diff und Live-Prüfungen (`generateCfg`) | Gerät, Zugang | Apply, Update, Downgrade, Cleanup, Device-Mode, Bootstrap, Reboot, Power-Cycle; jede Änderung in NetBox |
| `RouterosState`: Version, Pakete (`update::read_state`) | Gerät | Update, Downgrade, Cleanup, Repartition, Boot-Partition, Bootstrap; jede Änderung in NetBox |
| `NewestVersion` pro Kanal (`update::newest`) | global, Kanal | nichts |
| `Leftovers` (`leftovers::read`) | Gerät | jedem Job auf dem Gerät |
| `Partitions`, `DeviceMode` | Gerät | Repartition, Boot-Partition, Device-Mode, Update; `DeviceMode` auch jede Änderung in NetBox |
| `Neighbors`: was das Gerät sieht (`neighbors::read_table`), oder an einem Port (Bootstrap) | Gerät, Port | jedem Job auf dem Gerät, auch über es |
| `Interfaces` des Nachbarn (Portnamen für mac-telnet) | Nachbar | Apply auf dem Nachbarn |
| `InterfaceLive`: die Live-Prüfung der Interfaces | Gerät | jedem Job auf dem Gerät, auch über es; jede Änderung in NetBox |

Weil `Config`, `RouterosState` und `DeviceMode` gewünschte Pakete und Device-Mode aus NetBox
mitrechnen, verwirft eine Änderung der Topologie sie für alle Geräte
(`facts::invalidate_everywhere`), ebenso `InterfaceLive`, das gegen NetBox prüft. Damit ein neues
Laden ohne Änderung nicht als Änderung gilt, sortiert `fetch` die Listen eines Prefix.

Billig und daher nie gecacht: Warnungen, `bootstrap::plans`, Befunde, Watchdog-Beziehungen,
`wanted_packages`, der Vergleich des Gelesenen mit NetBox.

### Verwerfen nach Aktionen

Jede `JobKind` sagt, welche Arten sie verwirft (`JobKind::invalidates`, die Tabelle oben
umgekehrt gelesen); jeder Job auf einem Gerät verwirft `Neighbors` und `InterfaceLive`, das
Gerät, über das er läuft (`through`), verliert `Neighbors`, `InterfaceLive` und `Login`
(`THROUGH_INVALIDATES`), ein Batch betrifft jedes seiner Geräte.

Verworfen wird **nach der Ausführung**, egal ob der Job gelang, fehlschlug oder abgebrochen
wurde; dabei steigt die Generation, und was während des Jobs gelesen wurde, wird nicht
gespeichert. Während der Job läuft, gelten die Einträge von vorher weiter.

Was der Provisioner nicht sieht (Winbox, Stromausfall), deckt das Alter des Verwenders ab, plus
„Neu laden“: Es verwirft die Erkenntnisse des Geräts (`refreshDevice`), damit alle Seiten, die
es zeigen, neu fragen. „Nochmals lesen“ im Vergleich mit den Geräten (`refreshNeighbors`) und
„Erneut prüfen“ der Live-Prüfung (`refreshInterfaceLive`) verwerfen nur ihren Fakt.

### Wie alt darf es sein

Der Verwender entscheidet; die Werte stehen bei ihm als Konstanten mit ihrem Grund:

| Verwender | Erkenntnis | `freshness` | warum |
|---|---|---|---|
| Job, der schreibt (Apply, Update, …) | Konfiguration, Stand, Reste | `Since(job_start)` | plant nie aus Altem |
| Durchprobieren | Login ging (`LOGIN_WENT`) | 1 min | die Verbindung von eben |
| Durchprobieren | Login verweigert (`LOGIN_REFUSED`) | 10 min | ändert sich erst mit den Zugangsdaten |
| Durchprobieren | keine Antwort (`LOGIN_NO_ANSWER`) | 20 s | ein Kabel kann gerade eingesteckt worden sein |
| Geräteseite | Konfiguration, Diff (`CONFIG_AGE`), Reste | 1 min | was die Seite vor einer Minute sah |
| Geräteseite | RouterOS-Stand, Partitionen, Device-Mode | 10 min | ändert sich nur durch Jobs, und die verwerfen |
| alle | neueste Version (`NEWEST_AGE`) | 15 min | MikroTik veröffentlicht selten |
| Ersteinrichtung | Nachbarn am Port (`DISCOVERED_AGE`) | 10 s | das neue Gerät wird gerade eingesteckt |
| Ersteinrichtung | Portnamen des Nachbarn (`PORT_NAMES_AGE`) | 10 min | ändern sich nur mit einem Apply dort |
| „Vergleich mit den Geräten“ | was das Gerät sieht (`NEIGHBORS_AGE`) | 10 min | ein Kabel wandert selten |
| Live-Prüfung der Interfaces (`INTERFACE_LIVE_AGE`) | | 1 min | mit „Stand“; Jobs und NetBox verwerfen |
| Log, Ping | | `MaxAge(ZERO)` | der Moment |

GraphQL nimmt `maxAge` (Sekunden) an `access(maxAge:)`; es darf die Vorgabe des Resolvers nur
verkleinern. `generateCfg`, `routeros`, `neighborsAge` und `interfaceLiveAge` geben das Alter
(`age`). Die Werte sind ein Startpunkt und werden angepasst, wenn es damit Probleme gibt.

## Live-Aktualisierung

### Änderungsstrom

Der Cache meldet auf einem `broadcast`-Kanal, nummeriert (`facts::changes`, `facts::sequence`):

```rust
pub enum Change {
    Stored(FactKey),                            // neu gespeichert
    Invalidated(DeviceId, &'static [FactKind]), // nach einem Job verworfen
    Job(DeviceId),                              // Job gestartet, neues Ereignis, Ende
    Topology,                                   // NetBox hat sich geändert
}
```

Ein neu gespeicherter Wert betrifft nur, wer ohne zu fragen nachschaut (`peek_newest`). Wer
fragt, fragt ohnehin neu, wenn der Wert älter wird als gezeigt; zwei offene Seiten würden sich
sonst gegenseitig zum Neufragen bringen.

### Subscription mit denselben Typen wie die Query

`SubscriptionAuthenticated` (`backend/src/graphql/live.rs`) hat ein Wurzelfeld
`topology: TopologyHolder!`, mit denselben Typen wie die Query. Eine Anzeige abonniert so genau
die Auswahl, die sie sonst abfragt, und die Resolver darunter sind dieselben:

```graphql
subscription DeviceOverview($id: Int!, $target: String, ...) {
    topology { deviceById(id: $id) { name jobs { id state } access(target: $target) { ... } } }
}
```

Das Gerät hat dafür ein Feld `jobs` (dasselbe wie `jobs(deviceId:)` der Query), `topology` ein
Feld `netboxUrl`. Jedes Feld der Subscription ist ein Stream aus `live::rounds`:

1. Er gibt das Objekt. Beim Auflösen schreibt jedes `facts::get` Schlüssel und Alter in einen
   Sammler (`Reading`), das Feld `jobs` das Gerät. Den Sammler setzt die Extension
   `LiveExtension` für jedes Pollen des Streams (`facts::reading_in`).
2. Er wartet auf eine Änderung, die den Sammler betrifft (`Reading::concerns`): verworfene
   Erkenntnisse, die gelesen wurden, ein Job eines gelesenen Geräts, die Topologie; oder darauf,
   dass die jüngste gelesene Erkenntnis älter wird als erlaubt (frühestens nach 5 s; `maxAge: 0`
   weckt nicht). Dann wartet er 300 ms auf weitere Änderungen, weil ein Job mehrere auf einmal
   meldet.
3. Er leert den Sammler und gibt das Objekt erneut; die Resolver fragen mit ihrem Alter, mehrere
   offene Seiten hängen sich an dieselbe Abfrage.

Über HTTP (`/graphql`) beantwortet der Provisioner eine Subscription mit ihrem ersten Ergebnis
(`live::execute_once`). Eine Anzeige, die nur einmal fragt (der Helfer „Flags“ mit
`findings(after:)`), nutzt so dasselbe Dokument und dieselben generierten Typen.

Solange eine Subscription offen ist, liest der Provisioner NetBox alle 10 s
(`live::watch_netbox`), höchstens einmal gleichzeitig (`TopologyHolder::fetching`). Was NetBox
ändert, zeigen die Anzeigen so nach etwa 10 s.

Transport ist WebSocket an `/graphql/ws` mit dem Protokoll `graphql-transport-ws`. Die Anmeldung
geht über das `connection_init`-Payload (`{"token": …}`), weil Browser-WebSockets keine Header
setzen können; das Binary prüft das Token wie den Header von `/graphql`.

### Frontend

- Eine WebSocket-Verbindung pro Tab (`frontend/src/graphql/live.rs`) für alle Subscriptions der
  Seite. Sie öffnet sich mit der ersten und baut sich nach einem Abbruch mit der Staffel von
  `RETRY` und dem dann gültigen Token wieder auf; die offenen Subscriptions startet sie neu.
- `watch(scope, operation, deliver)` gibt eine `Subscription`; fällt sie weg, endet sie
  (`complete`). Antworten kommen als `graphql::Response` wie über HTTP.
- Fehler sind Erkenntnisse mit Stand wie alle anderen: „Gerät nicht erreichbar“ verschwindet,
  sobald eine neuere Erkenntnis da ist.
- Laufende Jobs eines Geräts erscheinen auf seiner Seite, auch wenn ein anderer Browser sie
  gestartet hat. Endet einer, liest die Seite Verlauf und Interfaces neu.

Abonniert sind die Übersicht der Geräteseite (`DeviceOverview`) mit „Stand“ und „Neu laden“,
ihre Interfaces (`InterfaceView`) mit der Live-Prüfung (`InterfaceLive`) und ihrem „Stand“, die
Befunde (`Findings`), die Site-Liste (`ListSites`), die Geräte einer Site (`SiteDevices`) und
„Abweichung von NetBox“ (`SiteDiffKnown`).

„Vergleich mit den Geräten“ (`NeighborFindings`) abonniert nach dem Klick jedes Gerät. Gecacht
ist nur, was das Gerät antwortet; verglichen wird mit NetBox, wie es jetzt ist. Ein in NetBox
übernommenes Kabel nimmt seinen Befund so ohne neues Lesen weg, mit dem „Stand“ des Lesens
daneben. Was die Gegenseite eines umbenannten Ports sieht, bleibt bis zu ihrem nächsten Lesen
alt (das Gerät meldet den neuen Namen ohnehin erst mit der nächsten Ankündigung).

Die Job-Ansicht abonniert ihren Job (`JobEvents`, Wurzelfeld `job(id)`), jede Antwort mit den
neuen Ereignissen ([Jobs](jobs.md#verfolgen-im-browser)). Nur „Abweichung von NetBox“ fragt die
Ereignisse ihres Prüf-Jobs noch jede Sekunde ab (`components/site/site_diff.rs`).

### Abweichung von NetBox

Die Karte „Abweichung von NetBox“ einer Site zeigt die Ereignisse ihres letzten Prüf-Jobs
(`diffSite`). Der Job liest jedes Gerät über denselben Fakt wie die Geräteseite (`Config`, mit
`Since` seinem Start); der Fakt trägt dafür die Änderungen pro Menü. Daneben abonniert die Karte
`knownDiff` der Geräte: das Neueste, was der Provisioner weiss, ohne zu fragen, und seit wann
das Gelesene verworfen ist (`facts::dropped`). Pro Gerät zeigt sie, was nach dem Start des Jobs
gelesen wurde, sonst das Ergebnis des Jobs, oder „seit der Prüfung verändert“, wenn es danach
verworfen wurde.

Ein Apply, dessen Kontrolle das Gerät wie NetBox fand (ohne Fallback danach), legt das an seinem
Ende als Fakt ab, nach dem Verwerfen (`facts::put`, `Job::keep_in_line`): Die Karte zeigt
„entspricht NetBox“ auch ohne offene Geräteseite. Bleibt nach der Kontrolle etwas übrig, legt es
nichts ab (die Änderungen pro Menü kennt die Kontrolle nicht), und die Karte zeigt „seit der
Prüfung verändert“.

## Metriken

Jede teure Abfrage geht durch `facts::get` und wird dort gemessen. Die Metriken legt `facts`
an, das Binary registriert sie in der Registry von `/metrics` am Management-Port
(`facts::register`).

| Metrik | Typ | Labels | wofür |
|---|---|---|---|
| `device_query_duration_seconds` | Histogram | `kind`, `outcome`, `path` | Verteilung der Dauer der echten Abfragen (nicht der Treffer im Cache) |
| `device_query_total` | Counter | `device`, `kind`, `outcome` | Erfolgsrate pro Gerät (NetBox-ID) |
| `fact_cache_requests_total` | Counter | `kind`, `result` | Treffer, angehängt, neu gefragt |
| `fact_cache_entries` | Gauge | `kind` | Grösse des Caches |
| `fact_queries_running` | Gauge | `kind` | laufende Abfragen, z.B. hängende Verbindungen |

- `kind`: die Art in snake_case (`login`, `route`, `config`, `routeros_state`,
  `newest_version`, `leftovers`, `partitions`, `device_mode`, `neighbors`, `interfaces`,
  `interface_live`).
- `outcome`: `ok`, `refused` (Login verweigert), `timeout`, `wrong_device`, `error` (alles
  andere). Jede dieser Zeitreihen entsteht mit 0, sobald eine Art über einen Weg zum ersten Mal
  gefragt wird: Prometheus' `increase` zählt den ersten Wert einer neuen Zeitreihe nicht, und der
  erste Fehlschlag fehlte sonst.
- `path`: `routed` oder `neighbor`, damit man sieht, was der Umweg über einen Nachbarn kostet;
  leer für Abfragen, die an keinem Gerät hängen (`route`, `newest_version`).
- `result`: `hit`, `joined`, `miss`.

Das Histogram hat kein Label `device`, pro Gerät zählt nur der Counter. Die Buckets sind für
Heatmaps fein und logarithmisch gestuft, von 25 ms bis 60 s (0.025, 0.05, 0.1, 0.2, 0.35, 0.5,
0.75, 1, 1.5, 2, 3, 5, 7.5, 10, 15, 20, 30, 60 s). 15 s ist `CONNECT_TIMEOUT`; ein Haufen dort
sind Timeouts.

### Dashboard

Das Dashboard liegt als JSON im Helm-Chart,
[dashboards/device-queries.json](../charts/netbox-provisioner/dashboards/device-queries.json).
Mit `grafanaDashboard.enabled` legt das Chart es als ConfigMap für den Grafana-Sidecar an, und
jedes Upgrade bringt es auf den Stand ([Deployment](deployment.md#helm-chart)); ohne Chart geht
es über Dashboards → Import. Geprüft mit Grafana 13.3 und Prometheus 3.15. Im Zentrum stehen die Verteilungen, nicht einzelne Quantile: Eine Verteilung zeigt zwei
Gruppen (direkt erreichbar und über einen Nachbarn, schnelle Geräte und alte RouterBOARDs), die
ein p95 zu einer Zahl verwischt.

- **Überblick:** Erfolgsrate pro Art, Trefferquote des Caches (`hit` und `joined`), laufende
  Abfragen, Fakten im Cache, Abfragen pro Minute nach Ergebnis.
- **Antwortzeiten:** eine Heatmap pro Art (Repeat über `$kind`), je eine nach Weg (`routed`,
  `neighbor`), eine der Fehlschläge (kommen sie schnell, verweigert, oder am Timeout), und die
  Tabelle der Geräte mit den meisten Fehlschlägen der letzten Stunde.

Die Heatmaps nehmen das Farbschema Spectral, ohne Abstand zwischen den Zellen. Die Variable
`$kind` (Mehrfachwahl, „All“) filtert alle Panels.
