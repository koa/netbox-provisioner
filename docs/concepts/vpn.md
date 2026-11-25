# VPNs

Entwurf zu C22. Mehrere Netze, in NetBox unterschiedlich gut dokumentiert, sind über VPNs
verbunden (OpenVPN, IPsec), das Routing zwischen ihnen läuft über BGP. Mindestens eine
Verbindung führt in ein Netz, das nicht in NetBox steht. Ziel: die VPNs in NetBox dokumentieren
und sie, wo möglich, auf ein schlankeres VPN wie WireGuard umstellen.

## Entschieden

- **Zwei Arten von Verbindungen** (2026-11-25): zwischen den Hubs IPv6 / IPsec / GRE, von jedem
  Spoke zu jedem Hub IPv4 / WireGuard.
- **Hubs** (Z63, 2026-11-25): die beiden Standorte mit statischem IPv6-/48 und 10 Gbit/s
  (CCR2004, RB5009).
- **NetBox** (Z59, 2026-11-25): das eigene VPN-Modell, ein Tunnel pro Paar von Enden, wie unter
  „In NetBox dokumentieren“.

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

  Router und Anschlüsse: Die beiden Standorte mit IPv6 haben 10 Gbit/s symmetrisch, an einem
  CCR2004 und einem RB5009. Die beiden übrigen mit öffentlicher IPv4 haben 1 Gbit/s, an einem
  RB1100AHx4 und einem RB760iGS (hEX S); die drei Mobilfunk-Standorte einen Chateau an LTE (bis
  etwa 50 Mbit/s).

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
  mit den stärksten Routern. Das sind die beiden IPv6-Standorte (CCR2004, RB5009, 10 Gbit/s):
  Leistung und feste Adressen fallen zusammen. Was ein Router an WireGuard schafft, ist nicht
  geprüft. Am ehesten bremst der RB760iGS (MIPS, zwei Kerne): An seinem 1-Gbit/s-Anschluss
  begrenzt wohl die CPU den WireGuard-Tunnel, nicht der Anschluss (nicht gemessen; der lab-hex,
  ein RB750Gr3, hat nach meinem Wissen dieselbe CPU).
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
den beiden Hubs über ihr statisches IPv6, GRE über IPv6 mit IPsec (`/interface gre6`), ohne
NAT und ohne Skript. CCR2004 und RB5009 können IPsec nach meinem Wissen in Hardware (nicht
geprüft); 10 Gbit/s schafft wohl keiner der beiden verschlüsselt. Für die
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

Zwei Arten von Verbindungen, beide mit NetBox' eigenem VPN-Modell (Z59). Geprüft in der
Lab-NetBox (4.7.3): die Felder und Auswahlwerte unten, Custom Fields an Tunnel-Terminations;
Interfaces haben nur den Typ `virtual` für Tunnel. Nicht geprüft: ob NetBox `gre` zusammen mit
einem IPsec-Profil annimmt.

Für beide gilt:

- **Ein `Tunnel` pro Paar von Enden**, mit zwei Terminations. Ein Hub mit allen Spokes in einem
  Tunnel ginge in NetBox, aber WireGuard ordnet Pakete nach `allowed-address` den Peers zu: An
  einem gemeinsamen Interface müsste der Hub die Netze jedes Spokes als Liste kennen, die BGP
  gerade erst lernt. Ein Interface pro Paar nimmt `0.0.0.0/0` und `::/0`, das Routing entscheidet.
- **Termination** am Tunnel-Interface jedes Endes: ein NetBox-Interface vom Typ `virtual`, mit dem
  Namen, den es auf dem Gerät bekommen soll, der MTU (`mtu`) und den inneren Adressen.
- **Innere Adressen:** pro Tunnel ein `/31` aus dem Transfer-Pool und, wo IPv6 gebraucht wird, ein
  `/127` oder nur Link-Local, wie bei „Verbindung“.
- **Schlüssel nicht in NetBox:** IPsec-PSK und WireGuard-Schlüssel leitet der Provisioner aus
  einem Geheimnis der Konfiguration und dem Tunnel ab, wie bei `ospf_auth`. Das Feld
  `preshared_key` der IKE-Policy bleibt leer.
- **Tunnelgruppe** nach Zweck, etwa „Hubs“ und „Spokes“, oder pro angebundenem Netz.

**IPv6 / IPsec / GRE (zwischen den Hubs):**

| NetBox | Wert |
|---|---|
| Tunnel `encapsulation` | `gre` |
| Tunnel `ipsec_profile` | ein Profil „hub-gre“: `mode` `esp`, IKE-Policy (`version` 2, Proposal z.B. `aes-256-gcm`, `preshared-keys`, Gruppe 19 oder 20), IPsec-Policy (Proposal `aes-256-gcm`, PFS-Gruppe) |
| Termination `role` | `peer` an beiden Enden |
| Termination `outside_ip` | die statische IPv6-Adresse des Hubs, in NetBox an seinem Uplink oder Loopback |

Auf dem Gerät: `/interface gre6` (GRE über IPv6) mit lokaler und entfernter Adresse aus den
beiden `outside_ip`. IPsec dazu entweder mit `ipsec-secret` am GRE-Interface (bei `gre6` nicht
geprüft) (RouterOS legt Peer und Policy selbst
an, das NetBox-Profil wäre dann nur Beschreibung) oder ausdrücklich als Peer, Identity, Profil,
Proposal und Policy im Transport-Modus aus dem NetBox-Profil. Vorschlag: ausdrücklich, damit das
Profil in NetBox gilt; welche Proposals die Hubs in Hardware rechnen, ist zu prüfen.

**IPv4 / WireGuard (Spoke zu Hub):**

| NetBox | Wert |
|---|---|
| Tunnel `encapsulation` | `wireguard` |
| Tunnel `ipsec_profile` | leer |
| Tunnel `tunnel_id` | der Listen-Port am Hub (jedes Interface braucht einen eigenen) |
| Termination `role` | `hub` am Hub, `spoke` am Spoke |
| Termination `outside_ip` | leer: die IPv4 ist dynamisch |
| Termination Custom Field `endpoint` | am Hub sein DDNS-Name; am Spoke leer (baut selbst auf) oder sein DDNS-Name, wo er erreichbar ist |

Auf dem Gerät: `/interface wireguard` mit `listen-port`, ein Peer mit dem öffentlichen
Schlüssel der Gegenseite, `allowed-address=0.0.0.0/0,::/0`, am Spoke `endpoint-address` und
`endpoint-port` aus der Termination des Hubs und `persistent-keepalive` (die Rolle `spoke` sagt,
wer aufbaut und das NAT offen hält).

Neue Custom Fields damit nur eins: `endpoint` (Text) an der Tunnel-Termination. Die Gegenstelle
in einem Netz ausserhalb NetBox: Z60.

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

- Z60 bis Z62 in [todo.md](../todo.md#entscheidungen).
- Welche Gegenstellen RouterOS sind, die der Provisioner erreicht, und welche nicht (Server,
  fremde Router).
- Ob RouterOS den DDNS-Namen eines WireGuard-Peers neu auflöst (CHR-Test, siehe oben).
- Ob NetBox `gre` mit einem IPsec-Profil annimmt, ob `gre6` `ipsec-secret` kann, und welche
  Proposals CCR2004 und RB5009 in Hardware rechnen.
