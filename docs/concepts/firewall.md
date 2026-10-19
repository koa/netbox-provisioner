# Konzept: Firewall aus NetBox

Status: Konzept, nicht umgesetzt. User Journey „Firewall“ aus
[pending.md](../pending.md#firewall).

## Zwei Arten

| | Perimeter-Firewall | Interne Firewall |
|---|---|---|
| wo | jeder Router mit Zugang zum Internet | der Gateway-Router einzelner Netze |
| Pflicht | ja, ohne Ausnahme | nein, nur in Ausnahmefällen |
| NAT | IPv4-NAT (Masquerading, Port-Forwarding) | keins |
| Zweck | Netz und Router gegen das Internet schützen; SSH erreichbar, mit Bruteforce-Schutz | Gäste-Netze auf bestimmte Ziele beschränken; geschützte Netze gegen internen Zugriff sperren |

Eine interne Firewall kostet Leistung: Ihre Regeln brauchen Connection Tracking auf dem Router,
der sonst nur routet. Deshalb nur dort, wo ein Netz es verlangt, und nur auf dem Router, an dem
das Netz hängt.

## Ausgangslage

Der Generator schreibt heute nur zwei Arten von Regeln, beide an Uplinks mit Tag `nat`:

- `srcnat action=masquerade out-interface=<Uplink>` (`masquerade`);
- `input` `drop` für UDP und TCP 53 vom Uplink (`close_dns`), damit der DNS-Resolver nicht offen ist.

`mikrotik-model` kennt `/ip/firewall/filter`, `/ip/firewall/nat`, `/ipv6/firewall/filter` und
`/ipv6/firewall/address-list`; das Ziel enthält sie (`ip_firewall_filter` …, nach `.id`
abgeglichen). **Zu prüfen, bevor das hier gebaut wird:** Was die Vorschau eines der heutigen
Firewall-Router zu seinen Regeln zeigt. Verwaltet das Ziel die ganze Tabelle, würde ein Apply
von Hand gesetzte Regeln entfernen. Das wäre schon heute ein Risiko, nicht erst mit diesem
Konzept.

**Heutiger Stand, gemessen** (2026-10-19, Lab und Code):

- Die Firewall-Tabellen werden nach `.id` abgeglichen (`generate_add_update_remove_by_id`):
  Einträge auf dem Gerät werden mit den Zielen gepaart, die wenigsten geänderten Felder zuerst,
  und **umgeschrieben**; was übrig bleibt, wird **entfernt**. Ein Apply auf einen Router mit
  eigener Firewall (die Werks-Firewall eines hEX, oder die von Hand gepflegten Regeln der
  Internet-Router) lässt nur `close_dns` übrig. Die Reihenfolge der Regeln spielt beim Paaren
  keine Rolle, für RouterOS aber schon.
- Die Lab-Geräte haben nach dem Anwenden **keine einzige Filter-Regel** (IPv4 und IPv6).
- `/ip/service` verwaltet der Generator nicht: ftp, telnet, www, winbox, api (ohne TLS) bleiben
  auf allen Geräten an, auf allen Adressen.
- `/ip/socks` des Management-Proxys lauscht auf allen Interfaces (`setup_management_proxy`:
  „reachable from wherever the device is“), geschützt nur durch das Passwort.

Ein Router mit Internet-Zugang ist damit nach einem Apply von aussen offen, bis auf DNS. Die
Perimeter-Firewall ist deshalb nicht nur eine Erweiterung, sondern behebt ein bestehendes
Problem; und bis sie da ist, sollte kein Router mit eigener Firewall angewendet werden.

## Perimeter-Firewall

**Wo:** an jedem Interface zum Internet. Heute markiert das Tag `nat` einen Uplink, dessen Netz
nicht zurückroutet. Ein Uplink mit öffentlichem, zurückgeroutetem Netz (IPv6, oder IPv4 ohne NAT)
braucht die Firewall genauso. Vorschlag: ein Tag `internet` für jedes Interface zum Internet;
`nat` setzt es voraus. Weil die Perimeter-Firewall Pflicht ist, gibt es sie dort immer, ohne
Schalter; ein Interface mit `nat` ohne `internet` ist ein Befund.

**Regeln**, für IPv4 und IPv6 gleich aufgebaut, aus dem Generator (nicht aus NetBox):

- `input` und `forward`: `established,related` annehmen, `invalid` verwerfen.
- `input` vom Internet:
  - ICMP bzw. ICMPv6 annehmen (IPv6 braucht ND und Path-MTU, mit Rate-Limit);
  - SSH (22) annehmen, durch den Bruteforce-Schutz;
  - der Zugang des Provisioners, falls er über das Internet kommt (SOCKS des
    `management_proxy` bzw. das VPN, nur von seiner Adresse). Fehlt der, sperrt sich der
    Provisioner beim Anwenden selbst aus; der Fallback holt das Gerät zurück, aber ohne
    Änderung;
  - der Rest wird verworfen: API, Winbox, DNS (das heutige `close_dns` geht darin auf).
- **Dienste:** `/ip/service` ins Modell; telnet, ftp, www aus, API nur von den eigenen Netzen
  (`address=`), besser api-ssl. Das gilt für alle Geräte, nicht nur am Perimeter.
- `forward` vom Internet:
  - IPv4 nur, was ein Port-Forwarding hereinlässt (`connection-nat-state=dstnat`);
  - IPv6 nur die Freigaben;
  - der Rest wird verworfen.
- **Bruteforce-Schutz** für SSH nach der
  [MikroTik-Anleitung](https://manual.mikrotik.com/docs/firewall-and-quality-of-service/user-guides/bruteforce-prevention),
  für IPv4 und IPv6. Eine neue Verbindung kommt in die Address-List `ssh_stage1`, eine weitere
  in kurzer Zeit nach `ssh_stage2`, `ssh_stage3`, dann `ssh_blacklist` (z.B. 10 Tage). Was auf
  der Blacklist steht, wird am `input` verworfen. Die Zeiten als Config Context übersteuerbar.

**NAT** (nur IPv4):

- Masquerading ausgehend wie heute (`nat`).
- **Port-Forwarding** aus NetBox **Services** (IPAM → Services: Protokoll, Ports, IP-Adressen,
  Gerät). Ein Service an der **inneren** IPv4 eines Geräts im Kunden-Netz, mit Tag `forward`,
  ergibt auf dem Perimeter-Router, hinter dem das Netz liegt,
  `dstnat in-interface=<Uplink> protocol=… dst-port=<aussen> action=dst-nat to-addresses=<innen> to-ports=<port>`.
  Gilt aussen ein anderer Port, steht er in einem Custom Field `public_port`. Der Router ist der
  mit `nat`-Uplink auf dem Weg aus dem Netz hinaus; gibt es keinen eindeutigen, ist das ein Befund.
- **IPv6-Freigaben** aus demselben Modell: ein Service an einer IPv6-Adresse mit Tag `forward`
  ergibt auf jedem Perimeter-Router `forward accept dst-address=<adresse> protocol=… dst-port=…`.
- Ein Service ohne `forward` bleibt Doku und erzeugt nichts.

## Interne Firewall

**Wo:** auf dem Router, an dessen Interface das Netz hängt (sein Gateway). Mit OSPF geht jeder
Verkehr in das Netz und aus ihm über dieses Interface, auch der Rückweg. Die Regeln brauchen
deshalb nur dort zu stehen, und die übrigen Router bleiben ohne Connection Tracking.

**Ausnahme: ein Netz auf mehreren Routern.** Ein über VXLAN getunneltes VLAN darf auf mehreren
Geräten Adressen desselben Netzes haben (Gateway plus Switch, `MultipleNetworksOnVlan` erlaubt
das). Hat ein zweiter Router dort eine Adresse und `ospf`, kündigt er das Netz ebenfalls an:
Verkehr kommt dann über ihn herein, die Antworten gehen über das Gateway der Clients hinaus. Das
ist asymmetrisch; Connection Tracking sieht auf jedem Router nur eine Richtung und verwirft sie
als `invalid`, und die Regeln auf dem Gateway werden umgangen. Für `guest` und `protected` darf
deshalb nur **ein** Router das Netz routen; mehr ist ein Befund.

**In NetBox:** ein Tag am Gateway-Interface, wie `dhcp` oder `ospf`. So gilt er für IPv4 und
IPv6 des Netzes zusammen.

- **`guest`: Gäste-Netz.** Darf ins Internet, an seinem Gateway nur DHCP, DNS und ICMP, und
  intern nur, was freigegeben ist:
  - ganze Netzbereiche: ein Custom Field `guest_access` (Objekte → IP-Prefixe) am Interface;
  - einzelne Dienste: Services mit Tag `guest` (z.B. der Drucker, ein Medienserver).

  Regeln am Gateway-Router:
  - `forward in-interface=<Netz>`: Freigaben annehmen, alle internen Prefixe verwerfen (eine
    Address-List aus den Prefixen der NetBox, IPv4 und IPv6), den Rest (Internet) annehmen;
  - `input in-interface=<Netz>`: nur DHCP, DNS, ICMP, Rest verwerfen. Gäste kommen nicht an das
    Management des Routers.
- **`protected`: geschütztes Netz.** Von innen nach aussen frei, von anderen internen Netzen
  her gesperrt, ausser freigegeben:
  - ein Custom Field `protected_access` (Objekte → IP-Prefixe): Netze, die herein dürfen
    (z.B. das Admin-Netz);
  - Services mit Tag `protected` an Adressen im Netz: einzelne Dienste, die alle erreichen
    dürfen.

  Regeln am Gateway-Router: `forward out-interface=<Netz> connection-state=new`, Freigaben
  annehmen, Rest verwerfen. Antworten auf Verbindungen von innen gehen als `established` durch.
- **Kein NAT.** Die Regeln sind reine Filter.
- **Leistung:** `/ip firewall connection tracking` bleibt auf `auto`; nur Router mit internen
  Regeln tracken. FastTrack für `established` mildert die Kosten. Die Geräteseite zeigt an,
  dass auf diesem Router eine interne Firewall läuft.

## Was der Provisioner selbst braucht

- Die Ersteinrichtung und das Anwenden gehen über die Adresse am Kabel und die Loopback. Interne
  Regeln dürfen den Weg des Provisioners zu den Routern nicht sperren: die Loopbacks und
  Transfernetze sind nie Ziel einer internen Regel, nur Kunden-Netze.
- Ein Gäste- oder geschütztes Netz an einem Port, über den ein Router erreicht wird, ist ein
  Befund (`ospf` und `guest`/`protected` am selben Interface).
- **DNS der Gäste:** Mit `dns_servers` im Config Context beantwortet der Router keine
  DNS-Anfragen (`allow-remote-requests=no`), die Clients fragen die Server von dort (z.B.
  Blocky im internen Netz). Diese Server gehören deshalb automatisch zu den Freigaben eines
  Gäste-Netzes, sonst haben Gäste kein DNS.

## Reihenfolge

1. Prüfen, was ein Apply heute mit den vorhandenen Regeln eines Firewall-Routers tut (Vorschau).
   Falls es sie entfernt: zuerst Regeln ohne NetBox-Ursprung behalten (z.B. nur Einträge mit
   einem Kommentar-Präfix verwalten), bevor irgendetwas anderes kommt.
2. Perimeter-Firewall (`internet`): Grundregeln, SSH mit Bruteforce-Schutz, Zugang des
   Provisioners. CHR-Test mit Uplink, Router und Client, auch: das Apply sperrt sich nicht aus.
3. Port-Forwarding IPv4 und IPv6-Freigaben aus Services.
4. Interne Firewall `guest`, dann `protected`, je mit CHR-Test (Gast erreicht das Internet und
   die Freigabe, sonst nichts; geschütztes Netz nur von der Freigabe).

## Zu entscheiden

- Neues Tag `internet` für jedes Interface zum Internet, oder genügt `nat` (dann bleibt ein
  Uplink ohne NAT ungeschützt)? Vorschlag: `internet`.
- `public_port` als Custom Field am Service, oder ein zweiter Service am Router?
- Freigaben der internen Firewall als Custom Fields am Interface, oder als eigene Prefix-Tags?
