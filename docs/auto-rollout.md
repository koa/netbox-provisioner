# Automatisches Rollout

Was sich in NetBox ändert, erreicht die Geräte der automatischen Sites ohne Anwenden von Hand:
als ein [Gemeinsam anwenden](jobs.md#gemeinsam-anwenden) mit dem Internet als Bedingung, das
der Provisioner selbst startet (`backend::auto_rollout`). Was noch fehlt, steht im
[Konzept](concepts/auto-rollout.md) und als Aufgabe C20 in [todo.md](todo.md).

## Bereich

- Das Custom Field `auto_rollout` an der Site macht ihre RouterOS-Geräte automatisch; dasselbe
  Feld mit `false` an einem Gerät nimmt es aus ([Custom Fields](custom-fields.md#auto_rollout-site-und-device-boolean)).
- Nur mit `allow_apply`; ohne startet die Automatik nicht.
- **Einschalten:** Sieht eine Runde eine Site zum ersten Mal automatisch und hat eines ihrer
  Geräte einen Diff, geht die Site in den Fehlerzustand „nicht wie in NetBox“ mit diesen
  Geräten; angewendet wird bei ihr nichts. Sonst ginge mit der ersten Änderung alles mit, was
  sich vorher angesammelt hat. Der Admin sieht die Diffs („Abweichung von NetBox“) und hebt den
  Fehler auf, dann rollt die nächste Runde sie aus, oder er wendet von Hand an.

## Vorbedingung

Vor dem Lesen eines Geräts prüft jede Runde in NetBox (`auto_rollout::precondition`, getestet
in `precondition/test.rs`), ob jedes automatische RouterOS-Gerät aller Sites hat:

- eine Loopback IPv4 (`/32`) und IPv6 (`/128`) auf `lo`;
- eine IP-Verbindung zu den übrigen: über OSPF-Nachbarn wie im Management-Baum (zwei
  RouterOS-Geräte mit je einem `ospf`-Interface im selben IPv4-Netz), auch über Geräte
  ausserhalb des Bereichs. Eine Funkbrücke ohne RouterOS ist wie ein Kabel: Die Netze ihrer
  Enden treffen sich ohnehin. Zerfallen die Geräte in Gruppen ohne Verbindung, hat die grösste
  ihre Verbindung (bei Gleichstand die mit dem ersten Namen), jedem anderen fehlt sie.

Fehlt einem Gerät etwas, liest die Runde kein Gerät und wendet nichts an (`PreconditionUnmet`),
an keiner Site, und die Karte jeder automatischen Site zeigt, was bei welchem Gerät fehlt
(`SiteAutoRollout.unmet`). Ein Admin hebt nichts auf: Die nächste Runde (nach 5 min oder der
nächsten Änderung in NetBox) prüft wieder und läuft weiter, sobald sie erfüllt ist. Das fängt ab, was ein
versehentliches Löschen in NetBox gefährlich macht, eine gelöschte Loopback oder die letzte
Verbindung eines Geräts.

Dazu liest die Runde mit dem Diff jedes erreichten Geräts seinen Device-Mode
(`fallback::without_scheduler`): Ohne `scheduler` hätte das Anwenden keinen Fallback, und ein
Gerät, das sich abschneidet, bliebe weg, bis jemand vor Ort ist. Hat ein automatisches Gerät
ihn nicht, wendet die Runde nichts an, ebenfalls mit `PreconditionUnmet` und dem Gerät auf der
Karte; einschalten geht nur mit Bestätigung am Gerät („Device-Mode“ auf der Geräteseite). Ein
nicht erreichtes Gerät wird ohnehin zurückgestellt. Von Hand angewendet bleibt es die Warnung
`FallbackUnavailable`.

## Runden

Eine Runde (`auto_rollout::round`) liest NetBox neu und den Diff jedes automatischen Geräts
der Sites ohne Fehler, dazu den der manuellen Geräte daneben (`related`: das andere Ende eines
Kabels oder einer Funkstrecke, Controller und APs derselben WLAN-Gruppe, die Geräte mit einem
VLAN, das über VXLAN zu einem automatischen geht), vier Geräte zugleich, mit Tabellen frisch
ab dem Beginn der Runde. Der Diff ist immer der ganze Unterschied zu NetBox, also auch, was von
Hand auf ein Gerät kam: das geht mit der nächsten Runde zurück. Ob ein Diff neu ist, sagt sein
Fingerabdruck (`GeneratedCfg::fingerprint`: SHA-256 des Skripts und ob das Passwort oder das
WebFig-Zertifikat fehlt).

Wann eine Runde läuft:

- **Webhook:** NetBox meldet jede Änderung an `/netbox-webhook` (Event Rule, signiert mit
  `netbox_webhook_secret`, [Betrieb](deployment.md)). Die Runde beginnt, wenn 20 s lang keine
  weitere kam (`QUIET`): was ein Plan oder ein Admin nacheinander schreibt, geht zusammen.
- **Nach einem Schreiben des Provisioners** in NetBox (Helfer, Anwohner, Rückgängig;
  `graphql::refetch`): wie ein Webhook, ohne auf ihn zu warten; kommt er dazu, fassen die 20 s
  Ruhe beide zusammen.
- **Alle 30 min** (`EVERY`), damit eine Änderung von Hand spätestens dann zurückgeht.
- **Nach 5 min** (`RETRY`), wenn die letzte ein Gerät zurückstellte, das Internet fehlte, ein
  Gerät beschäftigt, NetBox nicht lesbar oder die Vorbedingung verletzt war.
- Gleich nach dem Start (nach 20 s) und nach „Fehler aufheben“ oder „Freigeben“.

Entschieden wird ohne Gerät (`auto_rollout::decide`, getestet in `decide/test.rs`):

- Jedes automatische Gerät mit Diff geht in die Transaktion.
- **Nicht erreichbar:** Ein Gerät, dessen Diff sich nicht lesen liess, wird zurückgestellt,
  ebenso das andere Ende jeder seiner Funkstrecken, auch über eine Kette von Strecken: eine
  Änderung an nur einer Seite kann die Strecke trennen. Die übrigen gehen ohne sie.
- **Manuelles Gerät daneben:** Hat es einen anderen Diff als in der Runde zuvor (seine
  Grundlinie), hält es die ganze Runde an, kein teilweises Rollout. Es hält an, bis es von Hand
  angewendet ist (leerer Diff) oder ein Admin entscheidet: **freigeben** (`releaseAutoRollout`),
  dann geht es mit den automatischen in dieselbe Transaktion, oder **als Stand übernehmen**
  (`acceptAutoRolloutDiff`), dann wird der Diff, mit dem es anhielt, seine Grundlinie und bleibt
  auf dem Gerät; erst ein weiter geänderter Diff hält wieder an. Einen Diff, den es schon hatte,
  behält es als Grundlinie: was ein Operator dort liegen liess, hält nichts an.
- **Automatisches Gerät mit Grund** (`auto_rollout::hold`, `HoldReason`, aus dem Diff der Runde):
  Was der Fallback nicht sicher zurücknimmt, hält die ganze Runde ebenso an, bis ein Admin den
  Diff freigibt oder von Hand anwendet:
  - **Warnungen des Anwendens:** `PoeCut` (ein Port, der etwas speist, verliert PoE),
    `CapOffOutsideWlanGroup` (`CapOff`, die Radios senden danach kein WLAN),
    `ApiClientsOutside` (die API sperrt einen verbundenen Client aus). `WatchTargetSilent` und
    `OspfAuthKept` halten nicht an: Sie sagen, was absichtlich bleibt.
  - **Richtfunk** (`RadioLink`): Das Skript ändert eine Tabelle unter `/interface/wireless`,
    `/interface/wifi` oder `/interface/w60g` (`GeneratedCfg::changes_radio`) eines Geräts mit
    einer Funkstrecke (`batch::wireless_partners`): Strecke und alles dahinter können bis zu
    24 min weg sein (DFS). Im Zweifel hält es an, auch wenn das geänderte Radio ein WLAN-Radio
    desselben Geräts ist.
  - **Umstecken** (`Rewire`, `hold::rewired`): Die Nachbar-Prüfung des Geräts
    (`neighbors::read`, nur bei einem Gerät mit Änderungen) sagt, dass ein Kabel woanders
    steckt, als NetBox es hat: Am Port des NetBox-Kabels sieht es ein anderes Gerät oder einen
    anderen Port (`CableElsewhere`, `OtherFarPort`), oder es sieht ein NetBox-Gerät an einem
    Port ohne Kabel, während das NetBox-Kabel zu diesem Gerät an einem anderen Port niemanden
    sieht. Angewendet bekäme der neue Port das Netz, bevor jemand umgesteckt hat, und der
    Fallback nähme es zurück. Sieht das Gerät den Nachbarn am neuen Port, fällt der Grund weg,
    und die nächste Runde rollt aus. Ein Nachbar an einem Port, wo NetBox gar kein Kabel hat,
    hält nichts an (ein Link, der in NetBox fehlt).
  - **Nach aussen** (`Outward`, `device::ros::outward`): OSPF trägt jede Adresse der Area
    (über `redistribute` auch die verbundenen) zu jedem Router, auch zu einem ausserhalb von
    NetBox. Nach aussen zeigt ein Port mit `internet` oder `nat` und ein OSPF-Port, dessen
    IPv4-Netze kein anderes RouterOS-Gerät von NetBox an einem OSPF-Port hat (sein Nachbar ist
    ausserhalb, wie `ether15` von lab-np2 zu srv-router-berg); ein Port ohne IPv4-Netz zählt
    nicht. Es hält an, wenn das Skript an einem solchen Port eine Adresse oder die
    OSPF-Einstellungen ändert (sein Template ohne die Liste der Ports), wenn sich
    `originate-default` oder `redistribute` einer OSPF-Instanz ändert oder eine Instanz kommt
    oder geht, oder wenn eine Adresse dazukommt, die in keinem Aggregate von NetBox liegt (dem
    eigenen Adressraum; ohne Aggregates jede). Verglichen wird das typisierte Soll mit dem
    Gerät, mit den Namen, die das Soll den Ports gibt.

  Die Karte zeigt jedes mit seinen Gründen und „Freigeben und mit ausrollen“ (kein „Als Stand
  übernehmen“: ein automatisches Gerät soll wie in NetBox sein).
- **Freigeben** gilt für genau den Diff, mit dem das Gerät anhielt (`State.released_diffs`, mit
  seinem Fingerabdruck aus `holding`): Ändert er sich bis zur nächsten Runde, hält es wieder an.

Die Transaktion ist ein Gemeinsam anwenden (`job::start_all`, `batch::apply` mit Internet und
jedem anderen automatischen Gerät als Gerät ausserhalb: was vorher antwortet, muss es danach
wieder, sonst gehen alle zurück),
im Journal „Vom automatischen Rollout gestartet“. Ist eines der Geräte mit einem anderen
Auftrag beschäftigt, startet keiner (`Busy`), die Runde kommt nach 5 min wieder. Erreicht eines
vorher das Internet nicht über IPv4 und IPv6, wird nichts angewendet (`NoInternet`); das ist
kein Fehlerzustand. Scheitert die Transaktion, gehen alle zurück, und jede Site mit einem
Gerät darin geht in den Fehlerzustand „gescheitert“ mit den Geräten, die scheiterten.

## Fehlerzustand

Pro Site: Eine Site im Fehler wird in den Runden übergangen, die anderen laufen weiter.
NetBox bleibt, wie es ist. „Fehler aufheben …“ öffnet zuerst die Vorschau: eine frische
„Abweichung von NetBox“ der Site (`SiteDiff` mit `fresh`) mit jedem Gerät, das ausrollen würde.
Erst wenn sie alle Geräte gelesen hat, geht „Fehler aufheben und ausrollen“
(`clearAutoRolloutError`, braucht `allow_apply`): Es setzt die Site auf bereit und startet gleich
eine Runde, die ihre Diffs ausrollt.

Metriken an `/metrics`, für den Alarm im Alertmanager:

- `auto_rollout_site_error{site}`: 1, solange die Site im Fehler ist;
- `auto_rollout_held_back`: 1, wenn die letzte Runde von einem manuellen Gerät angehalten wurde;
- `auto_rollout_deferred_devices`: die Geräte, die die letzte Runde zurückstellte.

## Zustand

In `state_dir/auto_rollout.json` (`saved_job::state_file`), damit ein Neustart nichts
vergisst: der Zustand jeder automatischen Site (bereit, Fehler mit Grund, Geräten und
Zeitpunkt), die Grundlinien der manuellen Geräte, die Diffs der anhaltenden (`holding`, zum
Übernehmen oder Freigeben), die freigegebenen Diffs, die letzte
Runde (Ergebnis, angewendete, zurückgestellte und anhaltende Geräte, die automatischen mit
ihren Gründen), die Geräte, an denen
sie die Vorbedingung verletzt fand (`unmet`), und die Änderungen mit dem, was sie zurücknimmt
(beim WLAN auch das alte Passwort; die Datei liest darum nur der Benutzer des Provisioners).

## Stand pro Änderung

Was der Provisioner selbst in NetBox schreibt (Speichern eines Anwohners, ein Helfer, ein
Rückgängig), hält er als Änderung fest (`auto_rollout::changes`, die neuesten 200 in
`State.changes`): wann, wer („Hans Meier“; den Anwohner nennt die Seite der Site dazu, „für
Familie Meier“), für welchen Anwohner und die
RouterOS-Geräte, deren Konfiguration sie ändert (beim Anwohner die der Vorschau, beim Helfer
die seiner Schritte; ein Rückgängig die der Änderung, die es zurücknimmt). Jede Runde notiert
pro Gerät, wie sie es sah, zum Zeitpunkt ihres Starts (`State.devices`, `changes::observed`):
im Stand von NetBox, angewendet, angehalten mit Gründen, wartend (die Runde hielt an oder
setzte aus), zurückgestellt, gescheitert. Daraus ergibt sich der Stand einer Änderung pro Gerät
(`changes::state_of`): **ausgerollt**, sobald eine Runde, die nach ihr begann, das Gerät im
Stand von NetBox sah oder es anwendete; sonst sagt die neueste Runde nach ihr, warum nicht;
vor ihr ist sie **ausstehend**. Ein manuelles Gerät bekommt sie von Hand (ausgerollt erst,
wenn eine Runde es im Stand sieht). Eine Runde, die vor der Änderung begann, zählt nicht: Sie
las die Geräte ohne sie.

Die Anwohner sehen ihre Änderungen im Tab „Änderungen“ (`changes(resident:)`), die Karte der
Site die Änderungen an ihren Geräten, die noch nicht überall sind (`SiteAutoRollout.changes`).

Beim Speichern eines Anwohners hält der Provisioner auch fest, was die Änderung zurücknimmt
(`graphql::resident::undo::undo_of`, aus der Sicht des Anwohners davor): pro Teil den Stand
davor, beim WLAN nur, was die Änderung setzte (SSID, Passwort, ein/aus). Ein Port mit dem Netz
des Betreibers und ein Gerät am Router des Betreibers haben keinen Stand, den der Anwohner
setzen darf; sie fallen weg. Eine Änderung eines Helfers oder ein Rückgängig hat nichts.

## Auf der Seite der Site

Die Karte „Automatisches Rollout“ (`components::site::auto_rollout`, nur bei einer Site mit
`auto_rollout`; `SiteAccess.autoRollout`) zeigt eine verletzte Vorbedingung mit dem, was bei
welchem Gerät fehlt, ob die Site bereit ist oder im Fehler, die
letzte Runde, die zurückgestellten Geräte und die anhaltenden manuellen, jedes mit „Freigeben
und mit ausrollen“ und „Als Stand übernehmen“, und die Änderungen, die noch nicht überall
sind, mit dem Stand pro Gerät. Sie abonniert den Zustand: jede Änderung daran meldet
`facts::Change::AutoRollout`.

## Im Lab geprüft (2026-11-17)

Site „Lab“ automatisch, lab-np2 (der Management-Proxy) ausgenommen. Die erste Runde stellte die
Site in den Fehler „nicht wie in NetBox“ (lab-wap, lab-crs226, lab-sxt-a/b: ND, www,
Multicast-Querier, WebFig-Zertifikat). Nach dem Aufheben rollte die Runde die vier gemeinsam
aus, Internet vorher und nachher über IPv4 und IPv6 erreicht. Ein Label an einem freien Port
von lab-hex in NetBox: 20 s nach dem Webhook die Runde, 53 s nach der Änderung war der
umbenannte Port auf dem Gerät; das Label gelöscht, ebenso zurück. Ein Label an einem freien
Port von lab-np2 (manuell) hielt die Runde an; im Browser als Stand übernommen, ging die
nächste Runde weiter, ohne lab-np2 anzuwenden.

## Im Lab geprüft (2026-11-23)

- **Vorbedingung:** Die IPv6-Loopback von lab-hex auf `/127` gesetzt: Die Runde las kein Gerät,
  die Karte nannte „lab-hex: Loopback IPv6 (/128 auf lo)“; zurückgesetzt, lief die nächste
  Runde weiter. Alle automatischen Lab-Geräte haben `scheduler`; den Fall ohne prüfen nur die
  Unit-Tests (Ausschalten braucht eine Bestätigung am Gerät).
- **Geräte ausserhalb:** Ein Label an `ether4` von lab-hex ging automatisch hinaus, die fünf
  anderen automatischen Geräte vorher und nachher angemeldet; ebenso zurück.
- **Richtfunk:** Ein Label am Radio von lab-sxt-a hielt die Runde an („RadioLink“, auf der
  Karte mit „Freigeben“); nicht freigegeben, das Label entfernt, dann nichts zu tun.
- **Stand pro Änderung:** Als Meier die SSID des Lab-WLANs geändert: lab-np2 (Controller,
  manuell) „von Hand anwenden“, die Runde hielt an; lab-wap (CAP, ohne eigene Änderung)
  „ausgerollt“. Ein Raum eines Geräts ohne RouterOS: „ändert nur NetBox“. Beides zurückgesetzt.
- **Umstecken:** Das Kabel von lab-crs125 zu lab-np2 in NetBox von `ether1` auf `ether2`
  gelegt (dazu ein Label an `ether3` für einen Diff): Die Runde hielt mit „Rewire“ an. Kabel
  zurück, Label weg: nichts zu tun.

## Im Lab geprüft (2026-11-24)

- **Nach aussen:** Im Lab-NetBox die freigegebenen Bereiche als Aggregates (RIR „Lab“,
  `192.168.0.0/16`, `2a02:16a:fe00:8000::/50`). Ein neues `ether4` mit Label an lab-hex ging
  automatisch hinaus. Mit dem Aggregate vorübergehend auf `192.168.0.0/17` hielt
  `192.168.200.1/24` an diesem Port die Runde an („Outward“); mit `/16` zurück ging dieselbe
  Adresse automatisch hinaus. Adresse und Port entfernt, ebenfalls automatisch.
- **Journal:** Der Eintrag eines automatischen Anwendens an lab-hex nannte „Geändert:
  /interface/ethernet ~1, /ip/address ~1, /routing/ospf/interface-template ~2“.
- **Vorschau beim Aufheben:** Lab im Zustand des lokalen Provisioners in den Fehler gesetzt (ein
  echtes Scheitern lässt sich nicht gezielt auslösen), dazu ein Label an lab-hex: Der Dialog las
  die Site frisch, „Fehler aufheben und ausrollen“ ging erst danach; aufgehoben, rollte die
  Runde lab-hex aus. Label zurück, ebenfalls automatisch.
