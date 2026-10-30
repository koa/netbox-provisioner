# Config-Helper

NetBox ist die Quelle der Wahrheit, aber die Daten waren in schlechtem Zustand: In der
gespeicherten Produktionsantwort hatten von 41 RouterOS-Geräten 15 keine Primary IPv4 und 14 kein
`lo`, von 201 Adressen 73 kein Prefix gleicher Länge, 167 Ports kein Kabel. Der Generator erzeugt
trotzdem für alle etwas; die Lücken fallen nicht als Fehler auf, sondern als Konfiguration, die
etwas anderes tut als das Netz. Die Regeln, wie man richtig modelliert
([netbox-modeling.md](netbox-modeling.md)), verteilen sich über viele NetBox-Formulare. Darum hat
der Provisioner zwei Werkzeuge:

1. **Befunde:** was in NetBox fehlt oder nicht zusammenpasst und, wo er es lesen kann, was das
   Gerät anders macht. Nur lesen.
2. **Helfer:** für die Standardfälle ein kurzer Ablauf, der die nötigen NetBox-Objekte anlegt oder
   ändert. Ein Helfer schreibt nur in NetBox, nie auf ein Gerät; dorthin kommt es über „Anwenden“.

Aufgerufen werden die Helfer vom Port aus, in der [Interface-Ansicht](interface-view.md) der
Geräteseite.

## Grundsätze

- **Plan, Vorschau, Bestätigen.** Jede Helfer-Aktion berechnet zuerst einen *NetBox-Plan*: die
  Liste der Objekte, die angelegt, geändert oder gelöscht werden, mit altem und neuem Wert. Der
  Plan wird angezeigt und erst nach Bestätigung geschrieben, wie beim Anwenden auf ein Gerät.
- **Vorschau bis aufs Gerät.** Der Plan wird zusätzlich auf die schon geladene NetBox-Antwort im
  Speicher angewendet (`ResponseData`). Daraus baut der Provisioner wie gewohnt die Topologie
  (`build_topology_from`) und erzeugt die Konfiguration der betroffenen Geräte. Die Vorschau zeigt
  deshalb neben dem NetBox-Plan auch das RouterOS-Skript, das danach beim Anwenden käme. Fehler
  des Generators (MTU ungleich, Prefix fehlt, …) zeigen sich, *bevor* etwas in NetBox steht.
- **Nichts automatisch reparieren.** Der Befund schlägt Korrekturen vor, er führt keine aus.
  Jede Korrektur ist eine Helfer-Aktion mit Plan und Bestätigung, so wie der Provisioner auch
  auf Geräten nichts von selbst repariert.
- **Keine MACs in NetBox.** Auch ein Import vom Gerät überträgt keine MAC-Adressen.
- **Board-Wissen bleibt in `hw_facts.rs`.** Welche Ports ein Board hat, wo PoE-out ist, welches
  Funkpaket es braucht: Der Helfer fragt `hw_facts.rs`, statt eine zweite Tabelle zu führen.

## Befunde

Die Befunde stehen oben auf der Site-Seite, die Netz-Befunde auf der Site-Übersicht, ihre Anzahl
an den Gerätekarten, und auf der Geräteseite gruppiert an den Zeilen der Interface-Ansicht.

### NetBox in sich

Prüfungen, die nur NetBox brauchen (`topology/findings.rs`), einmal pro geladener Topologie
gerechnet. Jeder Befund hat eine Schwere (Fehler: Generator scheitert oder macht etwas Falsches;
Warnung; Hinweis) und die Objekte mit Link nach NetBox. Fast jede Regel aus `netbox-modeling.md`
ist eine Prüfung:

- Gerät mit Platform `routeros` ohne `lo` (`NoLoopback`), ohne Primary IPv4 (`NoPrimaryIpv4`: ein
  Fehler, wo das Gerät `ospf` hat, sonst eine Warnung) oder mit Primary IPv4 nicht auf `lo`.
- Eine Primary IPv4 auf `lo` ohne `ospf` an irgendeinem Interface (`LoopbackWithoutOspf`, Fehler):
  Niemand kündigt sie an, der Provisioner erreicht das Gerät dort nicht.
- Adresse ohne Prefix gleicher Länge (`AddressWithoutPrefix`); `dhcp` an einem Interface, in dessen
  Prefix keine Adresse für einen Pool frei ist (`DhcpPoolEmpty`, ein /30 mit beiden Hosts belegt,
  ein /31): Der Generator setzt dort keinen DHCP-Server auf, ohne Fehler, und der Befund rechnet
  den Pool wie er (`dhcp_pool_v4`). Hat das Interface ein anderes Netz mit Pool, ist es kein
  Befund: Sein einer DHCP-Server bedient das, und das Netz ohne Platz braucht keine Clients (das
  /31 neben dem DHCP-Netz einer Funkbrücke ohne RouterOS).
- Kabel, dessen Enden nicht im selben Netz liegen (`NoCommonSubnet`), nur ein Ende `ospf` hat
  (`OspfOnOneEnd`) oder ungleiche MTU (`MtuMismatch`). Netz und `ospf` nur, wenn beide Enden (der
  Port oder seine Bridge) IPv4 haben: Ein Brückengerät ohne Adresse arbeitet nur auf L2, das OSPF
  des Routers geht hindurch.
- Verkabelter Port eines RouterOS-Geräts mit IP, aber ohne `ospf` (`AddressWithoutOspf`, Hinweis:
  es gibt Kunden-Ports).
- Interface-Namen, die wie ein Port aussehen, aber keiner sind (`PortLikeName`, `ether3-alt`).
- VLAN ohne VLAN-Gruppe (`VlanWithoutGroup`); WLAN ohne VLAN (`WlanWithoutVlan`, Fehler am
  Controller und an den APs ihrer Gruppe: Die Clients verbinden sich und erreichen nichts).
- Ein Ende eines Wireless Links ohne RF-Rolle (`RadioWithoutRole`, Warnung): Das Radio behält seine
  Konfiguration, die Funkbrücke wird nicht eingerichtet.
- Pool fehlt oder ist fast voll (`PoolMissing`, `PoolNearlyFull`, siehe [Pools](#pools)).
- Die Befunde der Interface-Ansicht (ein L3-Übergang pro VLAN, tagged VLAN am Port, Strecke
  unvollständig, Label, VLAN ohne Port), siehe [dort](interface-view.md#was-geprüft-wird).

Den Stand des gespeicherten Exports schreibt der Test `findings_of_netbox_example` nach
`target/netbox-example/findings.txt`.

### NetBox gegen Gerät

Für erreichbare Geräte liest der Provisioner das Gerät nur und vergleicht. Das Gerät ist die
Wahrheit über das, was *ist*, NetBox über das, was *sein soll*; welche Seite recht hat,
entscheidet der Mensch: „In NetBox übernehmen“ (Helfer) oder „Gerät anpassen“ (Anwenden).

- **Nachbarn → Kabel** (`device/neighbors.rs`): `/ip/neighbor` (MNDP/LLDP) sagt, welches Gerät an
  welchem Port hängt. Fehlt das Kabel in NetBox oder geht es anderswohin, ist das ein Befund, mit
  „Kabel übernehmen“ bzw. „Verbindung“. Zugeordnet wird über den Namen des Nachbarn (Identity =
  NetBox-Name) und seinen Port, nicht über die MAC. „Nachbarn lesen“ auf der Site-Seite fragt jedes
  Gerät mit einer eigenen Abfrage (`access.neighborFindings`), alle gleichzeitig, und zeigt jedes,
  sobald es antwortet; die Geräteseite zeigt es in der Live-Prüfung der Interfaces.
- **Adressen und Leases**, die nur auf dem Gerät stehen: in der Live-Prüfung, mit [Adresse
  übernehmen](#adresse-übernehmen) und [Lease übernehmen](#lease-übernehmen).
- **PoE**, das NetBox anders hat als das Gerät: in der Live-Prüfung, siehe [PoE](#poe).
- **Die ganze Site:** siehe [Abweichung von NetBox](#abweichung-von-netbox).

Was dabei gelernt wurde:

- Auf dem Gerät heissen die Ports wie die generierte Config sie nennt (`s01-core`), auch die,
  die ein Nachbar meldet. Zugeordnet wird über den NetBox-Namen (Default-Name) oder das Kürzel
  vor dem ersten `-` (`s01` ist `sfp-sfpplus1`).
- Über eine Funkbrücke sieht ein Router den Router der Gegenseite, nicht das Brückengerät. Ein
  Kabel zu einem Brückengerät (Rolle `wireless-bridge` oder ein Port mit RF-Rolle) gilt deshalb als
  erfüllt, solange das Gerät dort nur bekannte Geräte sieht, die NetBox anderswo hat.
- Im Lab hatten `lab-ap1`, `lab-sxt-a` und `lab-sxt-b` `discover-interface-list=*2000011`: Die
  Discovery verweist auf eine Interface-Liste, die ein Apply entfernt hat (die Factory-Liste
  `LAN`), und ist damit aus. Das meldet der Befund als eigenen Fehler statt lauter „sieht
  niemanden“. Der Generator setzt sie jetzt auf die Interface-Liste `maintenance` (die Ports zu
  eigenen Geräten und die freien Ports, dieselbe wie für mac-telnet), nicht auf Kunden-Ports.

### Abweichung von NetBox

Am Anfang und nach Arbeiten von Hand gilt für jedes Gerät dieselbe Schleife: den Diff ansehen,
was davon gewollt ist (dann NetBox korrigieren, etwa mit „Adresse übernehmen“) und was falsch
(dann anwenden). Den Überblick gibt „Abweichung von NetBox“ auf der Site-Seite (`diffSite`,
`device/site_diff.rs`, `components::site_diff`): Ein Job der Site liest ihre RouterOS-Geräte,
vier zugleich, und meldet je Gerät, was ein Anwenden je Menü hinzufügt, ändert und entfernt (wie
das Skript: Updates ohne Felder zählen nicht), oder warum es nicht lesbar war. Die Karte zeigt
den Zwischenstand, die meisten Änderungen zuerst, hebt entfernte Adressen, statische Leases, OSPF
und Bridge-Ports hervor und zählt, wer schon „entspricht NetBox“. Der Job sperrt kein Gerät, ein
Anwenden daneben geht; ein zweiter Start gibt den laufenden zurück, und die Seite übernimmt beim
Öffnen den letzten Job der Site.

Historische Reste in echten Konfigurationen (deaktivierte oder verwaiste Einträge) sind nicht
automatisch Anforderungen; der Mensch entscheidet beim Übernehmen.

## Helfer

Jeder Helfer ist ein kurzes Formular, das einen Plan erzeugt (`backend/src/helper/`, im Frontend
`components/netbox_helper/`, ein Modul pro Helfer).

### Neues Gerät

Vom freien Port eines Routers aus („Neues Gerät anschliessen“, `helper::new_device`): das
Gerät hängt an diesem Port. Frei ist ein Port ohne Kabel und einer, dessen einziger Kabelweg
offen an einem Patchpanel endet (Front- oder Rear-Port ohne weiteres Kabel, `landing`): im Feld
der Normalfall, Port → Panel → Dose, die Dose ohne dokumentiertes Gerät. Das neue Kabel geht
dann von diesem offenen Ende zum Gerät, die Panels bleiben, wie NetBox sie hat. Sieht die Live-Prüfung dort ein MikroTik, das NetBox nicht kennt
(`NeighborsNotInNetbox` mit Board), ist die Aktion ein Lösungsvorschlag mit Identity, Board und
Port der Gegenseite. Ein Gerät ohne Kabel zu einem bekannten Gerät hat keinen Einstieg: die
Ersteinrichtung braucht den Nachbarn.

Eingabe: Name, Device Type, Rolle, Port am neuen Gerät (gesehen, sonst `ether1`) und für einen
Access Point die WLAN-Gruppe (nur Gruppen mit Controller). Der **Device Type** ist einer der
MikroTik-Typen in NetBox (`Query.newDeviceOptions`): RouterOS meldet das Board
(`RB962UiGS-5HacT2HnT`), NetBox heisst die Typen meist wie MikroTik sie verkauft („hAP ac“).
Vorgewählt ist der Typ, dessen Modell oder Part Number das gesehene Board ist; sonst wählt man,
und fehlt der Typ, legt man ihn zuerst in NetBox an (entschieden 2026-10-24). Die Rolle ist mit
`router` vorbelegt.

Ein Plan (entschieden 2026-10-24):

1. Device (`CreateDevice`): Site des Routers, Platform `routeros` (nach Namen gesucht, wie
   `has_routeros` sie erkennt), Device Type, Rolle, Status aktiv, Tenant des Routers (sein
   eigener, sonst der seiner Location; ohne beide keiner, dann gilt der der Site). Der Tenant
   trägt die Zugangsdaten, und die Ersteinrichtung loggt sich vom Router aus ein. Dazu
   `wlan_group`, wenn gewählt.
2. Der Port, an dem das Kabel landet, und das Kabel selbst, wenn es an einem Patchpanel beginnt
   oder das Gerät ein Brückengerät ist (sonst legt es „Verbindung“ an). Legt NetBox ihn mit dem Device Type schon an, übernimmt
   der Schreib-Job diesen. Weitere Ports legt der Helfer nicht an: die des Device Types kommen
   von NetBox, ein Port, den NetBox nicht hat, ist für den Generator frei.
3. „Loopback“ und, ausser für ein Brückengerät (Rolle `wireless-bridge`), „Verbindung“ zum Port
   (/31, `ospf`, Labels), beide auf der Vorschau des Geräts gerechnet. Ein Brückengerät bekommt
   nur das Kabel; seinen Richtfunk macht danach „Funkbrücke“.

Die Schritte nach dem Gerät verweisen auf seine Objekte mit den Ids der Vorschau (von `u32::MAX`
abwärts, `PlanStep::with_real_ids`); der Schreib-Job setzt die Ids ein, die NetBox beim Anlegen
vergibt. „Rückgängig“ löscht alles in umgekehrter Folge, zuletzt das Gerät mit den Ports seines
Device Types. Nach dem Schreiben führt „Zum neuen Gerät“ auf dessen Seite mit der
Ersteinrichtung; der Router bekommt die Verbindung mit „Anwenden“.

### Loopback

Nächste freie `/32` aus dem Loopback-Pool (siehe [Pools](#pools)), auf `lo`, als
Primary IPv4, dazu eine `/128` aus dem IPv6-Loopback-Pool als Primary IPv6. Was schon da ist,
bleibt: ein `lo`, eine Adresse der IP-Version darauf, eine Primary IP, die diese Adresse ist.

`POST …/available-ips/` legt die Adresse mit der Länge des Prefixes an (`/26`), nicht
als `/32`. Der Plan liest deshalb die erste freie Adresse (`GET …/available-ips/?limit=1`) und
legt sie beim Schreiben als `/32` bzw. `/128` an, nachdem er geprüft hat, dass sie noch frei ist.
Hat sie inzwischen jemand, bricht er ab (siehe Gleichzeitige Änderungen).

Frei in NetBox heisst nicht frei im Netz: Ein Gerät, das NetBox nicht kennt, kann die Adresse
schon haben. Deshalb prüft der Plan jede neue Adresse gegen die Routen eines Routers der
OSPF-Area, die alle dieselben kennen (`helper/routes.rs`, nur lesend): zuerst den
Einstiegs-Router (`management_proxy.device`), sonst die Router mit den meisten OSPF-Interfaces,
den ersten von dreien, der antwortet. Liegt die
Adresse in einer Route, die spezifischer ist als der Pool-Prefix (eine /32 aus OSPF, ein
Transfernetz), überspringt er sie und nimmt die nächste freie. Die übersprungene zeigt der Plan
mit Route und Gateway an, damit sie in NetBox nachgetragen wird. Antwortet keiner, vergibt der
Helfer keine Adresse. Der Schreib-Job rechnet den Plan neu und
prüft damit noch einmal.

### Verbindung

Zwei Varianten, beide mit dem nächsten freien Netz aus dem Transfer-Pool (`POST
…/available-prefixes/`), `ospf` an beiden Enden, gleicher MTU an beiden Enden (Vorgabe übernehmen
oder eingeben) und als Label je dem Namen der Gegenseite:

- **Direkt:** Zwei Ports wählen (oder ein Kabel aus dem Befund übernehmen). Plan: Kabel, falls es
  fehlt; ein `/31`, je eine Adresse an beide Enden. Ein `/31` hat genau zwei Adressen, ohne
  Netz- und Broadcast-Adresse (RFC 3021); der Transfer-Pool reicht so doppelt so weit wie mit
  `/30`. Die vorhandenen `/30` bleiben gültig, der Helfer vergibt nur neue als `/31`.
- **Über eine Bridge:** Die beiden Router hängen an den Enden einer [Funkbrücke](#funkbrücke).
  Plan: ein `/29` mit festen Adressen für die beiden Router und die beiden Brückengeräte, siehe
  [Brückengeräte mit fester Adresse](#brückengeräte-mit-fester-adresse).

Geprüft (CHR 7.24.5): `three_node_ospf_chain_over_31` (Kette über zwei `/31`, OSPF mit dem
generierten `type=broadcast` auf beiden Hops `Full`, Anwenden über den Nachbarn) und
`mac_telnet_setup_over_31` (Ersteinrichtung und Tunnel über den Nachbarn im `/31`). Der
Generator vergleicht bei der Ersteinrichtung nur Netze, der DHCP-Pool betrifft nur `dhcp`-Netze.

„Direkt“ (`helper/connection.rs`): vom eigenen Port aus die Gegenseite und deren Port wählen
(`linkPorts`; ein Port mit Kabel zu einem Router bringt die Gegenseite mit) und optional die MTU (sonst die grössere der
beiden Ports). Was schon da ist, bleibt: ein Kabel zwischen den beiden, Adressen in einem
gemeinsamen Netz. Abgelehnt wird ein Port mit Kabel woandershin, in einer Bridge oder mit einer
IPv4 ausserhalb des Netzes der Gegenseite. Das `/31` ist das erste aus `available-prefixes` des
Pools, in dem NetBox auch keine Adresse hat (ältere Verbindungen haben oft Adressen ohne Prefix)
und das kein Router routet (wie bei Loopback). Das neue Prefix bekommt kein Tag, als
Beschreibung die beiden Geräte. Ports und Geräte stehen in der Auswahl in NetBox' Reihenfolge
(`topology/natural.rs`, wie NetBox' `naturalize_interface`). Meldet der Vergleich mit den Geräten an einem Port, den NetBox hat, „kein Kabel“ und sieht das
Gerät dort genau einen Nachbarn, dessen Port NetBox auch hat, öffnet „Verbindung“ mit diesen beiden
Enden. Den Helfer selbst im Lab geprüft: Plan, Vorschau, Schreiben und Rückgängig mit zwei
vorübergehenden Ports `sfp-sfpplus2` an lab-np1 und lab-np2 (danach gelöscht); das Lab-NetBox hat
dafür den Transfer-Pool `192.168.101.0/24`.

**Verbindung abräumen** (`helper/disconnect.rs`): Nimmt eine falsche Verbindung aus NetBox, damit
„Verbindung“ die richtige anlegen kann. Für einen Port: sein Kabel, seine Adressen aus dem
Transfer-Pool und die der Gegenseite im selben Netz, das Prefix dieses Netzes, wenn danach nichts
mehr darin ist (nie ein Prefix des Pools selbst), und mit dem Kabel an beiden Enden das Label,
wenn es die Gegenseite nennt, und `ospf`. Andere Adressen (ein Kunden-Netz) bleiben. Ist eine
der Adressen die Primary IP eines Geräts, lehnt der Helfer ab. Die Adressen verschwinden beim
nächsten Anwenden von den Geräten; ein Gerät, das sonst keine Verbindung zu einem Router hat,
nennt der Plan als abgeschnitten, bis die neue Verbindung an beiden Enden angewendet ist. In
„Verbindung“ bekommt ein gewählter Port, dessen Verbindung im Weg ist (Kabel woandershin, IPv4
nicht im Netz der Gegenseite), einen Knopf „Verbindung an … abräumen“. Der Port kann auch der
Gegenseite gehören; geschrieben wird im Job des Geräts, dessen Seite fragt. „Rückgängig“ legt
Gelöschtes aus dem JSON wieder an, das NetBox vorher hatte (mit neuer ID). Im Lab geprüft mit
vorübergehenden Ports `sfp-sfpplus2` an lab-np1 und lab-np2 und `ether9` an lab-np2: Knopf,
Plan, Vorschau, Schreiben, Rückgängig (Kabel, Adressen, Prefix, Label und `ospf` wieder da),
danach gelöscht.

**Kabel übernehmen** (`helper/cable_takeover.rs`): Sieht das Gerät an einem Port genau einen
Router (kein Brückengerät), aber NetBox hat dort ein Kabel woandershin (`CableElsewhere`) oder an
einen anderen Port desselben Geräts (`OtherFarPort`), bietet die Live-Prüfung der Zeile „Kabel
übernehmen“ mit beiden Enden an; ohne Kabel in NetBox „Verbindung“ mit beiden Enden. Der Plan ist
„Verbindung abräumen“ an jedem der beiden Ports, der in NetBox eine andere Verbindung hat (Kabel
oder Adressen), nacheinander auf der Vorschau des vorigen, dann „Verbindung“ auf dem Ergebnis,
deren Schritte hinten angehängt. Ein Kabel über ein Patch-Panel lehnt er ab (`PORT_PATCHED`): Das
neue Kabel wäre direkt und ersetzte, was NetBox vom Gebäude dokumentiert; das korrigiert man von
Hand. Im Lab geprüft: das Kabel lab-np2 `ether2` – lab-wap `ether1` in NetBox vorübergehend auf
lab-wap `ether2` gelegt, auf lab-np2 Live prüfen, „Kabel übernehmen“ (11 Schritte, neues `/31`,
lab-wap als abgeschnitten genannt), Schreiben, Rückgängig, danach das Kabel zurückgelegt.

### Kunden-Netz

Für einen Port oder ein VLAN das **Parent-Prefix** wählen: Der Helfer bietet die Prefixe der Site
des Geräts an (Kunden-Netze gehören zum Standort), gewählt wird von Hand, nicht automatisch.
Plan: nächstes freies Netz darin (Länge wählbar, Vorgabe `/24`), Gateway-Adresse (erste Adresse)
am Port bzw. an einem neuen virtuellen VLAN-Interface, `dhcp`. Dazu oder allein ein
**IPv6-Prefix** der Site: das nächste freie `/64` mit `::1` als Gateway am selben Interface, das
`dhcp` per SLAAC ankündigt. Der DHCP-Pool ist der freie Rest des Netzes; eine IP-Range mit Rolle
`dhcp` legt der Helfer nicht an (E6).

`helper/customer_net.rs`:
- **Auswahl:** freie Ports und Bridges (ohne IPv4, oder ein Kunden-Netz mit `dhcp` ohne IPv6;
  nicht in einer Bridge, ohne VLAN, nicht zu einem Router verkabelt) und die VLANs des Geräts
  ebenso. Jede Familie ist wahlfrei („keins“), mindestens eine muss es sein; so bekommt ein
  bestehendes IPv4-Kunden-Netz sein IPv6 nachträglich, eine Familie, die der Port schon hat,
  lehnt der Plan ab. Ein VLAN mit einem
  virtuellen Interface ohne Adresse bekommt das Netz dort, sonst entsteht ein neues virtuelles
  Interface mit dem Namen des VLANs (Mode access, Untagged VLAN).
- **Parent-Prefixe:** je Familie die Prefixe mit Scope der Site des Geräts, ohne Tag `loopback`/`transfer`
  und ohne Adresse eigener Länge darin (also selbst kein Netz). Die Topologie liest dafür den Scope
  der Prefixe (`IpPrefixData.site`).
- **Plan:** das erste freie Netz der Länge (`available-prefixes`), in dem NetBox keine Adresse hat
  und das kein Router routet, auch nicht teilweise (`Routes::overlapping`). Dazu das Prefix mit
  Scope Site und, bei einem VLAN, dem VLAN, die erste Adresse als Gateway und das Tag `dhcp`.

Im Lab geprüft an lab-np1 mit einem vorübergehenden Port `ether9` und einem VLAN `lab-test` (199)
im Prefix `192.168.120.0/22` (Scope Lab): Die Vorschau zeigt in beiden Fällen Pool, DHCP-Server
und DHCP-Netz. Für das VLAN habe ich geschrieben und rückgängig gemacht, danach alles gelöscht.
IPv6 ebenso an `ether9` mit `fd99:1ab::/48` (Scope Lab): beide Familien zusammen (Vorschau mit
`fd99:1ab::1/64 advertise=yes` neben Pool und DHCP-Server), und an einem schon mit
`192.168.120.1/24` und `dhcp` versehenen Port nur das `/64`; IPv4 dort lehnt der Plan ab.

### OSPF- und DHCP-Flags

Die Tags `ospf`, `dhcp`, `dhcp-client`, `nat` eines Interfaces als Schalter (`helper/flags.rs`).
Die Tabelle zeigt die Interfaces des Geräts in NetBox' Reihenfolge mit Adressen, Kabel und
Bridge. Ports einer Bridge fehlen, weil dort
die Tags der Bridge zählen; einer mit eigenem Tag steht trotzdem da, damit es sich entfernen lässt.
Ein umgestellter Schalter ist markiert, der Plan ändert je Interface die Tags in einem Schritt.
Von einem Port der Interface-Ansicht her zeigt die Tabelle nur dieses Interface.
Darunter stehen die Befunde des Geräts, wie sie nach den umgestellten Schaltern wären
(`topology.findings(deviceId:, after:)` rechnet sie auf der Vorschau des Plans), mit Links nach
NetBox. Schaltet man etwa `ospf` an einem Ende einer Verbindung aus, erscheint „`ospf` nur an einem
Ende“, bevor etwas geschrieben ist. Im Lab geprüft an lab-np1: `ospf` an `sfp-sfpplus1` aus zeigt
den Befund, `nat` an `ether2` zeigt in der Vorschau die `srcnat`-Regel; Schreiben und Rückgängig.

### PoE

Eine Beziehung zwischen zwei Ports (`helper::poe`): `PSE` am speisenden, `PD` am gespeisten. Die
Karte wählt die Richtung (das Gerät speist oder wird gespeist), seinen Port und Gerät und Port der
Gegenseite (die Geräte der Site und die, zu denen seine Kabel gehen; alle Ethernet-Ports, auch von
Geräten ohne RouterOS). „Vom Gerät ablesen“ liest `/interface/ethernet/poe/monitor` und zeigt die
Ports mit `poe-out-status=powered-on`, mit der Gegenseite aus dem Kabel in NetBox oder, ohne
Kabel, aus `/ip/neighbor`; „Übernehmen“ setzt sie in die Karte. Hat NetBox es schon so (Kabel,
`PSE` hier, `PD` dort), steht „in NetBox so erfasst“ statt des Knopfs. Der Plan setzt `PSE` und `PD`
(`UpdateInterface.poe`) und legt das Kabel an, wenn beide Ports frei sind. Ein Kabel woandershin
wird abgelehnt. Ob das Board am Port PoE-out hat, weiss NetBox nicht (kein Board-Modell); der
Generator setzt `poe-out` ohnehin nur, wo das Board es hat. Im Lab geprüft an lab-np1: abgelesen
`e01-sxt-a` und `e02-ap1`, übernommen, `lab-sxt-a ether1` bekam `PD`, Rückgängig nahm es wieder
weg. Ein gespeistes Gerät meldet seine Quelle nicht; abgelesen wird deshalb am speisenden.
Von einem Port der Interface-Ansicht her ist er vorbelegt, die Gegenseite mit dem Ende seines
Kabels.

Speist das Gerät, darf die Gegenseite offen bleiben (`PoeInput.pd` leer): dann bekommt nur der
Port `PSE`, und der Generator schaltet dort PoE-out ein (`setup_poe` braucht kein Kabel). So
läuft PoE, bevor das Gerät am anderen Ende in NetBox ist. Kommt es online, sieht die
Live-Prüfung es am Port, und „Neues Gerät anschliessen“ legt es an. An einem `PSE`-Port bekommt
dabei sein Port gleich `PD` (`new_device`). Ist das Gerät schon erfasst und verkabelt, schlägt die
Live-Prüfung „PoE vervollständigen“ vor, sobald der Port laut Gerät speist (`powered-on`) und am
Gegenüber `PD` fehlt (`PoeProblem::PdMissing`). „PoE“ ist dann mit dem Port und dem Kabelende
vorbelegt. Im Lab geprüft: lab-core `ether1` allein auf `PSE` geschrieben und zurückgenommen.

Die dritte Richtung „Kein PoE an diesem Port“ nimmt einen Eintrag wieder heraus
(`Helper.poeRemove`, `helper::poe::plan_remove`, `UpdateInterface.poeRemoved`): den Modus des
Ports und am anderen Kabelende den passenden (`PD` gegenüber einem `PSE` und umgekehrt), denn ein
Ende allein sagt nichts. Ein Gegenüber mit demselben oder keinem Modus bleibt.

Geprüft gegen das Gerät wird in der Live-Prüfung der Interfaces (`helper::poe::mismatches`,
`LiveRow.poe`), an jedem Port eines RouterOS-Geräts mit Modus in NetBox:
- `PSE`, aber der Port hat kein `poe-out` (`/interface/ethernet`): das Board kann dort nicht
  speisen.
- `PSE` mit Kabel, aber `poe-out-status=waiting-for-load`: am anderen Ende bezieht nichts Strom.
  Das Gerät dort kann kein PoE, hat ein eigenes Netzteil oder ist aus. Im Lab so bei lab-np2
  `ether3` zu lab-ap2, das bewusst ohne Strom ist. Ohne Kabel ist das kein Befund: Der Port
  wartet auf sein Gerät (lab-np2 `ether4`).
- `PD`, aber das Gerät speist dort selbst (`powered-on`).
Jeder dieser Ports bekommt den Vorschlag „PoE-Eintrag entfernen“: „PoE“ mit „Kein PoE“
vorbelegt. Nicht prüfbar: ob ein Port PoE annimmt (RouterOS meldet es nicht pro Port), und alles
an Geräten ohne RouterOS; deren Einträge lassen sich nur von Hand entfernen. Im Lab geprüft:
`ether4` von lab-np2 entfernt (NetBox nimmt `poe_mode` leer an), Rückgängig setzte `PSE` wieder.

### Funkbrücke

Vervollständigt oder korrigiert eine Brücke zwischen zwei Routern, deren beide Geräte in NetBox
schon existieren (anlegen ist Teil von „Neues Gerät“); Brückengeräte ohne RouterOS siehe
[unten](#brückengeräte-ohne-routeros). Aufgerufen auf der Seite einer der beiden
Brücken (`radioBridgeCandidates`: Geräte mit genau einem Funk-Port) oder vom Router-Port
einer Richtfunk-Zeile der Interface-Ansicht her, dann für das dort verkabelte Brückengerät mit
diesem Router-Port vorbelegt. Von einem freien Router-Port her („Als Richtfunk“) wählt man zuerst
das Brückengerät (genau ein Funk-Port, kein Kabel zu einem Router), dann ebenso. Eingabe: ob diese
Brücke AP oder Station ist (immer gewählt, vorbelegt mit ihrer RF Role), die andere Brücke
(vorbelegt mit der Gegenseite ihres Wireless Links), je Seite der Router-Port nur dort, wo
`ether1` noch nicht zu einem Router verkabelt ist, optional die Adresse jeder Bridge und die SSID.

Plan (`helper::radio_bridge`) für jede Seite:

- Kabel vom ersten freien Ethernet-Port zum gewählten Router-Port, falls keines zu einem Router
  geht (`NoRouterPort` ohne beides).
- Die Bridge des Geräts (die des Funk-Ports, sonst ein vorhandenes Bridge-Interface), sonst eine
  neue `bridge` mit `ospf`; Ethernet-Port und Funk-Port als ihre Ports (`UpdatePort`).
- Der Funk-Port mit dem RouterOS-Namen (`wlan1`, bei 60 GHz `wlan60-1`; `NameTaken`, wenn das
  Gerät schon so ein Interface hat) und der RF Role.
- `lo` mit Loopback wie der Helfer „Loopback“, falls sie fehlt.

Dann der Wireless Link: einer zu einem dritten Gerät wird abgelehnt (`WirelessLinkElsewhere`). Ein
vorhandener bekommt fehlende SSID (Eingabe, sonst der Name des APs) und, wenn er offen ist, WPA
Personal mit einem erzeugten PSK (24 Zeichen). Ein vorhandener PSK bleibt; ist der Link
verschlüsselt, NetBox gibt den PSK aber nicht heraus, bricht der Plan ab (`PskUnreadable`). Ohne
Link wird einer angelegt (`CreateWirelessLink`). Ändert sich am Funk etwas, stehen AP und
Station in `apply_together`: Sie finden sich erst wieder, wenn beide die neuen Einstellungen
haben. Das geht nur gemeinsam (Batch); nacheinander lädt der Fallback der zuerst angewendeten
Seite die alten Einstellungen zurück.

Zuletzt das Segment: L3-Interfaces sind die beiden Router-Ports (bzw. deren Bridge) und die beiden
Bridges. Das Netz ist das IPv4-Netz eines Router-Ports (`PortAddressed`, wenn der andere eins
ausserhalb hat), sonst ein neues `/29` aus dem Transfer-Pool. Jedes Ende ohne IPv4 bekommt eine
Adresse darin (die eingegebene oder die nächste freie, gegen die Routen geprüft wie bei
„Verbindung“; `NetworkTooSmall`, wenn das Netz keine vier Adressen hat) und `ospf`. Für IPv6
reichen die Link-Local-Adressen, die Brücken sind über ihre Loopback erreichbar. Die Adresse,
die eine Brücke per DHCP hatte, kennt NetBox nicht; ist sie die nächste freie, bekommt sie
dieselbe, sonst eine andere (die eingegebene geht vor).

Zuletzt die Labels: An beiden Kabeln bekommt jedes Ende den Namen des Geräts am anderen Ende,
wo sein Label es nicht schon nennt (`LabelNotFarEnd`), wie bei „Verbindung“. Hat der Plan für
das Interface schon eine Änderung (`ospf`), kommt das Label in dieselbe: Eine zweite fände das
Label beim Schreiben schon geändert.

Geschrieben werden Port-Änderungen (Name, RF Role, Bridge, vorher geprüft) und der Wireless Link
(`POST wireless/wireless-links/`, `status=connected`, `auth_cipher=aes`); der PSK steht nicht im
Fingerprint und nicht im Job-Log. Rückgängig nimmt beim geänderten Link Authentication und PSK
wieder weg.

Im Lab geprüft an `lab-sxt-a`/`lab-sxt-b`: vollständig ergab „NetBox hat schon alles dafür“; ohne
die Bridge-Adresse von `lab-sxt-b` plante der Helfer `192.168.101.12/29` (dieselbe) und schrieb
sie; ohne RF Role an `lab-sxt-b wlan1` setzte er `station`. Anlegen des Links, Umbenennen und
Verkabeln sind nur mit Unit-Tests geprüft (`schopf_burri_gets_its_addresses` am Produktions-Export:
drei Adressen in `172.17.0.40/29`, `ospf` auf beiden Bridges).

#### Brückengeräte ohne RouterOS

Zwei Brückengeräte ohne Platform `routeros` (zwei UISP Wave Nano,
[foreign-radio-bridge.md](foreign-radio-bridge.md)) richtet man von Hand ein, als L2-Brücke
mit DHCP-Client. Sie sind Kandidaten wie die mit RouterOS (`radioBridgeCandidates.routeros`),
ein Ethernet-Port nach dem NetBox-Typ, und die andere Brücke ist von derselben Art (eine mit
RouterOS gegenüber einer ohne: `NoRouterOs`). Eine Wave Nano hat zwei Funk-Ports, `wlan0`
(60 GHz) und das 5-GHz-Backup `wlan1`: Angezeigt wird sie mit dem einen 60-GHz-Port
(`main_radio`), geplant werden alle. Die Funk-Ports der beiden Seiten bilden Paare, über den
Wireless Link, den einer hat, sonst über den gleichen Namen; einer ohne Gegenstück ist
`RadioUnpaired`, einer mit Link zu einem dritten Gerät `WirelessLinkElsewhere`. Das Formular hat keine Adressen. Der Plan
(`helper::foreign_radio_bridge`) ist die Seite von NetBox und den Routern:

- Kabel und Labels wie oben, die RF Role an jedem Funk-Port (`UpdatePort` ohne Bridge).
- Je Paar der Wireless Link mit SSID; ein neuer mit erzeugtem PSK, den man auf den Funkgeräten
  setzt.
  Ein vorhandener bekommt keinen: Seine Geräte laufen mit einem Schlüssel, den NetBox nicht kennt.
  Nichts in `apply_together`, die Funkgeräte wendet der Provisioner nicht an.
- Ein `/31` aus dem Transfer-Pool zwischen den beiden Router-Ports für OSPFv2, das Nachbarn nur
  im selben Subnetz bildet. Ein Router-Port in einer Bridge ist `PortInBridge`: Netze, `dhcp`
  und `ospf` kämen auf die Bridge und alle ihre Ports (1og-sw2 `ether13` steckt in seiner
  Client-Bridge); „Aus Bridge lösen“ macht ihn vorher frei.
- An jedem Router-Port ein eigenes `/29` aus dem Transfer-Pool, die erste freie Adresse darin am
  Port, mit `dhcp_lease_time` 120 s (`LEASE_SECONDS`): Eine Wave Nano nimmt das erste Angebot und
  wechselt innerhalb einer Lease zum anderen Router, wenn ihrer oder die Strecke ausfällt.
- `ospf` und `dhcp` an beiden Router-Ports.

Vorhandenes bleibt: Das Netz, das beide Router-Ports haben, ist das Transfernetz, eins, das nur
ein Port hat, sein DHCP-Netz; ein zweites eigenes (oder ein eigenes `/31`) ist `PortAddressed`.
Die beiden Netze des Plans liegen nicht übereinander (`next_free_network` lässt die schon
geplanten Prefixe aus). Die Vorschau generiert für den Router auf der AP-Seite.

Im Lab geprüft: `lab-wave-a`/`lab-wave-b`, von Hand so eingerichtet, ergaben nur RF Roles und
Labels. Zwei vorübergehende Wave-Geräte zwischen `lab-core ether1` und `lab-np2 ether9` (nur
geplant, nicht geschrieben) bekamen `192.168.101.28/31` und die beiden `/29` `.64` und `.72`; die
Vorschau von lab-core hatte den DHCP-Server mit `lease-time=120s`.

### Richtfunk abräumen

Nimmt das Netz einer Richtfunk-Strecke aus NetBox, damit „Funkbrücke“ oder eine andere
Verbindung an ihre Stelle kann. Aufgerufen von der Richtfunk-Zeile der Interface-Ansicht („Verbindung
entfernen“, auf dem Router wie auf dem Brückengerät), mit einem Funk-Port der Strecke.

Plan (`helper::radio_disconnect`): L3-Enden sind die Bridges beider Brückengeräte und die
Router-Ports (bzw. deren Bridge), die mit ihnen verkabelt sind. Von diesen gehen die IPv4-Adressen
im Netz des Segments: jedes Netz auf einer Bridge eines Brückengeräts, im Transfer-Pool oder an
mindestens zwei Enden. Eine Primary IP bricht den Plan ab (`PrimaryAddress`). Das Prefix des
Netzes geht, wenn danach nichts mehr darin ist (wie bei „Verbindung abräumen“), `ospf` an jedem
Ende, an dem keine andere IPv4 bleibt, und an beiden Kabeln das Label, das das andere Ende nennt.
Kabel, Brückengeräte, ihre Bridges und die Funk-Ports bleiben. Der Wireless Link geht nur mit dem
Schalter im Plan (`DeleteWirelessLink`, das Rückgängig legt ihn samt PSK wieder an). IPv6 bleibt:
„Funkbrücke“ legt im Segment keine an.

Nach dem Anwenden sind die Brückengeräte abgeschnitten, niemand kündigt ihre Loopback mehr an;
ein Router auch, wenn er nur über diese Strecke erreichbar war (`cut_off`).

### Kunden-Anschluss abräumen

Das Gegenstück zu „Kunden-Netz“, aufgerufen von der Kunden-Zeile der Interface-Ansicht
(„Verbindung entfernen“) mit dem Interface der Zeile: einem Port, einer Bridge oder einem
VLAN-Interface.

Plan (`helper::customer_disconnect`): alle Adressen des Interfaces (eine Primary IP bricht ab),
ihr Prefix, wenn danach nichts mehr darin ist, `dhcp` und die VLANs am Port (`UpdatePortVlan`:
untagged und tagged weg, ohne Mode). Bei einer Bridge sind das die Bridge und ihre Ports, bei
einem VLAN-Interface die Kunden-Ports des Geräts mit seinem VLAN (Ethernet, in keiner Bridge,
kein Kabel zu einem RouterOS-Gerät). Ein VLAN-Interface, das sonst nichts hat (kein `ospf`,
`nat`, `dhcp-client`, Label oder tagged VLAN), wird gelöscht (`DeleteInterface`, nach seinen
Adressen). Das VLAN selbst bleibt, andere Geräte können es haben. Ein Port, dessen L3 anderswo
liegt, verliert nur sein VLAN.

### Aus Bridge lösen

Nimmt einen Port aus der Bridge eines Kunden-Anschlusses, vom Menü des Ports aus
(`helper::bridge_leave`): ein Schritt `UpdatePort` ohne Bridge (`leaves_bridge`, geprüft gegen
die Bridge von vorher) und, wenn der Port eigene VLANs hat, `UpdatePortVlan` ohne sie. Danach
ist er frei. Sein Kabel bleibt, die Bridge mit Adressen, Prefix und `dhcp` auch: War er ihr
letzter Port, nennt der Plan sie (`NetboxPlan.emptied_bridge`), ihr Netz erreicht dann
niemanden, und „Kunden-Anschluss abräumen“ auf ihrer Zeile nimmt es heraus. Ein Port ohne
Bridge ist `NO_BRIDGE`. Im Lab geprüft an lab-np2 `ether4` (letzter Port von `lan`): Plan mit
dem Hinweis, geschrieben, Rückgängig brachte ihn zurück in `lan`.

### Adresse übernehmen

Trägt eine Adresse, die nur auf dem Gerät steht, in NetBox an ihrem Interface ein, damit das
nächste Anwenden sie nicht entfernt (Datenkorrektur).
Die Live-Prüfung der Interfaces findet sie: statische Einträge von `/ip/address` und
`/ipv6/address` (ohne link-local), deren IP keines der NetBox-Interfaces des Geräts hat. Adressen
an Interfaces, die der Provisioner nicht verwaltet, lässt das Anwenden stehen, wie BGP: Tunnel
(`gre`, `ipip`, …), PPPoE-Clients und alles andere von Hand Eingerichtete; die Prüfung zeigt sie
deshalb nicht (`/interface` vom Typ nicht `ether`, `bridge`, `vlan`, `vxlan`, `wlan`, `wifi`,
`w60g`, `loopback`). Eine an einem gelöschten Interface (`*35`) entfernt es. Sie
stehen an der Zeile ihres NetBox-Interfaces (über den Port-Namen oder eine andere Adresse, die
NetBox dort hat) unter „Nur auf dem Gerät“, als Warnung, sonst unter den weiteren Befunden. Den
Knopf „<Adresse> übernehmen“ gibt es nur, wenn NetBox die IP nirgends hat; hat sie ein anderes
Gerät oder kein Interface, nennt die Zeile das.

Plan (`helper::address_takeover`): `CreateAddress` am Interface, davor `CreatePrefix` für ihr Netz,
wenn NetBox weder dieses Prefix noch eine Adresse darin hat und die Länge keine Host-Länge ist
(`/32`, `/128`), mit dem kleinsten Prefix, das es enthält, als „aus“ und der Site des Geräts
(ausser in einem Pool, wie bei den anderen Helfern). Hat NetBox die IP schon, ist das
`ADDRESS_IN_NETBOX`. Im Lab geprüft an lab-np1 `ether6`: `192.168.199.1/30` über NetBox und
Anwenden aufs Gerät gebracht, in NetBox gelöscht; die Live-Prüfung zeigte sie, der Plan legte
Prefix und Adresse an, danach war der Diff leer; Rückgängig und Anwenden nahmen sie wieder weg.

### Lease übernehmen

Trägt den Client eines statischen DHCP-Leases, den nur der Router hat, in NetBox ein, damit das
nächste Anwenden ihn schreibt statt entfernt (Datenkorrektur, wie „Adresse übernehmen“; zum
Modell siehe [NetBox-Modellierung](netbox-modeling.md), „Statische DHCP-Leases“). Die
Live-Prüfung liest `/ip/dhcp-server` und seine Leases: die statischen, deren Adresse NetBox
nicht als Lease des Routers hat, stehen an der Zeile des Server-Interfaces (bei einem Lease für
alle Server die Zeile, deren Netz die Adresse enthält) unter „Leases nur auf dem Gerät“, als
Warnung, mit dem, was NetBox nicht halten kann (`block-access`, `rate-limit`, Lease-Zeit, …)
und dem Hinweis, wenn NetBox die Adresse an einem anderen Gerät hat. Der Knopf „Lease
<Adresse> übernehmen“ kommt nur für einen Lease mit MAC (nicht per Client-ID), dessen Adresse
NetBox nirgends hat.

Das Formular nimmt den Namen, den der Client geschickt hat (`host-name` bis zum ersten
Steuerzeichen: ein Tinkerforge-Brick füllt sein Feld mit Nullbytes auf), sonst den Kommentar;
Device Type und Rolle aus NetBox, „Generic“ und `client` vorgewählt, wo es sie gibt. Plan
(`helper::lease_takeover`): `CreateDevice` an der Site des Routers ohne Plattform,
`CreateInterface` `eth0` mit der MAC als primärer (`dcim/mac-addresses`, dann
`primary_mac_address` am Interface), `CreateAddress` mit der Länge des DHCP-Netzes. Abgelehnt
wird eine Adresse in keinem DHCP-Netz des Routers (`NO_DHCP_NETWORK`), eine, die NetBox schon
hat, eine ungültige MAC (`INVALID_MAC`) und ein vergebener Name. Die Übersicht der Site hebt
entfernte statische Leases hervor. Im Lab geprüft an lab-np2 `lan`: ein Client mit MAC über
NetBox und Anwenden als Lease aufs Gerät gebracht (danach leerer Diff), in NetBox gelöscht; die
Live-Prüfung zeigte ihn mit Name und MAC, der Plan legte Gerät, Interface und Adresse an,
danach war der Diff leer.

### VLAN am Port

Legt ein VLAN untagged an einen Port des Geräts (Mode access), für einen Kunden: das Netz eines
VLAN-Interfaces, dessen VLAN noch an keinem Port liegt (Befund `VlanWithoutPort`, von der
Kunden-Zeile aus mit dem VLAN vorbelegt), oder einen Port, dessen Netz auf einem anderen Gerät
liegt und über das VXLAN der VLAN-Gruppe kommt (aus „Rolle zuweisen“ einer unbekannten Zeile mit
dem Port vorbelegt, auch einer freien).

Plan (`helper::port_vlan`): ein Schritt `UpdatePortVlan`, tagged VLANs am Port fallen weg (Z26).
Zur Wahl stehen (`portVlanOptions`) die Ethernet-Ports ohne Bridge, Adresse, Uplink
(`dhcp-client`, `nat`) und Kabel zu einem RouterOS-Gerät, auch einer mit einem VLAN (das dann
ersetzt wird), und die VLANs, die das Gerät schon hat, oder aus einer Gruppe mit `vxlan_vni`: ein
lokales VLAN eines anderen Geräts erreichte den Port nicht. Gewählt wird in zwei Stufen, erst die
VLAN-Gruppe, dann ihr VLAN. Vorbelegt ist die Gruppe des vorbelegten VLANs, sonst die eines VLANs,
das das Gerät schon hat, die eines WLANs zuerst.

VLANs aus verschiedenen Gruppen dürfen auf einem Gerät liegen. Weggelassen und im Plan abgelehnt
wird, was der Generator nicht aufsetzen könnte:

- eine VID, die auf dem Gerät schon ein anderes VLAN hat (`VLAN_ID_TAKEN`, im Generator
  `AmbiguousVlanIds`): RouterOS kennt nur die Nummer, nicht die Gruppe. Das VLAN, das der Port
  selbst hat, zählt nicht, er bekommt ja das neue.
- ein getunneltes VLAN einer Gruppe, deren VXLAN auf dem Gerät in einer anderen Bridge hängt, als
  der Port käme (`VXLAN_IN_BRIDGE`, im Generator `PortInMultipleBridges`). Der Port kommt in die
  modellierte Bridge, die sein VLAN schon führt, sonst in die Standard-Bridge
  ([L2](l2-generation.md#schritt-2-nach-root-gruppieren-root-lose-ebenen-zusammenführen)).

Was diese Regeln nicht fassen, fängt der Plan ab: Er setzt auf seiner Vorschau L2 auf wie der
Generator und lehnt mit dessen Fehler ab, wenn das Gerät vorher ging und danach nicht mehr. Hat
der Port das VLAN schon (und kein tagged), lehnt der Plan ab (`PORT_HAS_VLAN`). Im Lab geprüft:
lab-wap `ether2` von VLAN 104 auf 105, Vorschau (`pvid=105` an seinem Bridge-Port), Schreiben und
Rückgängig; die Gruppe `lab-wlan` war vorbelegt.

### Brückengeräte mit fester Adresse

Ohne feste Adresse holte sich ein Brückengerät seine per DHCP-Client von einem der beiden Router
([netbox-modeling.md](netbox-modeling.md#funkbrücke)); funktionierte dieser Router nicht richtig,
war es gerade dann nicht erreichbar, wenn man es zur Fehlersuche brauchte. Der Helfer
[Funkbrücke](#funkbrücke) legt die Brückengeräte deshalb wie Router an, nur ohne Routing der
Nutzdaten:

- **Loopback** `/32` und `/128` aus den Loopback-Pools als Primary IPv4 (Router-ID) und
  Primary IPv6 (Management-Adresse).
- **Feste Adresse im `/29`** auf der Bridge des Brückengeräts, Tag `ospf` an der Bridge.
- OSPF-Priorität 0, damit ein Brückengerät nie DR oder BDR wird, fehlt noch (E7).

Warum das hilft:

- Ohne DHCP-Abhängigkeit bleibt ein Brückengerät erreichbar, solange einer der beiden Router es
  erreicht.
- Fällt die Funkstrecke aus, zerfällt das `/29` in zwei Hälften. Beide Router kündigen es weiter an,
  eine Adresse im `/29` wäre also nur zufällig richtig geroutet. Die Loopback dagegen kündigt nur
  die Seite an, auf der das Brückengerät noch hängt. So ist jedes Brückengerät auch bei
  unterbrochener Strecke von seiner Seite her erreichbar. Das ist der Fall, in dem man es am
  dringendsten braucht.

Die Nutzdaten bleiben auf L2: Die beiden Router sind im selben `/29` direkt Nachbarn und schicken
sich ihre Pakete direkt zu. Die Bridge des Brückengeräts leitet sie nur weiter. Zur CPU des
Brückengeräts kommt nur, was an seine eigenen Adressen geht (Management, OSPF-Hellos). Das
Brückengerät hat nur ein L3-Interface und kann deshalb keinen Transit-Verkehr routen. Weiterhin
gilt: Die Bridge darf keine IP-Firewall für gebridgten Verkehr einschalten (`use-ip-firewall=no`,
der Standard).

## Prefixe

| Prefix | in NetBox | vergeben wird |
|---|---|---|
| Loopback | gibt es, netzweit, IPv4 und IPv6 | `/32` und `/128` je Gerät, automatisch |
| Transfer | gibt es, netzweit, nur IPv4 | `/31` (direkt) oder `/29` (über Bridge) je Verbindung, automatisch |
| Kunden | pro Standort (Scope Site) | Netz wählbarer Länge im **gewählten** Parent-Prefix |

Transfernetze bekommen keine IPv6-Adressen: OSPFv3 und der IPv6-Verkehr laufen dort über die
Link-Local-Adressen der Ports.

Loopback und Transfer vergibt der Helfer automatisch. Die Prefixe dafür tragen die **Tags**
`loopback` (für IPv4 und IPv6) bzw. `transfer` (wie `ospf`/`dhcp` an Interfaces, Import:
[3-tags.yaml](netbox-import/3-tags.yaml)). Die Abfrage liest Tags und Scope der Prefixe.
Kunden-Netze brauchen kein Tag: Angeboten werden die Prefixe mit Scope der Site des Geräts.

### Pools

Alle Prefixe mit demselben Tag und derselben IP-Version bilden einen **Pool**. In Produktion sind das
für `transfer` `172.17.0.0/24` (Richtfunk) und `172.17.1.0/24` (direkt), für `loopback`
`172.16.1.0/24` und `2a02:16a:fe00:ffff:ffff:ffff:ffff:ff00/120`.

- **Gemischte Längen.** Direkt- (`/31`) und Bridge-Verbindungen (`/29`) kommen aus demselben Pool.
  NetBox vergibt per First-Fit und richtet jedes Netz an seiner Grösse aus: Ein `/29` beginnt auf
  einer Achtergrenze, die `/31` davor füllen die Lücke. Lücken entstehen nur durch Löschen, und
  kleinere Netze füllen sie wieder. Die beiden heutigen `/24` reichen zusammen für 256 `/31` oder
  64 `/29`. Die Art der Verbindung ist am Netz selbst nicht mehr abzulesen; sie steht im Kabel
  bzw. im Wireless Link.
- **Reihenfolge im Pool.** Die Prefixe werden nacheinander gefüllt (aufsteigend nach Adresse).
  Ist eines voll, kommt das nächste dran. Ein neues Prefix mit dem Tag erweitert den Pool, ohne
  dass sich am Provisioner etwas ändert.
- **Netzweit.** Die Pools haben keinen Scope; Pools pro Standort oder Mandant gehören zum
  [Self-Service](concepts/self-service.md).
- **Befund.** Gibt es für eine IP-Version keinen Pool oder ist ein Pool fast voll, meldet das der
  Befund (`PoolMissing`, `PoolNearlyFull`). Der Helfer vergibt dann nur, was noch geht.

## Technik

- **Schreiben über die REST-API.** NetBox' GraphQL kann nur lesen. `backend/src/netbox/rest.rs`
  (reqwest) liest und schreibt unter `api/`; GraphQL kann auch keine freien Adressen auflisten.
- **Eigene Freigabe.** `allow_netbox_write` (wie `allow_apply`) und ein eigenes Token
  `netbox_write_token` mit Schreibrechten (`NetboxWritesAllowed`). Ohne beide läuft der
  Provisioner nur lesend, die Befunde gehen trotzdem. Jeder angemeldete Benutzer darf schreiben,
  wie in NetBox selbst; Abstufungen gehören zum [Self-Service](concepts/self-service.md).
- **Wer war es.** Jedes Schreiben trägt `changelog_message` mit Aktion und Benutzer aus dem
  OIDC-Token (z.B. „Verbindung np1–np2 (koa)“). Im Changelog von NetBox steht so, wer im
  Provisioner was ausgelöst hat, auch wenn alle Änderungen mit demselben Token laufen.
- **Keine Transaktion, aber Reihenfolge und Rückweg.** NetBox' REST-API hat keine Transaktion über
  mehrere Objekte. Der Plan schreibt in einer Reihenfolge, nach der jeder Zwischenstand gültig ist
  (erst Prefix, dann Adressen, dann Primary IP). Das Schreiben ist ein Job; was geschrieben ist,
  steht in seinen Events (`NetboxWritten`, mit den alten Werten), und „Rückgängig“ ist ein eigener
  Job, der sie rückwärts abarbeitet: Angelegtes löschen, alte Werte zurück, Gelöschtes aus dem
  JSON wieder anlegen, das NetBox vorher hatte. Ein wieder angelegtes Objekt bekommt eine neue Id;
  was danach zurückkommt (die Adressen eines gelöschten VLAN-Interfaces, ein Kabel, ein Wireless
  Link, `bridge`/`parent` eines Ports), verweist auf die neue. Nach einem Neustart des
  Provisioners geht „Rückgängig“ nicht mehr, dann bleibt NetBox' Changelog. Bricht ein Plan
  mittendrin ab, sagt der Helfer „Nur teilweise geschrieben“.
- **Gleichzeitige Änderungen.** Der Schreib-Job rechnet den Plan neu und vergleicht ihn mit dem
  gezeigten (`fingerprint`). Vor jedem Schritt prüft er den Wert, auf dem der Schritt aufbaut:
  `lo` fehlt noch, die Adresse ist noch frei, die Primary IP ist noch die alte. Das erkennt genau
  die Änderungen, die der Plan überschreiben würde, und nicht jede andere an denselben Objekten.
- **Danach neu laden.** Nach dem Schreiben oder Rückgängig lädt der Provisioner die Topologie neu.
- **Tests:** die Vorschau auf dem gespeicherten Export
  (`helper::test::preview_has_what_the_plan_creates`, `preview_has_the_new_link`), die Pläne pro
  Helfer (`helper::connection::test`, …); im Lab-NetBox (`local/lab/`) von Hand, wie bei jedem
  Helfer oben beschrieben. Ein automatischer Lab-Test fehlt (E9).
