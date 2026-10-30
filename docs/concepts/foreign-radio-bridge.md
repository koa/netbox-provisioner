# Konzept: Funkbrücke ohne RouterOS

Status: Konzept; im Lab aufgebaut ist [Variante 2, DHCP](#variante-2-dhcp-je-seite)
(2026-10-27), und der Helfer „Funkbrücke“ trägt sie in NetBox ein
([config-helper.md](../config-helper.md#brückengeräte-ohne-routeros)). Offen sind die Ausfall-Tests. [Variante 1](#variante-1-routen-auf-die-wave-nanos) fällt weg: In der UISP-App
lässt sich bei fester IP kein Gateway setzen (ob die Weboberfläche es kann, ist ungeprüft).
Aufgabe C16 in [todo.md](../todo.md).

Eine Funkstrecke aus zwei Geräten ohne RouterOS (geplant: zwei UISP Wave Nano, 60 GHz) zwischen
zwei MikroTiks, L2-transparent, ohne VLANs. Der Provisioner konfiguriert die Funkgeräte nicht,
sorgt aber dafür, dass ihre Management-IPs von beiden Seiten erreichbar bleiben, auch wenn die
Strecke ausfällt.

## Die Funkgeräte

Die Wave Nanos werden von Hand eingerichtet (Weboberfläche oder Bluetooth-App, keine Cloud):

- PtP ab Wave-Firmware 3.2.0: eine Seite PtP-AP, die andere Station.
- Bridge-Modus; der Router-Modus macht aus der Strecke einen NAT-Router ohne dynamisches Routing.
- Variante 1: eine feste IPv4 im Netz der Strecke, als Gateway der MikroTik **der eigenen
  Seite**. Variante 2: DHCP-Client.

Nicht möglich (Datenblatt, Stand 2026-10-27): OSPF oder sonst ein Routing-Protokoll, eine
dokumentierte lokale API, SNMP mit Schreibzugriff. SNMP liest nur, taugt also fürs Monitoring
(Funkverbindung, Signal, Kapazität), nicht zum Konfigurieren.

## Variante 1: Routen auf die Wave Nanos

### In NetBox

- Ein IPv4-/29 für die Strecke, wie ein Router-Port-Netz: die beiden MikroTik-Ports (Tag `ospf`)
  und die beiden Wave Nanos.
- Die Wave Nanos als Geräte ohne Platform `routeros`, je mit Ethernet- und Funk-Interface, ihre
  IP als Primary IP, ein Kabel zum MikroTik-Port, ein Wireless Link zwischen den Funk-Interfaces.
- IPv6 auf den MikroTik-Ports bleibt Link-Local (OSPFv3). IPv6 für die Wave Nanos nur mit einem
  eigenen /64 auf der Strecke; ob die Firmware IPv6-Management kann, ist offen.

### Routing

Jeder MikroTik kündigt die Wave Nano seiner Seite an: eine statische Route auf ihre IP (`/32`,
bei IPv6 `/128`) über den Port, an dem sie hängt. OSPF verteilt sie schon heute
(`redistribute=connected,static,bgp`). Eine `/32` ist spezifischer als das gemeinsame `/29`, so
erreicht jeder, auch der MikroTik der anderen Seite, eine Wave Nano über ihren eigenen MikroTik.

Damit sind Hin- und Rückweg gleich, und eine gestörte Strecke trennt keine Wave Nano von ihrer
Seite: Von B aus bleibt Wave B erreichbar, von A aus Wave A, und mit einem zweiten Weg zwischen
A und B beide von überall.

Verworfen:

- **Ein Gateway je Seite, ohne Route:** asymmetrisch. Der MikroTik der anderen Seite stellt
  direkt ins `/29` zu, die Antwort geht über den Gateway der Wave Nano zurück. Eine Firewall mit
  `connection-state=invalid → drop` würde sie dort verwerfen.
- **Derselbe Gateway für beide:** symmetrisch, aber bei gestörter Strecke ist die Wave Nano der
  anderen Seite auch von ihrem eigenen MikroTik aus nur noch mit dessen Adresse im `/29` als
  Absender erreichbar, nicht mehr aus dem Netz.

Fällt ein MikroTik aus, ist die Wave Nano seiner Seite nicht mehr erreichbar: Ihr fester
Gateway fehlt.

### Generator

Die Route kommt ganz aus NetBox, ohne neue Felder: Ein Gerät ohne `routeros` mit einer IP in
einem Netz eines Router-Ports, per Kabel an einem Port eines MikroTiks, gibt diesem MikroTik eine
statische Route auf diese IP über diesen Port. Dazu kommt `/ip/route` (und `/ipv6/route`) ins
Ziel, wie die Firewall nur mit den eigenen Einträgen (Kommentar-Präfix `netbox-provisioner:`),
damit Routen von Hand bleiben.

### Im Lab zu prüfen

- Folgt die Wave-Firmware einem ICMP-Redirect? Für eine Verbindung von A zu Wave B schickt
  MikroTik B die Antwort über denselben Port zurück, über den sie von Wave B kam, und RouterOS
  schickt dann standardmässig ein Redirect (`/ip settings send-redirects`), Wave B solle A direkt
  nehmen. Folgt sie dem, ist der Rückweg wieder asymmetrisch; dann Redirects abschalten,
  pro Interface, falls RouterOS das kann, sonst auf dem Router.
- Was die Generierung heute zu fremden Geräten mit IP in einem Router-Port-Netz und zu einem
  Kabelpfad über eine fremde Funkstrecke meldet.
- Die OSPF-Nachbarschaft über die Strecke, und dass die `/32` bei einer gestörten Strecke über
  einen anderen Weg erreichbar bleibt.

## Variante 2: DHCP je Seite

Jeder MikroTik hat auf dem Port zur Strecke ein eigenes /29 mit DHCP-Server und kurzer Lease
(1–2 min); jede Wave Nano nimmt das Angebot, das zuerst kommt. Jedes /29 kündigt nur sein
MikroTik an, so sind Hin- und Rückweg gleich, ohne zusätzliche Routen und ohne die Frage der
ICMP-Redirects.

Fällt eine Seite weg (MikroTik aus oder Strecke gestört), verlängert die Wave Nano ihre Adresse
nicht mehr: Ab etwa 87,5 % der Lease fragt sie jeden Server, der verbliebene lehnt die fremde
Adresse ab (RouterOS ist standardmässig `authoritative`), und sie holt sich eine aus seinem Netz.
Bei gestörter Strecke landet so jede Wave Nano bei ihrem lokalen MikroTik, bei einem
ausgefallenen MikroTik beide beim anderen. Bis dahin vergeht höchstens eine Lease.

Kosten:

- Die IPs wechseln; wo welche Wave Nano ist, steht in den Leases der beiden MikroTiks, nicht in
  NetBox. Der Provisioner könnte sie an der Strecke anzeigen.
- OSPFv2 bildet auf einem Broadcast-Segment nur Nachbarschaften im selben Subnetz: Die beiden
  Ports brauchen zusätzlich ein gemeinsames Transfernetz, ein /31; auf dem Segment liegen dann
  drei Netze. OSPFv3 über Link-Local betrifft das nicht.
- Je ein DHCP-Server auf beiden MikroTiks im selben Segment. Pro Port ist es einer: Das /31
  lässt keine Adresse frei und bekommt nichts, der Server gehört dem /29.

Aufbau im Lab (lab-np1 und lab-np2, `ether8`, Tags `ospf` und `dhcp`): Transfernetz
`192.168.101.40/31`, Management `192.168.101.48/29` an lab-np1 und `192.168.101.56/29` an
lab-np2. Dafür nötig waren passives PoE (`forced-on`) und ein DHCP-Server pro Port für alle
seine Netze; mehr Code nicht. OSPFv2 und OSPFv3 laufen über die Strecke (`Full`). Beide Wave
Nanos nahmen ihre Lease zuerst von lab-np1 und sind von lab-np1, lab-np2 und lab-core aus
erreichbar.

Die kurze Lease ist das Custom Field `dhcp_lease_time` am Prefix; der Helfer setzt 120 s.

### Im Lab zu prüfen

- Ob die beiden DHCP-Server sich nicht stören, besonders nach einem Neustart einer Wave Nano
  (sie fragt dann ohne Server-ID nach ihrer alten Adresse).
- Wie lange der Wechsel tatsächlich dauert.

## Vergleich im Lab

Beide Varianten mit denselben Fällen: Strecke gestört, ein MikroTik aus, Neustart einer Wave
Nano. Entscheiden nach der Zeit und Zuverlässigkeit des Wechsels und danach, wie lästig die
wechselnden IPs von Variante 2 im Alltag sind.
