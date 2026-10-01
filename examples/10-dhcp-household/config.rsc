# 2026-10-01 15:32:03 by RouterOS 7.20.4
# software id = D5KL-6MHI
#
# model = CRS326-24G-2S+
# serial number = HCS08456S7N
/interface bridge
add name=switch protocol-mode=mstp vlan-filtering=yes
/interface ethernet
set [ find default-name=ether1 ] name=e01-2.01-1og-wohnz-balkon
set [ find default-name=ether2 ] name=e02-2.02-1og-wohnz-ecke
set [ find default-name=ether3 ] name=e03-2.03-1og-wohnz-buero
set [ find default-name=ether4 ] name=e04-2.04-1og-gr-zimmer
set [ find default-name=ether5 ] name=e05-2.05-1og-kl-zimmer
set [ find default-name=ether6 ] name=e06
set [ find default-name=ether7 ] name=e07-2.07-2og-louise-fenster-l
set [ find default-name=ether8 ] name=e08-2.08-2og-musik-fenster
set [ find default-name=ether9 ] name=e09-2.09-3og-vorne
set [ find default-name=ether10 ] name=e10-2.10-2og-louise-bett
set [ find default-name=ether11 ] name=e11-2.11-2og-musik-pfosten
set [ find default-name=ether12 ] name=e12-2.12-3og-hinten
set [ find default-name=ether13 ] name=e13-2.12-2og-musik-bett
set [ find default-name=ether14 ] name=e14-2.14-2og-louise-fenster-r
set [ find default-name=ether15 ] name=e15-2.15-2og-louise-fenster
set [ find default-name=ether16 ] name=e16-2.16-2og-musik-lukarne
set [ find default-name=ether17 ] name=e17-wlan
set [ find default-name=ether18 ] name=e18-loxone
set [ find default-name=ether19 ] name=e19
set [ find default-name=ether20 ] name=e20
set [ find default-name=ether21 ] name=e21
set [ find default-name=ether22 ] name=e22
set [ find default-name=ether23 ] name=e23
set [ find default-name=ether24 ] name=e24-uplink
set [ find default-name=sfp-sfpplus1 ] name=s01
set [ find default-name=sfp-sfpplus2 ] name=s02
/interface vlan
add interface=switch name=vlan1-lan vlan-id=1
/interface wireless security-profiles
set [ find default=yes ] supplicant-identity=MikroTik
/ip pool
add name=dhcp_pool0 ranges=10.192.9.2-10.192.9.254
/ip dhcp-server
add address-pool=dhcp_pool0 interface=vlan1-lan lease-time=10m name=dhcp-lan
/port
set 0 name=serial0
/routing bgp instance
add as=4200000015 name=bgp-instance-1 vrf=main
/routing bgp template
set default as=4200000015 disabled=no output.network=bgp-networks \
    .redistribute=connected,bgp routing-table=main use-bfd=yes
/routing ospf instance
add disabled=no name=default-v2 redistribute=connected
add disabled=no name=default-v3 redistribute=connected version=3
/routing ospf area
add disabled=no instance=default-v2 name=backbone-v2
add disabled=no instance=default-v3 name=backbone-v3
/routing pimsm instance
add disabled=yes name=pimsm-instance1 vrf=main
/system logging action
set 3 cef-event-delimiter="" remote=2a02:16a:fe00:1a0::10:8 \
    remote-log-format=cef syslog-time-format=iso8601
/user group
add name=prometheus policy="read,api,!local,!telnet,!ssh,!ftp,!reboot,!write,!\
    policy,!test,!winbox,!password,!web,!sniff,!sensitive,!romon,!rest-api"
/interface bridge port
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e01-2.01-1og-wohnz-balkon
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e02-2.02-1og-wohnz-ecke
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e03-2.03-1og-wohnz-buero
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e04-2.04-1og-gr-zimmer
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e05-2.05-1og-kl-zimmer
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e06
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e07-2.07-2og-louise-fenster-l
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e08-2.08-2og-musik-fenster
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e09-2.09-3og-vorne
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e10-2.10-2og-louise-bett
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e11-2.11-2og-musik-pfosten
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e12-2.12-3og-hinten
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e13-2.12-2og-musik-bett
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e14-2.14-2og-louise-fenster-r
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e15-2.15-2og-louise-fenster
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e16-2.16-2og-musik-lukarne
add bridge=switch disabled=yes frame-types=\
    admit-only-untagged-and-priority-tagged interface=e17-wlan
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e18-loxone
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e19
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e20
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e21
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e22
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e23
/interface ethernet switch l3hw-settings
set ipv6-hw=yes
/interface bridge vlan
# e17-wlan not a bridge port
add bridge=switch tagged=switch untagged="e01-2.01-1og-wohnz-balkon,e02-2.02-1\
    og-wohnz-ecke,e03-2.03-1og-wohnz-buero,e04-2.04-1og-gr-zimmer,e05-2.05-1og\
    -kl-zimmer,e06,e07-2.07-2og-louise-fenster-l,e08-2.08-2og-musik-fenster,e0\
    9-2.09-3og-vorne,e10-2.10-2og-louise-bett,e11-2.11-2og-musik-pfosten,e12-2\
    .12-3og-hinten,e13-2.12-2og-musik-bett,e14-2.14-2og-louise-fenster-r,e15-2\
    .15-2og-louise-fenster,e16-2.16-2og-musik-lukarne,e17-wlan,e18-loxone,e19,\
    e20,e21,e22,e23" vlan-ids=1
/interface ethernet switch
set 0 l3-hw-offloading=yes
/interface ovpn-server server
add mac-address=FE:F6:E4:C3:F2:43 name=ovpn-server1
/ip address
add address=172.17.1.30/30 interface=e24-uplink network=172.17.1.28
add address=172.16.1.56 interface=lo network=172.16.1.56
add address=10.192.9.1/24 interface=vlan1-lan network=10.192.9.0
add address=172.17.1.73/30 interface=e17-wlan network=172.17.1.72
/ip dhcp-server lease
add address=10.192.9.2 client-id=1:50:4f:94:a2:c6:9f comment=\
    "Loxone Miniserver 926" mac-address=50:4F:94:A2:C6:9F server=dhcp-lan
/ip dhcp-server network
add address=10.192.9.0/24 dns-server=172.31.88.2 gateway=10.192.9.1
/ip dns
set servers=172.16.1.4
/ip hotspot profile
set [ find default=yes ] html-directory=hotspot
/ip ipsec profile
set [ find default=yes ] dpd-interval=2m dpd-maximum-failures=5
/ipv6 address
add address=2a02:16a:fe00:114::1 interface=vlan1-lan
add address=2a02:16a:fe00:ffff:ffff:ffff:ffff:ff56/128 advertise=no \
    interface=lo
/ipv6 nd
set [ find default=yes ] hop-limit=64 interface=vlan1-lan
/routing bfd configuration
add interfaces=e24-uplink
/routing bgp connection
add afi=ip instance=bgp-instance-1 local.role=ebgp name=bgp-klr-sw1-v4 \
    remote.address=172.17.1.29 templates=default
/routing ospf interface-template
add area=backbone-v2 disabled=no interfaces=e24-uplink,e17-wlan
add area=backbone-v3 disabled=no interfaces=e24-uplink,e17-wlan
/routing pimsm interface-template
add disabled=yes instance=pimsm-instance1 interfaces=vlan1-lan,e24-uplink
/system clock
set time-zone-name=Europe/Zurich
/system identity
set name=wng-926-sw1
/system logging
add action=remote topics=info
add action=remote topics=warning
add action=remote topics=error
add action=remote topics=critical
/system routerboard settings
set auto-upgrade=yes
/tool romon
set enabled=yes
/tool sniffer
set filter-interface=e18-loxone filter-mac-address=\
    50:4F:94:A2:C6:9F/FF:FF:FF:FF:FF:FF streaming-enabled=yes \
    streaming-server=10.192.6.8
