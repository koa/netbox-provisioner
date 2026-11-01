# Konzept: Gerätezustand

Status: Schritte 1–4 umgesetzt (`backend/src/facts.rs`), Schritte 5–7 offen (C15 in
[todo.md](../todo.md)). Abweichungen vom Konzept stehen unter
[Stand der Umsetzung](#stand-der-umsetzung).

## Ziel

Was der Provisioner über ein Gerät herausgefunden hat, soll er sich merken, solange es stimmt,
und jede Anzeige soll es sehen, sobald es nicht mehr stimmt:

- Teure Erkenntnisse (alles, was ein Gerät oder MikroTik fragt) liegen in einem zentralen Cache.
  Billige (was aus NetBox und gecachten Erkenntnissen folgt) werden jedes Mal neu gerechnet.
- Eine Aktion, die eine Erkenntnis ändern kann, verwirft sie, sobald sie fertig ist.
- Jede Erkenntnis hat einen Zeitstempel. Wer sie braucht, sagt, wie alt sie sein darf. Ist der
  Cache-Eintrag älter, wird neu gefragt.
- Der Cache hält nichts länger als 15 Minuten.
- Auch Fehlschläge sind Erkenntnisse, etwa ein verweigerter Login beim Durchprobieren von
  Zugangsdaten und Adressen.
- Eine teure Abfrage läuft nie zweimal gleichzeitig. Wer sie braucht, während sie läuft, hängt
  sich an. Wer danach kommt, nimmt sie aus dem Cache. Vorausgerechnet wird nichts.
- Anzeigen, die sich durch Aktionen ändern können, melden sich für Änderungen an und
  aktualisieren sich selbst, egal in welchem Browser die Aktion ausgelöst wurde.
- Dauer und Erfolgsrate der Abfragen gehen an Prometheus, für ein Grafana-Dashboard mit Heatmaps
  der Antwortzeiten.

## Ausgangslage

- **Kein Cache.** Jede Anfrage der Geräteseite (`DeviceOverview`) baut alles neu auf:
  - `access` verbindet neu. Antwortet die Primär-IP nicht, wartet sie jedes Mal bis zu 15 s
    (`CONNECT_TIMEOUT`) und probiert dann die Wege über die Nachbarn, jeder wieder bis zu 15 s.
  - `generateCfg` liest die ganze Konfiguration (`DeviceCfg::fetch`).
  - `routeros` holt bei MikroTik fünfmal die neueste Version (`update::newest`), für jedes Gerät
    und jeden Aufruf.
  - `leftovers`, `partitions` und `deviceMode` verbinden je noch einmal.
  - `bootstrapPlan` meldet sich bei jedem Nachbarn an (`working_plan`). Das ist dieselbe
    Erkenntnis, die `access` beim Verbinden mit dem Nachbarn ohnehin hat, nur nicht weitergegeben.
- **Vergangene Erkenntnisse gehen verloren.** Dass sat-switch nur über 1og-sw2 erreichbar ist,
  weiss der nächste Aufruf nicht mehr. Er wartet wieder erst auf die Primär-IP. Die
  Ersteinrichtung von groove-38 las den Portnamen auf sat-switch neu über die API, bekam keine
  Antwort und nahm `ether2`, obwohl die Seite sat-switch kurz vorher erreicht hatte. Ein
  verweigerter Login wird beim nächsten Aufruf wieder versucht. Das zählt gegen die
  SSH-Blacklist von RouterOS (`ssh_blacklist_time`).
- **Nur eine Stelle hält schon Zustand:** die Topologie aus NetBox (`TopologyHolder`, nach Zeit
  neu geladen, nach einem Schreiben in NetBox per `refetch`). Die Jobs (`job::JOBS`) halten ihre
  Ereignisse, und `version_before_update` liest daraus eine Erkenntnis, aber nur diese.
- **Das UI aktualisiert sich nur nach eigenen Jobs.** Die Geräteseite lädt nach `JobFinished` neu.
  Startet jemand anders einen Job oder ändert eine andere Seite NetBox, bleibt die Seite stehen.
  Alte Fehlermeldungen („Gerät nicht erreichbar“, Bootstrap-Problem) verschwinden erst beim
  Neuladen. Jobs fragt das Frontend jede Sekunde ab (`components/job.rs`, `POLL`). GraphQL hat
  keine Subscriptions (`EmptySubscription`).
- **Metriken:** `/metrics` am Management-Port hat HTTP-Metriken und `graphql_request` (Dauer pro
  Query und Anwender, `binary/src/main.rs`). Über die Abfragen an Geräte sagt es nichts.

## Begriffe

- **Erkenntnis:** ein Ergebnis, das der Provisioner von aussen holt, zum Beispiel „sat-switch
  antwortet über 1og-sw2 an 172.17.0.87“ oder „groove-38 läuft mit 7.23.8 und hat diese Pakete“.
  Ein Fehlschlag ist auch eine Erkenntnis („admin an 172.17.0.105: Login verweigert“).
- **Schlüssel:** Art der Erkenntnis plus Gerät (oder global) plus Parameter, die das Ergebnis
  ändern. Beispiel: `(Login, sat-switch, 172.17.0.87 über 1og-sw2, Zugangsdaten „tenant-x“)`.
- **Stand:** der Zeitpunkt, an dem die Abfrage **begann**. Was danach auf dem Gerät geschah, kann
  sie verpasst haben. Deshalb zählt der Beginn, nicht das Ende.
- **Alter:** wie lange der Stand her ist. Nur das Alter geht zwischen Server und UI hin und her.
- **Generation:** ein Zähler pro Gerät, der am Ende jeder Aktion steigt, die das Gerät verändern
  kann. Dazu kommt einer für die Topologie, der bei jedem Laden aus NetBox steigt.

## Zeit: absolut speichern, relativ übertragen

- **Im Speicher** ist jeder Zeitpunkt absolut auf der monotonen Uhr des Servers
  (`tokio::time::Instant`, läuft mit der Uptime). Eine verstellte Systemuhr oder NTP ändern
  daran nichts.
- **Persistiert** wird ein Zeitpunkt absolut als Zeit seit der Epoche (`SystemTime`). So macht es
  `saved_job` schon (`saved_at`, `fires`, umgerechnet mit `saved_job::instant`). Der Cache selbst
  wird nicht persistiert. Die Regel gilt für alles, was später Erkenntnisse speichert.
- **Zwischen UI und Server** nur relativ: Das UI schickt `maxAge` (Sekunden), der Server
  antwortet mit `age` (Sekunden, zum Zeitpunkt der Antwort). Die Uhren von Browser und Server
  müssen so nicht übereinstimmen. Die Anzeige „Stand vor 3 min“ zählt im Browser vom Empfang an
  weiter.

## Backend

### Der Cache (`backend/src/facts.rs`)

Ein Modul, ein globaler Speicher wie `job::JOBS`:

```rust
/// Wie alt eine Erkenntnis sein darf. Es gibt kein „egal“: Jeder Verwender sagt es.
pub enum Freshness {
    /// höchstens so alt; `MaxAge(ZERO)` fragt neu (hängt sich aber an eine eben begonnene an)
    MaxAge(Duration),
    /// begonnen nach diesem Zeitpunkt, z.B. dem Start des eigenen Jobs
    Since(Instant),
}

pub struct Fact<T> {
    pub value: Arc<Result<T, ApiError>>,
    /// Beginn der Abfrage
    pub as_of: Instant,
}

pub async fn get<T>(key: FactKey, freshness: Freshness, ask: impl Future<Output = Result<T, Error>>) -> Fact<T>;
pub fn invalidate(device: DeviceId, kinds: &[FactKind]);
pub fn changes() -> broadcast::Receiver<Change>;
```

Ein Eintrag ist entweder **fertig** (Wert oder Fehlschlag, Stand, Generation bei Beginn) oder
**laufend** (gemeinsames Future, Beginn, Generation bei Beginn). Pro Schlüssel gibt es höchstens
eine laufende Abfrage.

`get` geht so vor:

1. Gibt es einen fertigen Eintrag, der zu `freshness` passt, kommt er sofort zurück, auch ein
   Fehlschlag.
2. Läuft eine Abfrage, deren Beginn zu `freshness` passt, hängt sich der Aufrufer an. Bei
   `Since(t)` muss die laufende Abfrage nach `t` begonnen haben. Sonst wartet der Aufrufer ihr
   Ende ab und startet dann eine neue, an die sich wieder alle Späteren hängen. Zwei Abfragen mit
   demselben Schlüssel laufen also nie gleichzeitig.
3. Sonst startet `get` die Abfrage. Sie läuft auf dem Runtime des Provisioners (`RUNTIME.spawn`),
   nicht im Request: Bricht der erste Verwender ab, weil der Browser die Seite schliesst, bekommen
   die angehängten trotzdem ihr Ergebnis, und es landet im Cache.
4. Am Ende wird das Ergebnis nur gespeichert, wenn die Generation des Geräts noch dieselbe ist wie
   beim Beginn. Ist dazwischen eine Aktion fertig geworden, bekommen die Wartenden das Ergebnis,
   aber der Cache bleibt leer. Sonst überlebt ein Ergebnis von vor dem Apply.

Abgefragt wird nur, was jemand gerade braucht. Weder nach einem Job noch für angemeldete Anzeigen
rechnet der Server etwas voraus. Fragen nach einem Job mehrere Anzeigen gleichzeitig nach,
startet die erste die Abfrage und die übrigen hängen sich an (Schritt 2).

Es gibt keine Lebensdauer pro Art. Wie alt eine Antwort sinnvollerweise sein darf, weiss nur der
Verwender (siehe [Wie alt darf es sein](#wie-alt-darf-es-sein)). Der Cache hat nur eine feste
obere Grenze von **15 Minuten**. Ein Aufräumer läuft jede Minute und verwirft ältere Einträge.
Keine Anfrage bekommt etwas Älteres, auch wenn sie ein grösseres `MaxAge` nennt. Der Cache lebt
nur im Speicher. Nach einem Neustart ist er leer.

### Fehlschläge

Fehlschläge werden gespeichert wie Werte. Ob ein gespeicherter Fehlschlag gilt, entscheidet der
Verwender mit seinem `MaxAge`, wie bei jedem anderen Ergebnis. Damit sie nützen, sind sie nach ihrer
Ursache getrennt, denn die Ursachen sind verschieden endgültig:

- **Login verweigert** (`LoginRefused`, falsches Passwort): bleibt, bis jemand die Zugangsdaten
  ändert.
- **Keine Antwort** (Timeout): kann nach dem Einstecken eines Kabels eine Sekunde später anders
  sein.
- **Falsches Gerät** (`WrongDevice`): bleibt, bis jemand das Gerät oder NetBox ändert.

Das Durchprobieren in `DeviceAccess::connect` (Adressen, Wege über Nachbarn) und beim Ausprobieren
von Zugangsdaten wird damit zu einer Folge einzelner Erkenntnisse mit eigenem Schlüssel: Adresse,
Weg, Zugangsdaten. Ein Durchlauf überspringt jede Kombination, die innerhalb seines `MaxAge`
schon fehlgeschlagen ist, und probiert die zuerst, die zuletzt ging. So kostet ein totes Gerät
die 15 s nur einmal, und ein falsches Passwort wird nicht bei jedem Seitenaufruf wieder probiert.

Eine Änderung der Zugangsdaten (Konfiguration, eingegebene Ad-hoc-Daten) ändert den Schlüssel. Ein
alter Fehlschlag gilt dann gar nicht erst.

### Was gecacht wird

Teuer heisst: Es fragt ein Gerät oder MikroTik. Billig heisst: Es rechnet aus NetBox und
gecachten Erkenntnissen. Billiges wird nie gespeichert, sondern bei jedem Aufruf neu gerechnet.
So kann es nicht veralten, wenn sich NetBox oder eine Erkenntnis ändert.

| Erkenntnis | Schlüssel | verworfen nach |
|---|---|---|
| Login: geht, oder warum nicht (Identität, Weg) | Gerät, Adresse, Weg, Zugangsdaten | jedem Job auf dem Gerät, Topologie-Generation |
| Live-Konfiguration (`DeviceCfg::fetch`) | Gerät | Apply, Update, Downgrade, Cleanup, Device-Mode, Bootstrap, Reboot, Power-Cycle |
| RouterOS-Stand: Version, Pakete (`update::read_state`) | Gerät | Update, Downgrade, Cleanup, Repartition, Boot-Partition, Bootstrap |
| Neueste Version pro Kanal (`update::newest`) | global, Kanal | nichts |
| Reste (`leftovers::read`) | Gerät | jedem Job auf dem Gerät |
| Partitionen, Device-Mode | Gerät | Repartition, Boot-Partition, Device-Mode, Update |
| Nachbarn an einem Port (`/ip/neighbor`, `discovered`) | Nachbar, Port | Bootstrap, Power-Cycle des Nachbarn |
| Interfaces des Nachbarn (Portname für mac-telnet) | Nachbar | Apply auf dem Nachbarn |

`interfaceLive`, `log` und Ping wollen genau den Moment zeigen. Ihre Verwender fragen mit
`MaxAge(ZERO)`. Zwei gleichzeitige Seiten teilen sich so nur dieselbe Abfrage.

Billig und daher nie gecacht: generiertes Skript und Diff (aus Live-Konfiguration plus NetBox),
Warnungen, `bootstrap::plans`, Findings, Watchdog-Beziehungen, `wanted_packages`.

`working_plan` wird damit zur billigen Ableitung: Es fragt den Login der Nachbarn mit seinem
`MaxAge` ab. Hat die Geräteseite des Nachbarn ihn gerade erreicht, kostet es nichts mehr.

Ad-hoc-Zugangsdaten gehören in den Schlüssel als Hash, nicht im Klartext. Was mit ihnen gelesen
wurde (Konfiguration, Stand), bekommt den Zugang ebenfalls in den Schlüssel. Es soll nicht bei
einer Anzeige ohne diese Zugangsdaten auftauchen.

### Verwerfen nach Aktionen

Jede `JobKind` sagt, welche Arten von Erkenntnissen sie auf welchen Geräten verwirft (die Tabelle
oben, umgekehrt gelesen):

```rust
impl JobKind {
    fn invalidates(self) -> &'static [FactKind];
}
```

Betroffen sind das Gerät des Jobs, das Gerät, über das er läuft (`through`, z.B. der Nachbar bei
Bootstrap), und bei einem Batch jedes Gerät.

Verworfen wird **erst nach der Ausführung**, egal ob der Job gelang, fehlschlug oder abgebrochen
wurde. Dabei steigt die Generation. Was während des Jobs gelesen wurde, wird darum nicht
gespeichert (Schritt 4 von `get`). Während der Job läuft, gelten die Einträge von vorher weiter.
Die Seite zeigt ohnehin den laufenden Job.

Ein Schreiben in NetBox (Helfer, `refetch`) und jedes Laden der Topologie erhöhen die
Topologie-Generation. Login-Erkenntnisse hängen von NetBox ab (Primär-IP, Kabel, Nachbarn) und
tragen die Topologie-Generation mit. Nach einer neuen gelten sie als veraltet. Alles, was nur aus
NetBox folgt, ist billig und ohnehin neu gerechnet.

Was der Provisioner nicht sieht, etwa eine Änderung per Winbox oder einen Stromausfall, deckt das
`MaxAge` des Verwenders ab, plus der Knopf „Neu laden“, der mit `maxAge: 0` fragt.

### Wie alt darf es sein

Der Verwender entscheidet, je nachdem, wofür er die Antwort braucht:

| Verwender | Erkenntnis | `freshness` | warum |
|---|---|---|---|
| Job, der schreibt (Apply, Update, …) | Konfiguration, Stand, Reste | `Since(job_start)` | plant nie aus Altem |
| Job, der schreibt | Login | `MaxAge(1 min)` | der Weg zum Gerät darf von eben sein, der Job verbindet ohnehin selbst |
| Durchprobieren in `connect` | verweigerter Login | `MaxAge(10 min)` | ändert sich erst mit den Zugangsdaten |
| Durchprobieren in `connect` | keine Antwort | `MaxAge(20 s)` | ein Kabel kann gerade eingesteckt worden sein |
| Geräteseite | Konfiguration, Diff | `MaxAge(1 min)` | was die Seite vor einer Minute sah |
| Geräteseite | RouterOS-Stand, Partitionen | `MaxAge(10 min)` | ändert sich nur durch Jobs, und die verwerfen |
| alle | neueste Version | `MaxAge(15 min)` | MikroTik veröffentlicht selten |
| `working_plan` | Login des Nachbarn | `MaxAge(1 min)` | |
| Vorab-Prüfung von Batch und [Netzwerk-Update](network-update.md) | Login aller Geräte | `MaxAge(1 min)` | was die Seiten gerade gesehen haben |
| Live-Interfaces, Log, Ping | | `MaxAge(ZERO)` | der Moment |

Die Werte stehen beim Verwender als Konstanten mit ihrem Grund, nicht in einer zentralen
Tabelle.

GraphQL bekommt ein Argument `maxAge` (Sekunden) auf `deviceById` und an den teuren Feldern. Es
ersetzt die Vorgabe des Resolvers, darf sie aber nur verkleinern. Jedes teure Ergebnis bekommt ein
Feld `age` (Sekunden). Die Seite zeigt es als „Stand vor 3 min“, neben einem Knopf „Neu laden“
(`maxAge: 0`).

## Live-Aktualisierung

### Änderungsstrom

Der Cache meldet auf einem `broadcast`-Kanal jede gespeicherte und jede verworfene Erkenntnis:

```rust
pub struct Change {
    pub key: FactKey,          // oder ein ganzes Gerät / eine Art beim Verwerfen
    pub what: ChangeKind,      // Stored (neuer Wert da), Invalidated (Job fertig)
}
```

Auch die Jobs melden auf diesem Kanal: Start, Ende, neue Ereignisse. Die Topologie meldet ein neues
Laden aus NetBox. Ein Treffer im Cache oder ein Anhängen meldet nichts.

### Subscription mit derselben Auswahl wie die Query

Eine Anzeige abonniert **genau das, was sie sonst abfragt**: Die Subscription hat dieselbe Wurzel,
dieselben Argumente und dieselbe Auswahl wie die Query. Für die Geräteseite:

```graphql
subscription DeviceOverviewLive($id: Int!, $maxAge: Int, $target: String, $credentialName: String, $adhocCredentials: AdhocCredentials) {
    topology {
        deviceById(id: $id, maxAge: $maxAge) {
            ...DeviceOverviewFields
        }
    }
}
```

`DeviceOverviewFields` ist das Fragment, das die Query `DeviceOverview` auch benutzt. Es gibt
keine zweite Auswahl, die man mitpflegen müsste.

Im Backend ist `SubscriptionRoot::topology` ein Stream desselben `TopologyHolder`-Objekts wie in
der Query. Die Resolver darunter sind dieselben. Der Stream arbeitet so:

1. Er liefert das Objekt. async-graphql löst die Auswahl auf, und dabei schreibt jedes
   `facts::get` seinen Schlüssel in einen Sammler im Kontext der Subscription. Dasselbe tun das
   Lesen der Topologie (Topologie-Generation) und der Jobs eines Geräts. Danach weiss der Stream
   genau, wovon das gesendete Ergebnis abhing.
2. Er wartet auf eine Änderung, deren Schlüssel im Sammler steht. Andere Änderungen lassen ihn
   kalt. Der Filter ist so genau das, was die Query gelesen hat: dieselben Geräte, derselbe Zugang
   (Adresse, Zugangsdaten), dieselben Felder.
3. Er leert den Sammler und liefert das Objekt erneut. Die Resolver fragen mit ihrem `maxAge`.
   Ein eben verworfener Eintrag wird dabei neu gefragt, und mehrere Abonnenten hängen sich an
   dieselbe Abfrage. Ein gespeicherter Eintrag kommt aus dem Cache.

Die Subscription liefert also Daten, nicht nur den Hinweis auf eine Änderung. Das erste Ergebnis
ist dasselbe wie das der Query. Eine Anzeige braucht die Query nicht mehr, wenn sie live sein
soll.

Mit `maxAge` hat ein gespeicherter Eintrag ein Verfallsdatum aus Sicht der Anzeige. Der Stream
weckt sich auch, wenn das älteste gelesene Ergebnis älter als `maxAge` wird. So bleibt eine offene
Seite höchstens `maxAge` alt, ohne dass etwas vorausgerechnet wird: Gefragt wird nur, weil diese
Anzeige es gerade zeigt.

Transport ist WebSocket mit dem Protokoll `graphql-transport-ws`. `async-graphql-actix-web` kann
das (`GraphQLSubscription`). Die Anmeldung geht über das `connection_init`-Payload mit demselben
Token, das sonst im Header steht: Browser-WebSockets können keine Header setzen.

### Frontend

- Eine WebSocket-Verbindung pro Tab, als Yew-Context über der Seite (neben dem Login). Sie
  multiplext alle Subscriptions der Seite. Nach einem Abbruch baut sie sich selbst wieder auf, mit
  derselben Staffel wie `RETRY`, und startet die offenen Subscriptions neu. Das erste Ergebnis
  bringt jede Anzeige wieder auf den Stand.
- `graphql_client` erzeugt die Typen auch für Subscriptions. Die Verbindung schickt den Body wie
  bei einer Query und liest jede `next`-Nachricht als `ResponseData`.
- Eine Anzeige startet ihre Subscription beim Aufbau und beendet sie beim Abbau (`complete`).
- Fehler sind Erkenntnisse mit Stand wie alle anderen: „Gerät nicht erreichbar“ verschwindet, sobald
  eine neuere Erkenntnis da ist, nicht erst beim Neuladen der Seite.
- Laufende Jobs eines Geräts erscheinen auf seiner Seite, auch wenn ein anderer Browser sie
  gestartet hat (heute nur über `running_job` beim Laden).

Betroffene Anzeigen: Geräteseite (alle Abschnitte), Geräteliste, Interfaces und Live-Interfaces,
Site-Diff, Findings, Helfer nach einem Schreiben in NetBox.

## Metriken für Prometheus

Jede teure Abfrage geht durch `facts::get`. Dort wird gemessen, und die Metriken kommen in die
vorhandene Registry von `/metrics` am Management-Port (`binary/src/main.rs`). Das Backend bekommt
dafür die Abhängigkeit `prometheus`. `facts` legt die Metriken an, das Binary registriert sie, wie
heute `graphql_request`.

| Metrik | Typ | Labels | wofür |
|---|---|---|---|
| `device_query_duration_seconds` | Histogram | `kind`, `outcome`, `path` | Verteilung der Dauer der echten Abfragen (nicht der Treffer im Cache) |
| `device_query_total` | Counter | `device`, `kind`, `outcome` | Erfolgsrate pro Gerät: welche Geräte oft nicht antworten oder Logins verweigern |
| `fact_cache_requests_total` | Counter | `kind`, `result` | wie oft Treffer, angehängt, neu gefragt, zu alt |
| `fact_cache_entries` | Gauge | `kind` | Grösse des Caches |
| `fact_queries_running` | Gauge | `kind` | laufende Abfragen, z.B. hängende Verbindungen |

- `kind`: die Art der Erkenntnis (`login`, `config`, `routeros_state`, `newest_version`,
  `leftovers`, `partitions`, `device_mode`, `neighbors`, `interface_live`, `log`).
- `outcome`: `ok`, `refused` (Login verweigert), `timeout`, `wrong_device`, `error` (alles
  andere). Die Fehlerarten von `Error` auf diese wenigen Werte abzubilden, gehört zu `facts`.
- `path`: `routed` oder `neighbor`, damit man sieht, was der Umweg über einen Nachbarn kostet.
- `result`: `hit`, `joined`, `miss`, `too_old`.

Das Histogram hat kein Label `device`. Ein Label für den Gerätetyp kann später dazukommen, wenn
sich Typen in der Dauer unterscheiden. Pro Gerät zählt nur der Counter.

Die Buckets sind für Heatmaps fein und logarithmisch gestuft, von 25 ms bis 60 s:
0.025, 0.05, 0.1, 0.2, 0.35, 0.5, 0.75, 1, 1.5, 2, 3, 5, 7.5, 10, 15, 20, 30, 60 s. 15 s ist
`CONNECT_TIMEOUT`. Ein Haufen dort sind Timeouts, die man in der Heatmap als eigene Zeile sieht.

### Dashboard

Das Dashboard kommt als JSON nach `docs/grafana/device-queries.json`. Im Zentrum stehen die
Verteilungen, nicht einzelne Quantile: Eine Verteilung zeigt zwei Gruppen (direkt erreichbar und
über einen Nachbarn, schnelle Geräte und alte RouterBOARDs), die ein p95 zu einer Zahl verwischt.

- **Heatmap der Antwortzeiten pro Art**, eine Zeile pro `kind`, Panel-Typ Heatmap, Format
  „Heatmap“, Abfrage
  `sum by (le) (increase(device_query_duration_seconds_bucket{kind="$kind"}[$__rate_interval]))`
  mit `$kind` als Variable (Repeat).
- **Heatmap nach Weg:** dieselbe Abfrage mit `path="routed"` und `path="neighbor"` nebeneinander.
- **Heatmap der Fehlschläge:** `outcome!="ok"`. Zeigt, ob Fehler schnell kommen (verweigert) oder
  am Timeout hängen.
- **Erfolgsrate pro Art:** `sum by (kind) (rate(device_query_total{outcome="ok"}[5m])) / sum by (kind) (rate(device_query_total[5m]))`
- **Geräte mit den meisten Fehlschlägen:** `topk(10, sum by (device, outcome) (increase(device_query_total{outcome!="ok"}[1h])))`
- **Trefferquote des Caches:** `sum by (kind) (rate(fact_cache_requests_total{result=~"hit|joined"}[5m])) / sum by (kind) (rate(fact_cache_requests_total[5m]))`
- **Laufende Abfragen:** `fact_queries_running` als Zeitreihe.

## Schritte

1. **`facts`-Modul:** Speicher, `get` mit Anhängen, nie zwei gleichzeitige Abfragen pro Schlüssel,
   Generationen, Grenze von 15 Minuten, Aufräumer, Metriken. Tests mit angehaltener Tokio-Zeit
   (`tokio::time::pause`): Anhängen, `Since` wartet eine ältere laufende Abfrage ab statt
   parallel zu fragen, Ergebnis nach einer Generation nicht gespeichert, Fehlschläge gelten nach
   `MaxAge`, Grenze.
2. **Login und Durchprobieren:** `DeviceAccess::connect` probiert über `facts` (Adresse, Weg,
   Zugangsdaten). Es überspringt frische Fehlschläge und nimmt zuerst, was zuletzt ging.
   `working_plan` und `tunnel` lesen den Login der Nachbarn aus dem Cache. Das ist der grösste
   Gewinn: keine 15-s-Wartezeiten bei jedem Aufruf und keine wiederholten falschen Logins mehr.
   Danach ist das Dashboard schon sinnvoll.
3. **Verwerfen nach Jobs:** `JobKind::invalidates`, Verwerfen am Ende, Topologie-Generation bei
   `refetch` und beim Laden nach Zeit.
4. **Die übrigen Erkenntnisse der Tabelle,** jede mit ihren Verwendern und deren `MaxAge`, und mit
   Test für ihr Verwerfen.
5. **`maxAge` und `age` in GraphQL,** „Stand“ und „Neu laden“ auf der Geräteseite.
6. **Subscriptions:** Sammler der gelesenen Schlüssel, Stream je Wurzel, WebSocket im Binary,
   Verbindung im Frontend. Die Geräteseite zuerst, dann die übrigen Anzeigen.
7. **Optional:** Job-Ereignisse über eine Subscription statt jede Sekunde abfragen.

Schritte 1–4 ändern am UI nichts und lassen sich einzeln prüfen, im Lab und auf dem CHR. Danach
macht Schritt 5 den Cache sichtbar, Schritt 6 löst die stehen gebliebenen Anzeigen.

## Stand der Umsetzung

Umgesetzt sind Cache, Login und Durchprobieren, Verwerfen nach Jobs und die Erkenntnisse der
Geräteseite, mit den Metriken. Abweichend vom Konzept:

- **Durchprobieren als eigene Erkenntnis** (`FactKind::Route`): Das ganze Durchprobieren von
  `DeviceAccess::connect` läuft für ein Gerät und einen Zugang nur einmal gleichzeitig. Es wird
  nicht gespeichert (`MaxAge(ZERO)`), die einzelnen Logins schon.
- **Der Login hält die Verbindung:** Ein Login, der vor höchstens einer Minute ging, gibt seine
  Verbindung wieder. Vorher prüft ein `/system/identity` sie, eine tote wird verworfen und neu
  aufgebaut.
- **Generierte Konfiguration statt Live-Konfiguration** (`FactKind::Config`): gespeichert wird
  das Ergebnis von `generateCfg`, also mit den Live-Prüfungen (Watchdog-Adressen, API-Clients,
  PoE). Dazu werden RouterOS-Stand (mit den gewünschten Paketen) und Device-Mode aus NetBox
  abgeleitet. Ändert sich die Topologie, verwirft der Cache diese drei Arten für alle Geräte
  (`facts::invalidate_everywhere`).
- **Topologie vergleichbar:** Damit ein neues Laden aus NetBox ohne Änderung nicht als Änderung
  gilt, sortiert `fetch` die Listen eines Prefix (Kinder, Bereiche, Adressen). Sie kamen aus
  einer Map und hatten bei jedem Laden eine andere Reihenfolge.
- **`maxAge`** steht an `access(maxAge:)`, **`age`** an `generateCfg` und `routeros`.
- **Vorschau eines Helfers** (`generating_for`): Ihre Erkenntnisse haben einen eigenen Schlüssel
  und werden nicht mit denen des echten Geräts geteilt.

## Offene Fragen

- Die `MaxAge`-Werte der Verwender sind ein Startpunkt. Sie werden angepasst, wenn es damit
  Probleme gibt.
- Wer darf welche Änderungen sehen, wenn es einmal Rollen gibt ([self-service.md](self-service.md))?
  Die Subscription läuft durch dieselben Resolver wie die Query. Was die Query einem Anwender
  verweigert, verweigert sie also auch.
