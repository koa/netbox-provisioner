# Konzept: Config-Helper

Status: Konzept, nicht umgesetzt. Offener Punkt „Config-Helper“ aus [pending.md](../pending.md).

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

Nächste freie `/32` aus dem Loopback-Prefix, auf `lo`, als Primary IPv4. Mit IPv6 ebenso eine
`/128`. NetBox vergibt freie Adressen selbst und atomar (`POST
/api/ipam/prefixes/{id}/available-ips/`), zwei gleichzeitige Helfer bekommen also nicht dieselbe.

### Verbindung

Zwei Ports wählen (oder ein Kabel aus dem Befund übernehmen). Plan: Kabel, falls es fehlt;
nächstes freies Transfernetz (`/30`, `POST …/available-prefixes/`) aus dem Transfer-Prefix; je
eine Adresse an beide Enden; `ospf` an beide Enden; gleiche MTU an beiden Enden (Vorgabe
übernehmen oder eingeben); Label je mit dem Namen der Gegenseite.

### Kunden-Netz

Ein Port oder ein VLAN wählen. Plan: nächstes freies Netz aus dem Kunden-Prefix (Länge wählbar,
Vorgabe `/24`), Gateway-Adresse (erste Adresse) am Port bzw. an einem neuen virtuellen
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
WPA Personal und einem erzeugten PSK; die Kabel von `ether1` zu den Router-Ports. Auf
Wunsch gleich „Verbindung“ zwischen den beiden Routern über die Brücke (Transfernetz, `ospf`, MTU
über die ganze Strecke gleich, wie [netbox-modeling.md](../netbox-modeling.md#mtu) es verlangt).

## Prefix-Konventionen

Damit der Helfer „das“ Loopback-, Transfer- oder Kunden-Prefix findet, muss NetBox es kennzeichnen.
Vorschlag: **Prefix-Rollen** mit festen Slugs, wie die IP-Range-Rolle `dhcp`:

| Rolle | Inhalt | vergeben wird |
|---|---|---|
| `loopback` | z.B. `172.16.1.0/24` | `/32` je Gerät |
| `transfer` | Container für Transfernetze | `/30` je Verbindung |
| `clients` | Container für Kunden-Netze | `/24` (wählbar) je Netz |

Gesucht wird das Prefix mit der Rolle, dessen Scope (Site, Location, Region) am nächsten am Gerät
liegt, sonst ein globales. Die heutigen Daten (30 `/30`, 62 `/24`) sehen nach genau dieser
Aufteilung aus. Ob es Container dafür schon gibt, sieht man in den Daten aber nicht: Die Abfrage
liest heute weder Rolle noch Scope eines Prefixes. Siehe offene Fragen.

## Technik

- **Schreiben über die REST-API.** NetBox' GraphQL kann nur lesen. Ein neues Modul
  `backend/src/netbox/write.rs` (reqwest, wie die Abfrage) mit den paar Endpunkten, die die
  Helfer brauchen: `dcim/devices`, `dcim/interfaces`, `dcim/cables`, `ipam/ip-addresses`,
  `ipam/prefixes/{id}/available-ips|available-prefixes`, `ipam/ip-ranges`,
  `wireless/wireless-links`, Tags.
- **Eigene Freigabe.** `allow_netbox_write` (wie `allow_apply`) und ein eigenes Token
  `netbox_write_token` mit Schreibrechten. Ohne beide läuft der Provisioner nur lesend, der Befund
  geht trotzdem.
- **Wer war es.** NetBox ab 4.4 nimmt `changelog_message` bei jedem Schreiben an. Der Helfer setzt
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

## Offene Fragen

- Gibt es in NetBox schon Container-Prefixe für Loopbacks, Transfernetze und Kunden-Netze? Mit
  Rolle, oder nur als Konvention über Adressbereiche?
- Welche NetBox-Version läuft? (`changelog_message` ab 4.4; das `nbt_`-Token deutet auf 4.5+.)
- Darf der Provisioner ein Token mit Schreibrechten bekommen? Für alle Benutzer aus
  `auth_users`, oder braucht es eine zweite Liste für die, die NetBox ändern dürfen?
- Transfernetze `/30` oder `/31`? RouterOS kann `/31`, die vorhandenen sind `/30`.
