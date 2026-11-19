# Aufgaben

Alle offenen Aufgaben über die drei Repositories (netbox-provisioner, mikrotik-model,
mikrotik-api), gesammelt aus [pending.md](pending.md), den [Konzepten](concepts/) und der
Durchsichten des Codes vom 2026-10-19 und 2026-11-06. Die Nummern bleiben, auch wenn umsortiert wird; eine
erledigte Aufgabe wandert mit Datum und Commit nach [done.md](done.md), ein erledigter Teil
ebenso, der Rest bleibt hier.

Aufwand grob: **S** Stunden, **M** ein bis zwei Tage, **L** mehr.
Status: **offen**, **in Arbeit**.

## Reihenfolge

Nach C4 (cynic) nochmals durchgesehen (2026-11-06, dieselben Lints): Das Frontend hat rund
2500 Zeilen mehr, weil die Abfragen jetzt Structs sind, dafür geteilte Typen (`types`) und keine
nennenswerten Doppelungen (`cancelJob` nahm noch einen eigenen Typ, behoben). Die langen
Funktionen sind dieselben wie vorher; B15 bis B23 bleiben, wie sie sind, B14 ist im Frontend
kleiner geworden.

## Entscheidungen

Fragen aus den Konzepten und der Durchsicht, mit der Antwort und den Aufgaben, die davon
abhängen. Eine neue Frage kommt mit Status **offen** dazu; entschieden, wird sie mit Datum
markiert und ins Konzept übernommen (dort unter „Entschieden“); sind alle Aufgaben einer
Entscheidung erledigt, wandert sie nach [done.md](done.md#entscheidungen).

| # | Frage | Entscheidung | betrifft |
|---|---|---|---|
| Z4 | Freigaben der internen Firewall: Custom Fields am Interface oder Prefix-Tags? | Custom Fields am Interface (2026-10-23) | C3 |
| Z7 | Befehle ohne Strings: lohnt Stufe 2? | vorerst nur Stufe 1, Stufe 2 nicht geplant (2026-10-23) | C5 |
| Z10 | Anwohner als Tenant oder Contact Group, und wo stehen seine Benutzer? | weder noch: ein NetBox-Owner mit seinen NetBox-Benutzern in `users` (Login über Pocket-ID), in der Pflicht-Owner-Group „Anwohner“; Mitglieder werden in NetBox gepflegt, damit dieselbe Zuordnung die Leserechte in NetBox steuert (in der Lab-NetBox geprüft, 2026-10-23) | C7 |
| Z11 | Ports eines Anwohners an einem fremden Switch? | NetBox-Feld `owner` am Interface, kein Custom Field (2026-10-23) | C7 |
| Z12 | Apply eines Anwohners sofort oder mit Admin-Freigabe? | sofort (2026-10-23); abgelöst durch Z46: der Anwohner schreibt nur NetBox, die Geräte erreicht es über das automatische Rollout | C7 |
| Z13 | Alarme im Provisioner oder über Prometheus? | nur Prometheus/Alertmanager (2026-10-23) | C8 |
| Z14 | Ziel des Uplink-Pings? | das Gateway des Uplinks (2026-10-23) | C8 |
| Z15 | Wo liegen die Positionen der Netz-Karte? | Admins publizieren das offizielle Layout in ein NetBox Custom Field; andere nehmen es oder speichern Änderungen im Browser (2026-10-23) | C9 |
| Z16 | Grenzwerte für Health? | keine: Health nicht beurteilen, nur anzeigen (Hover/Popup); Farben nur nach Erreichbarkeit (2026-10-23) | C8, C9 |
| Z17 | Power Ports in NetBox modellieren für `psuN-state`? | entfällt mit Z16 (2026-10-23) | C9 |
| Z18 | Pilot eines Netz-Updates? | Vorschlag des Provisioners, im Plan änderbar (2026-10-23) | C10 |
| Z19 | Wartungsfenster? | vorerst nicht (2026-10-23) | C10 |
| Z20 | Netz-Update pro Site oder über mehrere? | mehrere Sites, frei gewählt (2026-10-23) | C10 |
| Z35 | Kunden-Netz mit DHCP-Range (E6): Welche Grenzen soll die Range bekommen? Der ganze freie Rest wäre dasselbe wie heute ohne Range. Vorschlag: im Formular ein Häkchen „DHCP-Range anlegen“ mit Start und Ende, vorgegeben die obere Hälfte des Netzes (bei einem `/24` `.128`–`.254`), die untere bleibt für feste Adressen. Oder lieber feste Anzahl Adressen am Anfang frei (z.B. die ersten 20)? | Häkchen mit Start und Ende, vorgegeben die obere Hälfte des Netzes, änderbar; eine feste Anzahl liesse in kleinen Netzen kaum Range übrig (2026-11-06) | E6 |
| Z36 | Watchdog-Befunde (E1): Geräte ohne `ospf` (APs, Switches, CAPs) haben nie einen Parent; eine Warnung „kein Weg zu einer Wurzel“ für jedes wäre Rauschen. Warnen nur bei Geräten mit `ospf`-Interface, aber ohne Weg zu einer Wurzel (und ohne `propagate_default_route`)? Und das Custom Field für Sonderfälle: am Gerät `watchdog_target` (Text: leer automatisch, `none` aus, sonst eine feste IPv4), oder lieber eine Auswahl plus ein IP-Feld? Ein neues Custom Field muss auch in die produktive NetBox importiert werden | Warnen nur bei Geräten mit `ospf`-Interface ohne Weg zu einer Wurzel und ohne `propagate_default_route`; ein Textfeld `watchdog_target` am Gerät (leer automatisch, `none` aus, sonst eine IPv4) mit Validierungs-Regex in NetBox, keine widersprüchlichen Kombinationen (2026-11-06) | E1 |
| Z39 | Reiner L2-Switch (C18): Wie wird er in NetBox modelliert, und welche VLANs trägt sein Trunk? | Custom Field `l3_uplink` am Port des alten Switches zum L3-Switch, dessen L3-Funktionen dort umgesetzt werden; VLAN-IDs automatisch aus einem auf beiden Switches unbenutzten Bereich (2026-11-07) | C18 |
| Z40 | DNS der Geräte-Adressen (C19): Welcher Server beantwortet die Einträge? (a) ein eigener autoritativer Server (im Provisioner oder daneben, Zone aus NetBox), an den die übergeordnete Zone delegiert, Vorwärts- und Rückwärts-Zonen; (b) im bestehenden Blocky, als Einträge seiner Konfiguration (`customDNS`), die der Provisioner schreiben müsste; (c) wie (a), Blocky gibt die Zonen per `conditional` an den eigenen Server weiter, wo es keine Delegation gibt (intern). Was Blocky davon kann (Reload, PTR), ist noch zu prüfen. Und welche Namen: `<port>.<gerät>.<zone>`, oder `dns_name` der NetBox-Adresse, wo gesetzt? | **offen** | C19 |
| Z41 | Zertifikat eines OpenVPN-Servers (E26): Woher weiss der Provisioner, dass ein Gerät einen OpenVPN-Server hat (ein Service in NetBox, ein Custom Field am Gerät)? Stellt `www_ca` auch dieses Zertifikat aus, oder eine eigene CA für OpenVPN (die Clients sollten nur ihr trauen)? | **offen** | E26 |
| Z42 | Was bekommt ein Anwohner, und woher weiss man, welche WLANs und welche WLAN-Gruppe ihm gehören? | Geräte, Ports fremder Geräte und VLANs (`owner`); die WLANs auf seinen VLANs gehören ihm, seine WLAN-Gruppe ist die seiner WLANs (2026-11-17; Bedingung `vlan__owner__users` in der Lab-NetBox geprüft) | C7 |
| Z43 | Eine WLAN-Gruppe pro Anwohner oder alle APs einer Gruppe mit allen WLANs? | alle APs strahlen alle WLANs der Gruppe aus (2026-11-17) | C7 |
| Z44 | Wie wird ein fremdes Gerät am Port eines Anwohners kein OSPF-Nachbar? | OSPF-Authentisierung pro Link (2026-11-17) | A10, C7 |
| Z45 | Fremde VLANs von einem Gerät des Anwohners fernhalten? | nein, er konfiguriert es nur über den Provisioner (2026-11-17) | C7 |
| Z46 | Dürfen Anwohner-Änderungen fremde Geräte betreffen? | ja; die Mutationen regeln nur, was er in NetBox eintragen darf, auf die Geräte kommt es über ein automatisches Rollout (2026-11-17) | C7, C20 |
| Z47 | Darf ein Anwohner WLANs anlegen? | nein, vorerst nur Ports und bestehende WLANs: Passwort, nach dem Prototyp auch SSID (eindeutig in der WLAN-Gruppe) und ein/aus (2026-11-17) | C7, C21 |
| Z48 | Wer richtet ein Gerät eines Anwohners ein? | vorerst der Admin (2026-11-17) | C7 |
| Z49 | Was ist ein Netz für den Anwohner? | ein VLAN mit höchstens einem WLAN, das er ein- und ausschaltet (2026-11-17) | C21, E27 |
| Z50 | Anschluss (Uplink-Port) eines Geräts automatisch erkennen? | ja, über die Nachbarn; übernommen wird mit einem Klick, nicht von selbst (2026-11-17) | C21 |
| Z51 | Rückgängig nach gescheitertem Ausrollen? | nein, es bleibt als Abweichung sichtbar; eine ausgerollte Änderung lässt sich als neue Änderung zurücknehmen (2026-11-17) | C20, C21 |
| Z52 | Darf ein Admin das UI eines Anwohners sehen und dort ändern? | ja, Support-Ansicht mit Auswahl des Anwohners; der Verlauf nennt beide; später vielleicht Stufen (nur sehen) (2026-11-17) | C21 |
| Z53 | Welcher Teil des Netzes rollt automatisch aus? | die Sites mit dem Custom Field `auto_rollout`, ein Gerät kann ausgenommen werden; einschalten nur mit leeren Diffs (2026-11-17) | C20 |
| Z54 | Was, wenn eine Änderung ein manuelles Gerät betrifft? | kein teilweises Rollout, ein Admin gibt frei oder wendet von Hand an (2026-11-17) | C20 |
| Z55 | Was, wenn ein automatisches Gerät vor dem Rollout nicht erreichbar ist? | es wird übergangen und bekommt die Änderung nachgeholt, ebenso das andere Ende seiner Funkstrecken; nur ein Fehler während der Transaktion nimmt alles zurück (2026-11-17) | C20 |
| Z56 | Was von Hand auf ein automatisches Gerät kommt und NetBox widerspricht? | geht spätestens nach 30 min zurück (Abweichung mindestens so oft gelesen), der Journal-Eintrag nennt die Änderungen pro Tabelle; Drift wird nicht erkannt (2026-11-17) | C20 |
| Z57 | Internet vor und nach einem automatischen Rollout? | von jedem Gerät geprüft, IPv4 und IPv6 gleich wichtig; erreicht ein erreichbares Gerät nicht beides, geschieht nichts automatisch; fehlt eines danach, geht alles zurück und der Bereich in den Fehlerzustand (2026-11-17) | C20 |

## A. Sicherheit

| # | Aufgabe | Aufwand | Status |
|---|---|---|---|

## B. Bereinigungen

| # | Aufgabe | Aufwand | Status |
|---|---|---|---|

## C. Konzepte zum Umsetzen

| # | Aufgabe | Konzept | Aufwand | Status |
|---|---|---|---|---|
| C3 | Interne Firewall: `guest` und `protected` am Gateway-Interface, Freigaben als Custom Fields (Z4), nur ein Router pro Netz | [Firewall](concepts/firewall.md#interne-firewall) | M | offen |
| C7 | Self-Service und Personas (gebaut: Besitz, Rollen, lesende Schnittstelle, [self-service.md](self-service.md)); offen: NetBox über Pocket-ID mit der Gruppe „Anwohner“ und den Claims geprüft, Leserechte in NetBox nur auf eigene WLANs, Leases, Clients, WLANs: ein/aus (Z47; Passwort und SSID gebaut), Ports, eigene Geräte anschliessen (A10), Anschluss und Raum eines Geräts; Ersteinrichtung durch den Admin (Z48); die Geräte über C20 (Z46); das UI in C21 | [Self-Service](concepts/self-service.md) | L | offen |
| C8 | Überwachung: Collector, Prometheus-Metriken (Alarme dort, Z13), Ping zum Uplink-Gateway (Z14), Zustandsseite, Ansicht für Anwohner | [Überwachung](concepts/monitoring.md) | L | offen |
| C10 | Updates im ganzen Netz: mehrere Sites (Z20), Pilot vorgeschlagen (Z18), ohne Wartungsfenster (Z19) | [Updates im ganzen Netz](concepts/network-update.md) | L | Regel, Plan, Rollout und eingefärbte Knoten der Netz-Ansicht erledigt; offen: CHR-Test der Kette `entry -- a -- target` |
| C18 | Reiner L2-Switch als „Extender“: Ein alter Switch ohne L3-Offload oder mit anderen Problemen (CRS2xx, CRS354) hängt an einem modernen Switch oder Router, der seine L3-Funktionen über einen Trunk übernimmt: Port oder Bridge, mit IP oder VLAN (Z39) | [Reiner L2-Switch](concepts/l2-extender.md) | M–L | offen |
| C19 | DNS für alle Adressen auf Geräte-Ports: Vorwärts- (A, AAAA) und Rückwärts-Einträge (PTR) aus NetBox, ein Server dafür (Z40); zuerst ein Konzept | – | M–L | offen |
| C21 | Anwohner-UI nach dem Prototyp (gebaut: Tabs Dosen und Geräte nach Site und Location mit Front-Ports als Dosen, Netze mit „WLAN ändern“ für SSID, Passwort und ein/aus, Support-Ansicht mit Wahl des Anwohners, [self-service.md](self-service.md#anwohner-ui)); offen: Tabs Dosen und Geräte (Geräte pro Port aus der Bridge-Host-Tabelle), Netze (QR-Code, Clients mit Band und Signal), Änderungen (Stand pro Gerät, Zurücknehmen); Entwürfe mit Zurücksetzen und Vorschau; ein Status-Label pro Element aus Rollout (C20) und Live-Reads; Anschluss erkennen und übernehmen (Z50); Raum per Popup; Support-Ansicht für Admins mit Filter nach Besitz im Provisioner (Z52) | [Self-Service](concepts/self-service.md#anwohner-ui) | L | offen |

## D. In Arbeit, Rest offen

| # | Aufgabe | Konzept | Aufwand | Status |
|---|---|---|---|---|
| C20 | Automatisches Rollout, Rest: der Journal-Eintrag nennt die Änderungen pro Tabelle (Z56); „nie automatisch“ erkennen (Weg des Provisioners, Ankündigungen ausserhalb der freigegebenen Bereiche, Umstecken, Warnungen des Anwendens wie `PoeCut`); selbst anstossen, wenn der Provisioner schreibt; Stand pro Änderung und Gerät für das Anwohner-UI (Z51); Vorschau beim Aufheben des Fehlers | [Automatisches Rollout](concepts/auto-rollout.md) | M–L | Bereich, Runden, Transaktion, Fehlerzustand, Karte erledigt |

## E. Erweiterungen umgesetzter Teile

| # | Aufgabe | Doku | Aufwand | Status |
|---|---|---|---|---|
| E3 | Das Image in einem Cluster ausrollen und die beiden Workflows (CI, Aufräumen) auf GitHub einmal laufen lassen | [Deployment](deployment.md#noch-nicht-geprüft) | S | offen |
| E8 | Weitere Befunde (Rest; erledigt: WLAN-Gruppe ohne Controller, Land, OSPF-Nachbar an einem Port ohne `ospf`, Board gegen Device Type): die Fehler des Generators gesammelt pro Site und PoE als Befund für alle Geräte einer Site | [Config-Helper](config-helper.md#befunde) | M | offen |
| E9 | Automatischer Test der Helfer gegen das Lab-NetBox: ausführen, Vorschau mit dem Ergebnis vergleichen, zurücknehmen | [Config-Helper](config-helper.md#technik) | M | offen |
| E10 | Rollout im Feld: ein kurzer Test „Strecke ok“ (Ping, Bandbreite zum Nachbarn) und eine Ansicht der Strecke | [Geräteseite](device-page.md) | M | offen |
| E12 | Ansicht „Was fehlt diesem Gerät noch“: die Befunde des Geräts mit den Helfern als Knopf, der Reihe nach durch Loopback, Verbindung usw., ohne Wizard zu sein | [Geräteseite](device-page.md) | M | offen |
| E13 | Diff in Worten neben dem Skript („entfernt 2 Adressen, ändert die SSID“), die heiklen Teile hervorgehoben (Adressen, OSPF, Bridge-Ports) | [Geräteseite](device-page.md) | M | offen |
| E15 | Port angeschlossener Geräte ohne RouterOS erkennen: Für ein Gerät mit dokumentierter IPv4 oder IPv6 über ARP, IPv6-Nachbarn und die Host-Tabelle der Bridge finden, an welchem Port des Routers oder Switches es hängt, und in der Live-Prüfung der Interfaces ein fehlendes oder anders gelegtes Kabel melden, mit einem Helfer, der es in NetBox anlegt (Plan, Vorschau, Rückgängig). Zuordnung über die IP, nie über MACs aus NetBox; mehrere MACs an einem Port (Switch ohne RouterOS dahinter) machen ihn mehrdeutig | [Config-Helper](config-helper.md#netbox-gegen-gerät) | M–L | offen |
| E23 | Umbenennung von Geräten: Die Gerätenamen sind heute unstrukturiert, einige werden sich ändern. Nach einer Umbenennung in NetBox soll man im Provisioner den neuen Namen dem alten zuweisen können, damit die Gerätekonfiguration nachgezogen wird (heute zeigt die Geräteseite nur, dass `/system/identity` nicht der NetBox-Name ist; heisst das Gerät wie ein anderes NetBox-Gerät, gilt es als `WrongDevice`) | [Geräteseite](device-page.md) | M | offen |
| E25 | Fehler beim Start des Apply-Skripts zeigen: Lehnt RouterOS das Skript schon beim Parsen ab (lab-crs125, 7.24.6: `expected end of command (line 61 column 5)` im Log, Topic `script,error`), setzt es keine Marke; der Job endet mit „Nicht alles wurde angewendet“ und dem ganzen Restskript, aber ohne den Grund. Fehlt nach `unmarked` jede Marke, die Meldung aus `/log` lesen und mit Zeile und Befehl beim Job zeigen | [Jobs](jobs.md) | S | offen |
| E27 | Ausgeschaltete WLANs: `status` des Wireless LAN mitlesen (`fetch_topology.rs` liest nur SSID, Sicherheit, PSK und VLAN) und ein ausgeschaltetes nicht generieren. Heute käme auch ein in NetBox deaktiviertes WLAN auf die Geräte; Voraussetzung für den WLAN-Schalter der Anwohner (Z49) | [WLAN](netbox-modeling.md#wlan) | S | offen |
| E28 | Band der WLAN-Clients: `WlanClient` (`device/metrics`) um Band und Standard (2.4/5 GHz, ax) ergänzen, aus der Registrierungstabelle von `/interface/wifi` und `/caps-man` | [Self-Service](concepts/self-service.md#clients) | S | offen |
| E29 | Geräte pro Port: die Host-Tabelle der Bridge (`/interface/bridge/host`) lesen und Leases damit einem Port zuordnen (welche MAC an welchem Port gelernt wurde), für die Dosen im Anwohner-UI; verwandt mit E15 | [Self-Service](concepts/self-service.md#leases) | M | offen |
| E26 | Zertifikat für einen OpenVPN-Server: das Zertifikat aus einer CA ausstellen, hochladen und belassen wie das von WebFig (`www_certificate`), wo NetBox einen OpenVPN-Server nennt (Z41). Dass Apply eine OpenVPN-CA entfernte, ist behoben (in done.md) | [Deployment](deployment.md) | M | offen |
