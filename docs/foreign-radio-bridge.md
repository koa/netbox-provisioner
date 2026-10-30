# Funkbrücke ohne RouterOS

Eine Funkstrecke aus zwei Geräten ohne RouterOS (zwei UISP Wave Nano, 60 GHz) zwischen zwei
MikroTiks, L2-transparent, ohne VLANs. Der Provisioner konfiguriert die Funkgeräte nicht, sorgt
aber dafür, dass ihre Management-IPs von beiden Seiten erreichbar bleiben, auch wenn die Strecke
oder ein MikroTik ausfällt. Erledigt als C16 ([done.md](done.md)); in NetBox trägt sie der
Helfer „Funkbrücke“ ein ([config-helper.md](config-helper.md#brückengeräte-ohne-routeros)).

## Die Funkgeräte

Die Wave Nanos werden von Hand eingerichtet (Weboberfläche oder Bluetooth-App, keine Cloud):

- PtP ab Wave-Firmware 3.2.0: eine Seite PtP-AP, die andere Station.
- Bridge-Modus; der Router-Modus macht aus der Strecke einen NAT-Router ohne dynamisches Routing.
- DHCP-Client für das Management.

Nicht möglich (Datenblatt, Stand 2026-10-27): OSPF oder sonst ein Routing-Protokoll, eine
dokumentierte lokale API, SNMP mit Schreibzugriff. SNMP liest nur, taugt also fürs Monitoring
(Funkverbindung, Signal, Kapazität), nicht zum Konfigurieren.

## DHCP je Seite

Jeder MikroTik hat auf dem Port zur Strecke ein eigenes /29 mit DHCP-Server und kurzer Lease
(`dhcp_lease_time` am Prefix, der Helfer setzt 120 s); jede Wave Nano nimmt das Angebot, das
zuerst kommt. Jedes /29 kündigt nur sein MikroTik an, so sind Hin- und Rückweg gleich, ohne
zusätzliche Routen und ohne die Frage der ICMP-Redirects.

Fällt eine Seite weg (MikroTik aus oder Strecke gestört), verlängert die Wave Nano ihre Adresse
nicht mehr: Ab etwa 87,5 % der Lease fragt sie jeden Server, der verbliebene lehnt die fremde
Adresse ab (RouterOS ist standardmässig `authoritative`), und sie holt sich eine aus seinem Netz.
Bei gestörter Strecke landet so jede Wave Nano bei ihrem lokalen MikroTik, bei einem
ausgefallenen MikroTik beide beim anderen. Bis dahin vergeht höchstens eine Lease. Ausfall-Tests
dazu gibt es nicht (Entscheid 2026-10-30).

Kosten:

- Die IPs wechseln; wo welche Wave Nano ist, steht in den Leases der beiden MikroTiks, nicht in
  NetBox.
- OSPFv2 bildet auf einem Broadcast-Segment nur Nachbarschaften im selben Subnetz: Die beiden
  Ports haben zusätzlich ein gemeinsames Transfernetz, ein /31; auf dem Segment liegen dann
  drei Netze. OSPFv3 über Link-Local betrifft das nicht.
- Je ein DHCP-Server auf beiden MikroTiks im selben Segment. Pro Port ist es einer, für alle
  seine Netze: Das /31 lässt keine Adresse frei und bekommt nichts, der Server gehört dem /29
  (und `DhcpPoolEmpty` meldet das /31 nicht).

Die Wave Nanos brauchen passives PoE: `poe_type` `passive-…` gibt `poe-out=forced-on`, `auto-on`
erkennt sie nicht als Last.

Im Lab (lab-np1 und lab-np2, `ether8`, Tags `ospf` und `dhcp`): Transfernetz
`192.168.101.40/31`, Management `192.168.101.48/29` an lab-np1 und `192.168.101.56/29` an
lab-np2. OSPFv2 und OSPFv3 laufen über die Strecke (`Full`). Beide Wave Nanos nahmen ihre Lease
zuerst von lab-np1 und sind von lab-np1, lab-np2 und lab-core aus erreichbar.

## Verworfen: Routen auf die Wave Nanos

Feste IPs der Wave Nanos in einem gemeinsamen /29, jeder MikroTik kündigt die Wave Nano seiner
Seite mit einer statischen `/32` an. Fällt weg: In der UISP-App lässt sich bei fester IP kein
Gateway setzen, und ohne ihren Gateway wäre eine Wave Nano bei ausgefallenem MikroTik nicht
mehr erreichbar. Ein Gateway je Seite ohne Route wäre asymmetrisch (`connection-state=invalid`
verwirft die Antworten), derselbe Gateway für beide trennt bei gestörter Strecke die Wave Nano
der anderen Seite vom Netz.
