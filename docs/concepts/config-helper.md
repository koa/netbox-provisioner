# Konzept: Config-Helper

Status: Schritt 1 (Befund „NetBox in sich“) umgesetzt (`topology/findings.rs`, Befunde oben auf der
Site-Seite, Netz-Befunde auf der Site-Übersicht, Anzahl an den Karten), ohne die PoE-Prüfung, WLAN-Gruppen,
Regionen und die gesammelten Generatorfehler. Der Rest ist Konzept. Offener Punkt „Config-Helper“ aus
[pending.md](../pending.md).

## Ausgangslage

NetBox ist die Quelle der Wahrheit, aber die Daten sind in schlechtem Zustand. In der
gespeicherten Produktionsantwort (`docs/netbox-response-example.json`, Stand des letzten Exports):

| Befund | Anzahl |
|---|---|
| RouterOS-Geräte | 41 |
| davon ohne Primary IPv4 (also ohne OSPF, ohne VTEP) | 15 |
| davon ohne Loopback `lo` | 14 |
| Adressen an Interfaces | 201 |
| davon ohne Prefix gleicher Länge (für DHCP Pflicht) | 73 |
| physische Ports ohne Kabel | 167 |
| verkabelte Ports mit IP, aber ohne `ospf` | 9 |

Der Generator erzeugt für alle 41 Geräte etwas (`KNOWN_FAILURES` ist leer). Die Lücken fallen
also nicht als Fehler auf, sondern als Konfiguration, die etwas anderes tut als das Netz heute:
fehlende OSPF-Router-ID, ein DHCP-Server, der nicht entsteht, ein Port ohne sprechenden Namen.
Die Regeln, wie man richtig modelliert, stehen in [netbox-modeling.md](../netbox-modeling.md).
Sie sind zahlreich und verteilen sich über viele NetBox-Formulare. Das von Hand richtig zu machen,
ist der Engpass.

## Ziel

Zwei Dinge im Provisioner, in dieser Reihenfolge:

1. **Befund:** pro Site eine Liste, was in NetBox fehlt oder nicht zusammenpasst. Dazu, wo der
   Provisioner es lesen kann, was das echte Gerät anders macht. Nur lesen, sofort nützlich.
2. **Helfer:** für die Standardfälle ein kurzer Ablauf im UI, der die nötigen NetBox-Objekte
   anlegt oder ändert. Der Provisioner schreibt dabei nur in NetBox, nie auf ein Gerät. Auf das
   Gerät kommt es wie heute über die Geräteseite („Anwenden“).

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

## 1. Befund

Eine neue Seite pro Site (und eine Spalte „Befunde“ in der Geräteliste). Zwei Quellen:

### NetBox in sich

Prüfungen, die nur NetBox brauchen. Sie lassen sich aus der Topologie rechnen, die der
Provisioner ohnehin lädt. Fast jede Regel aus `netbox-modeling.md` wird eine Prüfung:

- Gerät mit Platform `routeros` ohne `lo`, ohne Primary IPv4, oder Primary IPv4 nicht auf `lo`.
- Adresse ohne Prefix gleicher Länge, Prefix ohne Prefix-Rolle (siehe unten).
- Kabel, dessen Enden nicht im selben Netz liegen, nur ein Ende `ospf` hat oder ungleiche MTU.
- Verkabelter Port eines RouterOS-Geräts mit IP, aber ohne `ospf` (Hinweis, kein Fehler: es gibt
  Kunden-Ports).
- PoE: `PSE` an einem Port, dessen Board dort kein PoE-out hat (`hw_facts.rs`), oder am anderen
  Kabelende kein Gerät.
- Interface-Namen, die wie ein Port aussehen, aber keiner sind (`ether3-alt`, `qsfpplus1`).
- VLAN ohne VLAN-Gruppe, WLAN-Gruppe ohne Controller, Site ohne Land in der Region.
- Alles, woran der Generator scheitert, gesammelt über alle Geräte, statt eines Fehlers pro
  Seitenaufruf.

Jeder Befund hat Schwere (Fehler: Generator scheitert oder macht etwas Falsches / Warnung /
Hinweis), Objekt mit Link nach NetBox und, wenn es dafür einen Helfer gibt, den Knopf dazu.

Umgesetzt und abweichend vom Entwurf oben:

- `ospf` nur an einem Ende und „verschiedene Netze“ werden nur geprüft, wenn beide Enden (der Port
  oder seine Bridge) IPv4 haben. Ein Brückengerät ohne Adresse arbeitet nur auf L2, das OSPF des
  Routers geht hindurch zum Router auf der anderen Seite. Im Export waren das zehn falsche Treffer.
- Ohne Primary IPv4 ist ein Fehler, wo das Gerät `ospf` hat (Router-ID), sonst eine Warnung.
- „Prefix ohne Prefix-Rolle“ entfällt, die Pools kommen über Tags.

Im gespeicherten Export, mit gesetzten Tags `loopback`/`transfer`: 16 Geräte ohne Primary IPv4 und 14 ohne
`lo` (die Brückengeräte, ein AP, ein Switch-Paar), eine Adresse ohne Prefix, ein Hinweis auf eine
Verbindung ohne `ospf` (`lkw-switch-20`–`lte-16`). Den Stand schreibt der Test
`findings_of_netbox_example` nach `target/netbox-example/findings.txt`.

### NetBox gegen Gerät

Für erreichbare Geräte liest der Provisioner das Gerät nur (wie heute `fetch_config`) und
vergleicht:

- **Nachbarn → Kabel.** `/ip/neighbor` (MNDP/LLDP) sagt, welches Gerät an welchem Port hängt. Fehlt
  das Kabel in NetBox oder geht es anderswohin, wird das ein Befund mit Vorschlag „Kabel
  anlegen“. Das ist der grösste Hebel bei 167 Ports ohne Kabel. Die Nachbarschaft wird über den
  Namen des Nachbarn (Identity = NetBox-Name) und seinen Port zugeordnet, nicht über die MAC.
- **Adressen.** Eine Adresse auf dem Gerät, die in NetBox fehlt, oder umgekehrt.
- **OSPF.** Ein Nachbar, der auf dem Gerät `Full` ist, über einen Port ohne `ospf` in NetBox.
- **PoE.** Ein Port, der laut Gerät gerade speist (`poe-out-status=powered-on`), in NetBox aber
  nicht `PSE` ist.
- **Board.** Das gemeldete Board gegen den Device Type in NetBox.

Wie der Generator heute schon das Board live liest, ist das Gerät hier die Wahrheit über das,
was *ist*. NetBox ist die Wahrheit über das, was *sein soll*. Der Befund zeigt beides
nebeneinander. Welche Seite recht hat, entscheidet der Mensch: „In NetBox übernehmen“ (Helfer)
oder „Gerät anpassen“ (Anwenden).

Das Lesen kostet pro Gerät ein paar Sekunden über den Management-Proxy. Der Befund einer Site
läuft deshalb als Job (Events pro Gerät), nicht als einzelne Abfrage.

## 2. Helfer

Die Fälle aus `pending.md`. Jeder ist ein kurzes Formular, das einen Plan erzeugt.

### Neues Gerät

Eingabe: Site, Name, Board. Das Board wird **erkannt**, wenn das Gerät schon am Kabel eines
erreichbaren Nachbarn hängt (die Kandidaten der Ersteinrichtung zeigen Board und Identity), sonst
aus der Liste der Boards in `hw_facts.rs` gewählt.

Plan: Device (Platform `routeros`, Device Type zum Board, Rolle, Tenant der Site), dazu die
Interfaces mit den RouterOS-Namen aus `hw_facts.rs` (legt NetBox sie über den Device Type schon an,
prüft der Helfer nur die Namen) und `lo` (Typ Virtual). Dann direkt weiter zu „Loopback“ und
„Verbindung“.

### Loopback

Nächste freie `/32` aus dem Loopback-Pool (siehe [Pools](#pools)), auf `lo`, als
Primary IPv4, dazu eine `/128` aus dem IPv6-Loopback-Pool als Primary IPv6. NetBox vergibt
freie Adressen selbst und atomar (`POST /api/ipam/prefixes/{id}/available-ips/`), zwei
gleichzeitige Helfer bekommen also nicht dieselbe.

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

Bevor der Helfer `/31` vergibt, ist auf einer CHR zu prüfen, dass RouterOS damit tut, was es soll:
Adresse, OSPF-Nachbarschaft und Ersteinrichtung über einen Nachbarn im `/31`. Ebenso, dass der
Generator damit umgehen kann (Ersteinrichtung sucht den Nachbarn „im selben Netz“, der DHCP-Pool
rechnet freie Adressen).

### Kunden-Netz

Ein Port oder ein VLAN wählen, dann das **Parent-Prefix**: Der Helfer bietet die Prefixe der Site
des Geräts an (Kunden-Netze gehören zum Standort), gewählt wird von Hand, nicht automatisch.
Plan: nächstes freies Netz darin (Länge wählbar, Vorgabe `/24`), Gateway-Adresse (erste Adresse) am Port bzw. an einem neuen virtuellen
VLAN-Interface, `dhcp`, optional eine IP-Range mit Rolle `dhcp`.

### OSPF- und DHCP-Flags

Eine Tabelle aller Interfaces eines Geräts mit den Tags `ospf`, `dhcp`, `dhcp-client`, `nat` als
Schalter. Die Prüfungen von oben laufen live mit, etwa „`ospf` nur an einem Kabelende“.

### PoE

Die PoE-Ketten der Site als Baum (wie `PowerChain` sie heute schon für Updates rechnet): wer
speist wen. Ändern heisst `PSE` am speisenden und `PD` am gespeisten Port setzen. Erlaubt ist das
nur, wo das Board PoE-out hat und das Kabel existiert. Der Befund „Gerät speist hier, NetBox weiss
es nicht“ führt direkt hierher.

### Funkbrücke

Eingabe: die beiden Standorte bzw. die beiden Router, an deren Ports die Brücke hängt, das Board
(Auswahl aus den Funk-Boards in `hw_facts.rs`, 5 oder 60 GHz), optional Frequenz und Breite.

Plan für beide Seiten: Device mit Board und Name (Vorschlag `<site-a>-<site-b>` und umgekehrt),
`ether1`, Funk-Port (`wlan1` bzw. `wlan60-1`), Bridge mit beiden; RF Role `AP` auf der einen
Seite, `Station` auf der anderen; ein Wireless Link mit SSID (Vorschlag aus den Namen),
WPA Personal und einem erzeugten PSK; die Kabel von `ether1` zu den Router-Ports; je eine
Loopback und eine feste Adresse im `/29` ([Brückengeräte mit fester
Adresse](#brückengeräte-mit-fester-adresse)). Dazu die [Verbindung](#verbindung) zwischen den
beiden Routern über die Brücke (`/29`, `ospf`, MTU über die ganze Strecke gleich, wie
[netbox-modeling.md](../netbox-modeling.md#mtu) es verlangt).

### Brückengeräte mit fester Adresse

Heute holen sich die Brückengeräte ihre Adresse per DHCP-Client von einem der beiden Router
([netbox-modeling.md](../netbox-modeling.md#funkbrücke)). Funktioniert dieser Router nicht
richtig, bekommen sie keine Adresse mehr und sind gerade dann nicht erreichbar, wenn man sie
zur Fehlersuche braucht.

Der Helfer legt die Brückengeräte deshalb wie Router an, nur ohne Routing der Nutzdaten:

- **Loopback** `/32` und `/128` aus den Loopback-Pools als Primary IPv4 (Router-ID) und
  Primary IPv6 (Management-Adresse).
- **Feste Adresse im `/29`** auf der Bridge des Brückengeräts, Tag `ospf` an der Bridge.
- **OSPF-Priorität 0** auf dem Brückengerät: Es wird nie DR oder BDR, die Rolle bleibt bei den
  Routern. Das braucht eine neue Regel im Generator. Das Feld `priority` von
  `/routing/ospf/interface-template` kennt `mikrotik-model` schon.

Warum das hilft:

- Ohne DHCP-Abhängigkeit bleibt ein Brückengerät erreichbar, solange einer der beiden Router es
  erreicht.
- Fällt die Funkstrecke aus, zerfällt das `/29` in zwei Hälften. Beide Router kündigen es weiter an,
  eine Adresse im `/29` wäre also nur zufällig richtig geroutet. Die Loopback dagegen kündigt nur
  die Seite an, auf der das Brückengerät noch hängt. So ist jedes Brückengerät auch bei
  unterbrochener Strecke von seiner Seite her erreichbar. Das ist der Fall, in dem man es am
  dringendsten braucht.

Die Nutzdaten bleiben auf L2: Die beiden Router sind im selben `/29` direkt Nachbarn und schicken
sich ihre Pakete direkt zu. Die Bridge des Brückengeräts leitet sie weiter wie heute. Zur CPU des
Brückengeräts kommt nur, was an seine eigenen Adressen geht (Management, OSPF-Hellos). Das
Brückengerät hat nur ein L3-Interface und kann deshalb keinen Transit-Verkehr routen. Weiterhin
gilt: Die Bridge darf keine IP-Firewall für gebridgten Verkehr einschalten (`use-ip-firewall=no`,
der Standard).

Zu prüfen auf CHR, bevor der Helfer das anlegt: vier OSPF-Sprecher in einem Segment, Priorität 0,
die Strecke getrennt (Loopback von jeder Seite erreichbar) und wieder verbunden.

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
[3-tags.yaml](../netbox-import/3-tags.yaml)). Die Abfrage liest Tags und Scope der Prefixe.
Kunden-Netze brauchen kein Tag: Angeboten werden die Prefixe mit Scope der Site des Geräts.

### Pools

Alle Prefixe mit demselben Tag und derselben IP-Version bilden einen **Pool**. Heute sind das
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
- **Pro Standort oder Mandant (später).** Ein Prefix mit dem Tag kann einen Scope (Location,
  Site, Region) haben. Für ein Gerät gilt dann der Pool mit dem nächstgelegenen Scope: zuerst
  Prefixe seiner Location, dann seiner Site, dann seiner Regionen nach oben, zuletzt die ohne
  Scope. Ebenso später der Tenant des Prefixes. Für eine Verbindung zwischen zwei Standorten
  gilt der Pool, der für beide Geräte am nächsten liegt (gemeinsame Region, sonst ohne Scope).
  Umgesetzt wird zuerst nur „ohne Scope“. Die Reihenfolge ist aber so gewählt, dass sie mit den
  heutigen netzweiten Prefixen übereinstimmt.
- **Befund.** Gibt es für eine IP-Version keinen Pool oder ist
  ein Pool fast voll (Vorschlag: unter 10 % frei), meldet das der Befund. Der Helfer vergibt dann
  nur, was noch geht.

## Technik

- **Schreiben über die REST-API.** NetBox' GraphQL kann nur lesen. Ein neues Modul
  `backend/src/netbox/write.rs` (reqwest, wie die Abfrage) mit den paar Endpunkten, die die
  Helfer brauchen: `dcim/devices`, `dcim/interfaces`, `dcim/cables`, `ipam/ip-addresses`,
  `ipam/prefixes/{id}/available-ips|available-prefixes`, `ipam/ip-ranges`,
  `wireless/wireless-links`, Tags.
- **Eigene Freigabe.** `allow_netbox_write` (wie `allow_apply`) und ein eigenes Token
  `netbox_write_token` mit Schreibrechten. Ohne beide läuft der Provisioner nur lesend, der Befund
  geht trotzdem.
- **Berechtigungen vorbereitet, noch nicht unterschieden.** Heute darf jeder angemeldete Benutzer
  NetBox ändern, so wie in NetBox selbst. Später soll es abgestuft werden: Self-Service für
  Nutzer, den Systemzustand sehen für alle, Systemänderungen nur für Admins. Damit das ohne Umbau
  geht, gehört jede Helfer-Aktion (und jede bestehende Mutation) schon jetzt zu einer Klasse, etwa
  `read`, `self_service`, `network`, `admin`. Geprüft wird an einer Stelle (wie heute
  `WritesAllowed` für `allow_apply`). Vorerst lässt diese Prüfung alle Klassen für alle durch.
  Später kommen die Klassen aus einem Claim des OIDC-Tokens (z.B. `groups`).
- **Wer war es.** NetBox 4.7 nimmt `changelog_message` bei jedem Schreiben an. Der Helfer setzt
  dort Aktion und Benutzer aus dem OIDC-Token (z.B. „Verbindung np1–np2 (koa)“). Im Changelog
  von NetBox steht so, wer im Provisioner was ausgelöst hat, auch wenn alle Änderungen mit
  demselben Token laufen.
- **Keine Transaktion, aber Reihenfolge und Rückweg.** NetBox' REST-API hat keine Transaktion
  über mehrere Objekte. Der Plan schreibt in einer Reihenfolge, nach der jeder Zwischenstand gültig
  ist (erst Prefix, dann Adressen, dann Primary IP). Er merkt sich, was angelegt wurde und welche
  Werte vorher galten. Bricht er ab, zeigt das UI, was geschrieben ist, und bietet „Rückgängig“
  an: Angelegtes löschen, alte Werte zurück. Läuft als Job mit Events, wie Anwenden.
- **Gleichzeitige Änderungen.** Vor dem Schreiben vergleicht der Plan `last_updated` der Objekte,
  die er ändert, mit dem Stand der Vorschau. Hat jemand inzwischen in NetBox geändert, bricht er
  ab, statt zu überschreiben.
- **Danach neu laden.** Nach dem Schreiben lädt der Provisioner die Topologie neu. Die Geräteseite
  zeigt dann das neue Skript.
- **Testen** gegen das Lab-NetBox (`local/lab/`): Jede Helfer-Aktion bekommt einen Test, der sie
  dort ausführt, die Vorschau mit dem Ergebnis vergleicht und alles wieder zurücknimmt.

## Reihenfolge

1. **Befund „NetBox in sich“.** Nur lesen, keine neue Berechtigung, sofort ein Überblick über die
   Datenlage. Klein, weil es auf der vorhandenen Topologie rechnet.
2. **Befund „NetBox gegen Gerät“**, zuerst Nachbarn → Kabel.
3. **Schreib-Infrastruktur** (Plan, Vorschau mit Generator, Changelog, Rückgängig) mit dem
   einfachsten Helfer: **Loopback**.
4. **Verbindung** und „Kabel aus Befund übernehmen“.
5. **OSPF-/DHCP-Flags**, **Kunden-Netz**.
6. **Neues Gerät**, **PoE**.
7. **Funkbrücke**.

Schritte 1 und 2 helfen auch dann, wenn jemand weiter von Hand in NetBox pflegt.
