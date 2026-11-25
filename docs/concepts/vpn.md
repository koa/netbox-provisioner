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
  dem Gerät), mit der äusseren IP; das Netz im Tunnel als Prefix mit den Adressen der Enden.
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

- Z59, Z60, Z61, Z62 in [todo.md](../todo.md#entscheidungen).
- Welche Gegenstellen RouterOS sind, die der Provisioner erreicht, und welche nicht (Server,
  fremde Router).
- Ob das Skript zur Adresse von GRE und IPsec mit WireGuard entfällt (WireGuard braucht die
  eigene äussere Adresse nicht).
