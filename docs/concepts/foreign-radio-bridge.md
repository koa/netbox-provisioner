# Konzept: Funkbrücke ohne RouterOS

Status: Konzept, warten auf die Wave Nanos im Lab. Aufgabe C16 in [todo.md](../todo.md).

Eine Funkstrecke aus zwei Geräten ohne RouterOS (geplant: zwei UISP Wave Nano, 60 GHz) zwischen
zwei MikroTiks, L2-transparent, ohne VLANs. Der Provisioner konfiguriert die Funkgeräte nicht,
sorgt aber dafür, dass ihre Management-IPs von beiden Seiten erreichbar bleiben, auch wenn die
Strecke ausfällt.

## Die Funkgeräte

Die Wave Nanos werden von Hand eingerichtet (Weboberfläche oder Bluetooth-App, keine Cloud):

- PtP ab Wave-Firmware 3.2.0: eine Seite PtP-AP, die andere Station.
- Bridge-Modus; der Router-Modus macht aus der Strecke einen NAT-Router ohne dynamisches Routing.
- Eine feste IPv4 im Netz der Strecke, als Gateway der MikroTik **der eigenen Seite**.

Nicht möglich (Datenblatt, Stand 2026-10-27): OSPF oder sonst ein Routing-Protokoll, eine
dokumentierte lokale API, SNMP mit Schreibzugriff. SNMP liest nur, taugt also fürs Monitoring
(Funkverbindung, Signal, Kapazität), nicht zum Konfigurieren.

## In NetBox

- Ein IPv4-/29 für die Strecke, wie ein Router-Port-Netz: die beiden MikroTik-Ports (Tag `ospf`)
  und die beiden Wave Nanos.
- Die Wave Nanos als Geräte ohne Platform `routeros`, je mit Ethernet- und Funk-Interface, ihre
  IP als Primary IP, ein Kabel zum MikroTik-Port, ein Wireless Link zwischen den Funk-Interfaces.
- IPv6 auf den MikroTik-Ports bleibt Link-Local (OSPFv3). IPv6 für die Wave Nanos nur mit einem
  eigenen /64 auf der Strecke; ob die Firmware IPv6-Management kann, ist offen.

## Routing

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

## Generator

Die Route kommt ganz aus NetBox, ohne neue Felder: Ein Gerät ohne `routeros` mit einer IP in
einem Netz eines Router-Ports, per Kabel an einem Port eines MikroTiks, gibt diesem MikroTik eine
statische Route auf diese IP über diesen Port. Dazu kommt `/ip/route` (und `/ipv6/route`) ins
Ziel, wie die Firewall nur mit den eigenen Einträgen (Kommentar-Präfix `netbox-provisioner:`),
damit Routen von Hand bleiben.

## Im Lab zu prüfen

- Folgt die Wave-Firmware einem ICMP-Redirect? Für eine Verbindung von A zu Wave B schickt
  MikroTik B die Antwort über denselben Port zurück, über den sie von Wave B kam, und RouterOS
  schickt dann standardmässig ein Redirect (`/ip settings send-redirects`), Wave B solle A direkt
  nehmen. Folgt sie dem, ist der Rückweg wieder asymmetrisch; dann Redirects abschalten,
  pro Interface, falls RouterOS das kann, sonst auf dem Router.
- Was die Generierung heute zu fremden Geräten mit IP in einem Router-Port-Netz und zu einem
  Kabelpfad über eine fremde Funkstrecke meldet.
- Die OSPF-Nachbarschaft über die Strecke, und dass die `/32` bei einer gestörten Strecke über
  einen anderen Weg erreichbar bleibt.
