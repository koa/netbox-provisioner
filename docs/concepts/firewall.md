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

**Ein Gateway pro Netz.** Hätte ein zweiter Router im selben VLAN eine Adresse und `ospf`,
kündigte er das Netz ebenfalls an: Verkehr käme über ihn herein, die Antworten gingen über das
Gateway der Clients hinaus. Das ist asymmetrisch; Connection Tracking sähe auf jedem Router nur
eine Richtung und verwürfe sie als `invalid`, und die Regeln auf dem Gateway würden umgangen.
Das kommt nicht vor: Mehr als ein Gerät mit einer Adresse in einem VLAN, auch über VXLAN, ist
der Fehler `VlanWithSeveralGateways` (`topology::vlan_domain`); ein Switch, der das VLAN nur auf
L2 weitergibt, hat dort keine Adresse. `MultipleNetworksOnVlan` (Generator) lehnt zusätzlich
mehrere verschiedene Netze in einem VLAN ab.

**In NetBox:** ein Tag am Prefix des Netzes (2026-11-23). „Gäste-Netz“ und „geschützt“ sind
Eigenschaften des Netzes, nicht des Ports: Das Interface ist nur der Ort, an dem der Router die
Regeln umsetzt, und den findet der Generator selbst, als das Interface mit einer Adresse im
Prefix (eindeutig, ein Gateway pro Netz). So folgt der Schutz dem Netz, wenn sein Gateway
wandert (Umstecken, ein neuer Router, der [L2-Extender](l2-extender.md), der die L3-Funktionen
auf den L3-Switch verschiebt); am Interface müsste jeder Helfer den Tag mitnehmen, und ein
vergessener machte ein Gäste-Netz unbemerkt offen. Die Freigaben verweisen ebenfalls auf
Prefixe, alles ist Netz zu Netz, und Anwohner legen im Self-Service Netze an, keine Interfaces.

- **IPv4 und IPv6** sind zwei Prefixe. Ein Netz ist `guest` oder `protected`, sobald eines
  seiner Prefixe den Tag trägt: die strengere Seite gewinnt, ein vergessener Tag öffnet nichts.
  Verschiedene Tags an den Prefixen eines Netzes sind ein Befund.
- **Nicht vererbt:** Der Tag gilt nur am Prefix, in dem die Adresse des Gateways liegt, nicht
  für die Prefixe darunter. Vererben (etwa „alle Anwohner-Netze in einem Bereich“) lässt sich
  später ergänzen, ist aber schwerer nachzuvollziehen.
- **Prefix Pflicht:** Ein Netz mit internen Regeln braucht sein Prefix in NetBox (bisher nur für
  DHCP); bei Kunden-Netzen ist es ohnehin da.

- **`guest`: Gäste-Netz.** Darf ins Internet, an seinem Gateway nur DHCP, DNS und ICMP, und
  intern nur, was freigegeben ist:
  - ganze Netzbereiche: ein Custom Field `guest_access` (Objekte → IP-Prefixe) am Prefix;
  - einzelne Dienste: Services mit Tag `guest` (z.B. der Drucker, ein Medienserver).

  Regeln am Gateway-Router:
  - `forward in-interface=<Netz>`: Freigaben annehmen, alle internen Prefixe verwerfen (eine
    Address-List aus den Prefixen der NetBox, IPv4 und IPv6), den Rest (Internet) annehmen;
  - `input in-interface=<Netz>`: nur DHCP, DNS, ICMP, Rest verwerfen. Gäste kommen nicht an das
    Management des Routers.
- **`protected`: geschütztes Netz.** Von innen nach aussen frei, von anderen internen Netzen
  her gesperrt, ausser freigegeben:
  - ein Custom Field `protected_access` (Objekte → IP-Prefixe) am Prefix: Netze, die herein dürfen
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
- Befunde: ein `guest`- oder `protected`-Prefix, dessen Gateway-Interface `ospf` hat (über den
  Port wird ein Router erreicht); eines ohne Gateway-Adresse; verschiedene Tags an den
  Prefixen eines Netzes.
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
  `protected_access`), dazu Services mit Tag `guest`/`protected`. Überholt am 2026-11-23, siehe
  unten.

## Entschieden (2026-11-23)

- `guest`/`protected` und die Custom Fields `guest_access`/`protected_access` hängen **am
  Prefix**, nicht am Gateway-Interface; das Gateway findet der Generator. Ein Netz gilt als
  `guest`/`protected`, sobald eines seiner Prefixe (IPv4, IPv6) den Tag trägt; nicht vererbt.
