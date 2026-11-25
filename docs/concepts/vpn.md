# VPNs

Entwurf zu C22. Mehrere Netze, in NetBox unterschiedlich gut dokumentiert, sind über VPNs
verbunden (OpenVPN, IPsec), das Routing zwischen ihnen läuft über BGP. Mindestens eine
Verbindung führt in ein Netz, das nicht in NetBox steht. Ziel: die VPNs in NetBox dokumentieren
und sie, wo möglich, auf ein schlankeres VPN wie WireGuard umstellen.

## Was bekannt ist

- **router-faesigrund-keller** (Export in `examples/02-edge-router-bgp-vpn`, Stand des Exports):
  ein OpenVPN-Client (`ovpn-colab-sihlquai`), drei OpenVPN-Server (`ovpn-styghalde`, `ovpn-tk`,
  `ovpn-wegmann`), GRE-Tunnel (`gre-colab`, `gre-tunnel-selnau`, `gre-backup-link`, `gre-lkw`
  aus), IPsec-Peers und -Policies, ein Skript, das die lokale Adresse von GRE und IPsec nach
  einer neuen öffentlichen IPv4 nachführt, drei BGP-Instanzen mit drei ASNs (65531, 65537,
  4200000000) und Filterketten pro Peer.
- **klr-sw-3** (Export in `examples/03-edge-router-bgp-cilium`): iBGP zu drei Kubernetes-Knoten,
  kein VPN.
- **Heute im Provisioner:** Tunnel-Interfaces und ihre Adressen lässt er stehen
  (`interface_live::hand_made_interfaces`, `keep_hand_made_addresses`), BGP ebenso, und OSPF
  verteilt die BGP-Routen weiter (`ospf_redistribute`). Ob ein Apply etwas davon berührt, prüft
  der Diff; Modell und Generator kennen keine VPNs.
- **Entschieden am 2026-10-02:** BGP bleibt von Hand, ohne NetBox-Modell, weil es nur zwei
  Router betrifft. Mit den VPNs stellt sich die Frage neu (Z61).
- **NetBox** (Lab, 4.7.3, geprüft): eigenes VPN-Modell mit `Tunnel` (Encapsulation `wireguard`,
  `openvpn`, `ipsec-tunnel`, `ipsec-transport`, `gre`, `ip-ip`, `l2tp`, `pptp`; Status, Gruppe,
  Tenant, Owner, IPsec-Profil) und Tunnel-Terminations an einem Interface eines Geräts oder einer
  VM, mit Rolle (Peer, Hub, Spoke) und äusserer IP.

- **Anschlüsse der Standorte** (vom Benutzer, 2026-11-25; in NetBox noch nicht vollständig, im
  Export also nicht ganz zu sehen):

  | Anzahl | IPv4 | IPv6 |
  |---|---|---|
  | 2 | öffentlich, dynamisch | statisches /48 |
  | 2 | öffentlich, dynamisch | keins |
  | 3 | Mobilfunk hinter CGNAT | keins |

  Router und Anschlüsse unterscheiden sich stark: von einem CCR2004 an 10 Gbit/s symmetrisch bis
  zu einem Chateau an 50 Mbit/s LTE.

- **NetBox-Termination** (Lab, geprüft): `outside_ip` ist ein Verweis auf eine IP-Adresse in
  NetBox, `role` `peer`, `hub` oder `spoke`; für einen Hostnamen gibt es kein Feld.

## Was daraus folgt

- **Wer wen erreicht:**

  | von \ zu | IPv6-Standort | IPv4-Standort | Mobilfunk |
  |---|---|---|---|
  | IPv6-Standort | IPv6, feste Adresse | IPv4 über DDNS | nie |
  | IPv4-Standort | IPv4 über DDNS | IPv4 über DDNS | nie |
  | Mobilfunk | IPv4 über DDNS | IPv4 über DDNS | nie |

  Ein Mobilfunk-Standort nimmt keine Verbindung an, er baut nur selbst auf. Jede IPv4 ist
  dynamisch, also nur über einen nachgeführten Namen erreichbar (DDNS, etwa RouterOS'
  `/ip cloud`). Fest ist nur IPv6, und das nur zwischen den beiden IPv6-Standorten.
- **Zwei Hubs:** die beiden IPv6-Standorte. Untereinander ein Tunnel über IPv6, ohne DDNS. Jeder
  andere Standort baut zu beiden einen Tunnel auf, über ihre IPv4; fällt ein Hub aus, läuft alles
  über den anderen. Der Verkehr zwischen zwei Spokes geht über einen Hub (bei BGP: der Hub gibt
  die Routen der Spokes weiter). Ein Server ausserhalb (VPS) ist damit nicht nötig.
- **Leistung zählt beim Hub:** Ein Hub trägt den Verkehr zwischen den Spokes doppelt (hinein und
  hinaus), dazu die Verschlüsselung aller Tunnel. Hubs gehören also an die stärksten Anschlüsse
  mit den stärksten Routern. Sind das nicht die beiden IPv6-Standorte, ändert sich die Wahl: ein
  starker IPv4-Standort kann Hub sein (über DDNS erreichbar wie die anderen), und ein schwacher
  IPv6-Standort ist dann nur Spoke. Was ein Router an WireGuard schafft, ist nicht geprüft
  (MikroTiks Testergebnisse pro Modell, oder ein Messen im Lab).
- **Bevorzugter Weg nach Leistung:** Bei zwei Hubs soll der Verkehr über den stärkeren laufen und
  den schwächeren nur bei Ausfall nehmen; mit BGP über Local Preference oder AS-Path pro Tunnel,
  mit OSPF über die Kosten. Wo zwei starke Spokes viel miteinander austauschen und einer
  erreichbar ist, lohnt ein direkter Tunnel daneben.
- **Kleine MTU auf Mobilfunk:** LTE hat oft weniger als 1500 Byte; die Tunnel-MTU richtet sich
  nach dem schwächsten Glied (WireGuard braucht 60 Byte über IPv4, 80 über IPv6). Nicht geprüft
  für die drei Anschlüsse.
- **In NetBox die Leistung:** Der Internetanschluss eines Standorts als Circuit mit
  `commit_rate` (Lab geprüft: das Feld gibt es), daraus Hub-Wahl und Gewichtung; das Board steht
  schon am Gerät.
- **Die Spokes hängen an DDNS:** Alle Tunnel zu den Hubs laufen über deren dynamische IPv4. Ob
  RouterOS einen Namen als `endpoint-address` eines WireGuard-Peers neu auflöst, wenn sich die
  Adresse dahinter ändert, ist nicht geprüft; das ist die wichtigste Frage des Entwurfs. Prüfen
  lässt es sich auf CHRs mit einem statischen DNS-Eintrag, der umgestellt wird. Löst RouterOS
  nicht neu auf, braucht es ein Skript, das den Peer nachführt (wie heute für GRE und IPsec).
- **WireGuard passt dazu besser als GRE und IPsec:** Wer aufbaut, braucht keine eigene feste
  Adresse; die Gegenseite lernt seine neue Adresse aus dem nächsten Paket. Der Aufbauende hält
  den Tunnel durch NAT mit `persistent-keepalive` offen. Das Skript, das heute die lokale Adresse
  von GRE und IPsec nachführt, entfällt so.
- **In NetBox:** Die Rolle der Termination sagt Hub oder Spoke. `outside_ip` passt nur für die
  festen IPv6-Adressen der Hubs; für die IPv4 braucht es ein Custom Field an der Termination,
  etwa `endpoint` (DDNS-Name, leer für „baut nur selbst auf“, die Mobilfunk-Standorte).

## WireGuard oder IPsec

Nicht gemessen (kein Lab-Gerät dafür); aus dem Wissen über RouterOS, im Einzelnen zu prüfen:

- **Leistung:** Viele MikroTik-Router verschlüsseln IPsec (AES) in Hardware, WireGuard läuft in
  RouterOS nur auf der CPU. Bei hohen Raten auf einem Router mit IPsec-Beschleunigung (etwa dem
  CCR2004) ist IPsec deshalb wahrscheinlich schneller; MikroTik gibt pro Modell einen
  IPsec-Durchsatz an, für WireGuard keinen. An einem 50-Mbit/s-LTE-Anschluss begrenzt der
  Anschluss, nicht die Verschlüsselung.
- **Routing:** RouterOS' IPsec arbeitet mit Policies, nicht mit einem Interface. Für BGP oder OSPF
  braucht es ein Interface, also GRE oder IPIP mit IPsec im Transport-Modus darunter (wie heute).
  WireGuard ist selbst ein Interface.
- **Dynamische Adressen und NAT:** GRE und IPIP brauchen beide Adressen fest eingetragen; ändert
  sich eine, muss ein Skript nachführen (wie heute). Hinter CGNAT kommt GRE nur in ESP über UDP
  (NAT-T) verpackt durch, und die Adressen darin sind die privaten. WireGuard lernt die Adresse
  der Gegenseite aus den Paketen.

IPsec passt also dort, wo **beide Enden feste Adressen** haben und viel Verkehr fliesst: zwischen
den beiden Hubs über ihr statisches IPv6, GRE über IPv6 mit IPsec (in RouterOS `ipsec-secret`
am GRE-Interface), ohne NAT und ohne Skript, wenn beide Router IPsec in Hardware können. Für die
Spokes (dynamische IPv4, CGNAT) bleibt WireGuard: Ihre Anschlüsse sind langsamer als die
Verschlüsselung, und sie brauchen kein Nachführen.

Messen ohne Lab: auf den zwei Hubs, sobald beide Tunnel stehen, mit `/tool bandwidth-test` über
jeden und der CPU-Last daneben (`/tool profile`); das macht der Betreiber, nicht der Provisioner.

## Vorschlag

### 1. Bestandsaufnahme (lesend)

Eine Seite „VPNs“ im Provisioner, die von allen RouterOS-Geräten liest, was an Tunneln und BGP da
ist: OpenVPN-Server und -Clients, IPsec-Peers und Policies, GRE, WireGuard-Interfaces und -Peers,
BGP-Verbindungen mit ihrem Zustand. Daneben, was NetBox davon kennt. Das ist die Grundlage zum
Dokumentieren und zeigt danach Abweichungen. Sie sieht nur Geräte, die in NetBox stehen und die
der Provisioner erreicht; was dort fehlt, muss zuerst in NetBox. Jede Tabelle über einen mikrotik-model-Typ: BGP
(`/routing/bgp/instance`, `connection`) ist modelliert, OpenVPN, IPsec, GRE und WireGuard
noch nicht.

### 2. In NetBox dokumentieren

- Ein `Tunnel` pro VPN-Verbindung, mit Encapsulation und Status; eine Tunnelgruppe pro Netz, das
  angebunden ist (z.B. „colab“, „Familie“).
- Je Ende eine Termination am Tunnel-Interface des Geräts (ein virtuelles Interface, Name wie auf
  dem Gerät), mit Rolle (Hub, Spoke) und, wo fest, der äusseren IP, sonst dem Custom Field
  `endpoint` (siehe oben); das Netz im Tunnel als Prefix mit den Adressen der Enden.
- Die Gegenstelle in einem Netz ausserhalb NetBox: siehe Z60.

### 3. WireGuard erzeugen

Erst wenn die Dokumentation steht, und Tunnel für Tunnel:

- Der Provisioner erzeugt das WireGuard-Interface und die Peers aus dem Tunnel in NetBox.
- **Schlüssel ohne Ablage:** Der private Schlüssel eines Endes wird aus einem Geheimnis der
  Konfiguration und Gerät und Tunnel abgeleitet, wie die OSPF-Schlüssel (`ospf_auth`); der
  öffentliche folgt daraus. NetBox hält keinen Schlüssel. Für eine Gegenstelle, die der
  Provisioner nicht konfiguriert, zeigt er den öffentlichen Schlüssel und die Peer-Angaben an.
- **Umstellen:** WireGuard neben dem alten Tunnel aufbauen, BGP über beide, dann den alten
  abschalten; so fällt die Verbindung nie aus. Die alten Tunnel bleiben bis dahin von Hand.
- BGP über den Tunnel je nach Z61: von Hand oder aus NetBox.

### Was damit wegfällt

E26 (Zertifikat für einen OpenVPN-Server) und Z41 braucht es nur, wo OpenVPN bleibt, etwa für
Clients, die kein WireGuard können.

## Offen

- Z59 bis Z63 in [todo.md](../todo.md#entscheidungen).
- Welche Gegenstellen RouterOS sind, die der Provisioner erreicht, und welche nicht (Server,
  fremde Router).
- Ob RouterOS den DDNS-Namen eines WireGuard-Peers neu auflöst (CHR-Test, siehe oben).
- Welche Standorte die starken sind (Router, Anschluss), insbesondere die beiden mit IPv6.
