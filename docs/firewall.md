# Firewall

Was der Generator an Firewall schreibt (`device/ros/firewall.rs`, `masquerade` in
`device/ros/mod.rs`). Port-Forwarding, die IPv6-Freigaben und die interne Firewall sind noch
Konzept: [concepts/firewall.md](concepts/firewall.md), Aufgaben C2 und C3 in [todo.md](todo.md).

## Eigene und fremde Regeln

In `/ip/firewall/filter`, `/ip/firewall/nat`, `/ipv6/firewall/filter` und
`/ipv6/firewall/address-list` gehören dem Provisioner nur die Einträge, deren Kommentar mit
`netbox-provisioner:` beginnt (Z1); jeder andere bleibt, wie er ist. Eine Regel ohne Kommentar,
die bis auf ihn einer eigenen gleicht, wird übernommen (die Regeln älterer Versionen hatten
keinen).

Die eigenen Regeln stehen **vor** den fremden, in der Reihenfolge des Ziels
(`move … destination=<erste fremde>`). Eine eigene Regel, deren Kommentar nach dem Präfix mit
`last ` weitergeht, steht dagegen **hinter** allen anderen (mikrotik-model `OWNED_LAST`, ein
zweites `move` ans Ende): das Verwerfen, mit dem eine Kette endet. So lässt eine von Hand gebaute
Regel weiter herein, was sie hereinlässt (bei router-faesigrund-keller die Webserver und das
IPsec der VPN), bis C2 das aus NetBox kann.

## Perimeter-Firewall

**Wo:** auf jedem Gerät mit einem Interface zum Internet, Tag `internet` (Z2). `nat` setzt
`internet` voraus; ein Interface mit `nat` ohne `internet` schützt der Generator trotzdem, die
Befunde melden es (`NatWithoutInternet`). Die Interfaces kommen in die Interface-Liste `uplink`
(`add_to_uplinks`), alle Regeln gelten nur für das, was dort hereinkommt (`in-interface-list=uplink`):
Das übrige Netz routet weiter ohne Regel. Ohne solches Interface schreibt der Generator keine Filter-Regel.

**Regeln**, für IPv4 und IPv6 gleich, in dieser Reihenfolge:

| Kette | Regel | Kommentar (nach dem Präfix) |
|---|---|---|
| `input` | `established,related` annehmen | `input established` |
| `input` | `invalid` verwerfen | `input invalid` |
| `input` | der Provisioner aus `api_networks` annehmen, je Netz seiner IP-Version (eine einzelne IPv4 ohne `/32`, wie RouterOS sie meldet) | `input provisioner <netz>` |
| `input` | was auf `ssh_blacklist` steht, verwerfen | `input ssh blacklisted` |
| `input` | ICMP annehmen (IPv6: ICMPv6, für ND und Path-MTU) | `input icmp`, `input icmpv6` |
| `input` | nur IPv6: UDP 546 von `fe80::/10`, die Antworten an den DHCPv6-Client | `input dhcpv6 client` |
| `input` | SSH-Stufen, siehe unten | `input ssh blacklist`, `… stage3`, `… stage2`, `… stage1` |
| `input` | SSH (TCP 22) annehmen | `input ssh` |
| `input` | den Rest verwerfen (API, Winbox, DNS, SOCKS …) | `last input drop` |
| `forward` | `established,related` annehmen | `forward established` |
| `forward` | `invalid` verwerfen | `forward invalid` |
| `forward` | nur IPv4: was ein Port-Forwarding hereinlässt (`connection-nat-state=dstnat`) | `forward dstnat` |
| `forward` | den Rest verwerfen | `last forward drop` |

Das frühere `close_dns` (DNS am `input` vom `nat`-Uplink verwerfen) geht im letzten
`input`-Drop auf, für IPv6 ebenso; ein Apply entfernt die alten Regeln, sie sind eigene.

**Bruteforce-Schutz für SSH** nach der
[MikroTik-Anleitung](https://manual.mikrotik.com/docs/firewall-and-quality-of-service/user-guides/bruteforce-prevention):
Eine neue SSH-Verbindung bringt ihre Quelle in `ssh_stage1`, eine weitere, solange sie dort steht,
nach `ssh_stage2`, dann `ssh_stage3`, dann `ssh_blacklist`. Die Regeln stehen von der letzten
Stufe her, so geht jede Verbindung nur eine Stufe weiter: Die vierte neue Verbindung kommt noch
durch und setzt die Quelle auf die Blacklist, die fünfte nicht mehr. Eine Stufe gilt eine Minute,
die Blacklist 10 Tage; der Config Context übersteuert beides (`ssh_stage_time`,
`ssh_blacklist_time`, Sekunden, siehe [netbox-modeling.md](netbox-modeling.md)). Die Einträge legt
RouterOS selbst an (dynamisch, ohne Kommentar); der Provisioner fasst sie nicht an.

**Der Zugang des Provisioners:** Die Netze aus `api_networks` (`config.yaml`) stehen vor den
SSH-Stufen, so zählen seine eigenen Logins und Uploads nie. Fehlen sie, erreicht er ein Gerät
vom Internet her nur noch per SSH, und ein Apply über den Uplink sperrt seine API aus; der
Fallback holt das Gerät zurück, aber ohne Änderung. Die Netze schreibt `generate_for`
(`allow_provisioner`), nach den übrigen Schritten.

**NAT** (nur IPv4): Ein Interface mit `nat` bekommt `srcnat action=masquerade out-interface=<Uplink>`.

**Geprüft** auf CHR 7.24.5 (`perimeter_firewall_keeps_the_internet_out_but_the_provisioner`,
`internet -- router -- client`): Das Apply geht über den SOCKS-Proxy von `internet` an die
Uplink-Adresse, also durch die Firewall, die es aufsetzt; eine neue Verbindung auf demselben Weg
kommt danach herein, und der Diff ist leer. Der Client erreicht das Internet über IPv4
(maskiert) und IPv6, das Internet erreicht ihn über IPv6 nicht; ein Fremder kommt nicht an die API
(IPv4 und IPv6) und nach vier SSH-Verbindungen auf die Blacklist, der Provisioner nie.
