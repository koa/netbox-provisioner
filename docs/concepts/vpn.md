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

- **Erreichbarkeit der Standorte** (vom Benutzer, 2026-11-25): Die meisten IPv4-Endpunkte haben
  dynamische Adressen, einige liegen hinter CGNAT. IPv6 gibt der Provider nur an einem Standort,
  dort statisch mit einem /48.
- **NetBox-Termination** (Lab, geprüft): `outside_ip` ist ein Verweis auf eine IP-Adresse in
  NetBox, `role` `peer`, `hub` oder `spoke`; für einen Hostnamen gibt es kein Feld.

## Was daraus folgt

- **Wer wen erreicht:** Ein Standort hinter CGNAT nimmt keine Verbindung an, er kann nur selbst
  aufbauen. Ein Standort mit dynamischer IPv4 ist erreichbar, aber nur über einen Namen, der
  nachgeführt wird (DDNS, etwa RouterOS' `/ip cloud`). Fest erreichbar ist nur der Standort mit
  IPv6, und nur für Standorte, die selbst IPv6 haben, also heute keinen. Ob seine IPv4 öffentlich
  ist, ist nicht bekannt.
- **Hub und Spokes statt Vollvermaschung:** Zwei Standorte hinter CGNAT verbinden sich nie
  direkt. Es braucht mindestens einen Hub, den alle erreichen, und der Verkehr zwischen Spokes
  läuft über ihn (bei BGP: der Hub gibt die Routen der Spokes weiter). Direkte Tunnel nur zwischen
  Standorten, von denen mindestens einer erreichbar ist.
- **WireGuard passt dazu besser als GRE und IPsec:** Wer aufbaut, braucht keine eigene feste
  Adresse; die Gegenseite lernt seine neue Adresse aus dem nächsten Paket. Der Aufbauende hält
  den Tunnel durch NAT mit `persistent-keepalive` offen. Das Skript, das heute die lokale Adresse
  von GRE und IPsec nachführt, entfällt so. Ob RouterOS einen Namen als `endpoint-address` bei
  einer Adressänderung neu auflöst, ist nicht geprüft; davon hängen Tunnel zu einem Hub mit
  dynamischer Adresse ab.
- **In NetBox:** Die Rolle der Termination sagt Hub oder Spoke. `outside_ip` passt nur für eine
  feste Adresse; für die übrigen braucht es ein Custom Field an der Termination, etwa
  `endpoint` (DDNS-Name, leer für „baut nur selbst auf“).

## Vorschlag

### 1. Bestandsaufnahme (lesend)

Eine Seite „VPNs“ im Provisioner, die von allen RouterOS-Geräten liest, was an Tunneln und BGP da
ist: OpenVPN-Server und -Clients, IPsec-Peers und Policies, GRE, WireGuard-Interfaces und -Peers,
BGP-Verbindungen mit ihrem Zustand. Daneben, was NetBox davon kennt. Das ist die Grundlage zum
Dokumentieren und zeigt danach Abweichungen. Jede Tabelle über einen mikrotik-model-Typ: BGP
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
- Welche Standorte hinter CGNAT sind, welche eine öffentliche dynamische IPv4 haben, und ob der
  Standort mit IPv6 auch eine öffentliche IPv4 hat.
