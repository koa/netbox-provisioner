# Konzept: Firewall aus NetBox

Status: Konzept; umgesetzt sind die ersten drei Schritte der [Reihenfolge](#reihenfolge), nur
eigene Regeln verwalten (A1), die Perimeter-Firewall (C1) und die Freigaben aus Services (C2),
beschrieben in [firewall.md](../firewall.md). Offen ist C3 in [todo.md](../todo.md).

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

## Perimeter-Firewall

Gebaut (C1, C2), beschrieben in [firewall.md](../firewall.md). Noch offen: Die von Hand gebauten
Freigaben von router-faesigrund-keller (Webserver, IPsec) können jetzt aus NetBox kommen.

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

1. ~~Nur eigene Regeln verwalten~~ (A1, erledigt 2026-10-26).
2. ~~Perimeter-Firewall~~ (C1, erledigt 2026-10-30).
3. ~~Port-Forwarding IPv4 und IPv6-Freigaben aus Services~~ (C2, erledigt 2026-10-30).
4. Interne Firewall `guest`, dann `protected`, je mit CHR-Test (Gast erreicht das Internet und
   die Freigabe, sonst nichts; geschütztes Netz nur von der Freigabe).

## Entschieden (2026-10-23)

- Eigene Regeln erkennt der Provisioner am **Kommentar-Präfix**; fremde bleiben stehen.
- Ein Interface zum Internet trägt das neue Tag **`internet`**; `nat` setzt es voraus, `nat` ohne
  `internet` ist ein Befund.
- Port-Forwarding setzt **keine Ports um**: aussen gilt derselbe Port wie innen, kein
  `public_port`.
- Freigaben der internen Firewall als **Custom Fields am Interface** (`guest_access`,
  `protected_access`), dazu Services mit Tag `guest`/`protected`.
