# 2026-10-01 14:56:45 by RouterOS 7.21.4
# software id = B3D9-96J6
#
# model = RB960PGS
# serial number = 78D20725D85B
/interface bridge
add name=loopback port-cost-mode=short
/interface ethernet
set [ find default-name=ether2 ] name=e02-hegglin
set [ find default-name=ether3 ] name=e03-walter poe-priority=5
set [ find default-name=ether4 ] name=e04-burri poe-priority=7
set [ find default-name=ether5 ] name=e05-ap-schopf
set [ find default-name=sfp1 ] advertise=\
    10M-baseT-half,10M-baseT-full,100M-baseT-half,100M-baseT-full
/interface lte apn
set [ find default=yes ] ip-type=ipv4 use-network-apn=no
/ip pool
add name=e04-burri-pool ranges=172.17.0.43-172.17.0.46
add name=e03-walter-pool ranges=172.17.0.59-172.17.0.62
add name=e02-hegglin-pool ranges=172.17.0.53-172.17.0.54
add name=e05-ap-pool ranges=172.17.1.66
/ip dhcp-server
add address-pool=e04-burri-pool interface=e04-burri lease-time=10m name=\
    dhcp-e04-burri-link
add address-pool=e03-walter-pool interface=e03-walter lease-time=10m name=\
    dhcp-e03-walter-link
add address-pool=e02-hegglin-pool interface=e02-hegglin lease-time=10m name=\
    dhcp-e02-hegglin-link
add address-pool=e05-ap-pool interface=e05-ap-schopf name=dhcp-ap-schopf
/routing bgp instance
add as=4200000008 name=bgp-instance-1 vrf=main
/routing bgp template
set default as=4200000008 disabled=no multihop=yes output.default-originate=\
    if-installed .network=bgp-networks .redistribute=connected,ospf \
    routing-table=main use-bfd=yes
/routing id
add disabled=no id=172.16.0.8 name=id select-dynamic-id=""
/routing ospf instance
add disabled=no name=default-v2 redistribute=connected,bgp router-id=\
    172.16.1.8
add disabled=no name=default-v3 redistribute=connected,bgp router-id=\
    172.16.1.8 version=3
/routing ospf area
add disabled=no instance=default-v2 name=backbone-v2
add disabled=no instance=default-v3 name=backbone-v3
/system logging action
set 3 cef-event-delimiter="" remote=2a02:16a:fe00:1a0::10:8 \
    remote-log-format=cef syslog-time-format=iso8601
/user group
add name=prometheus policy="read,api,!local,!telnet,!ssh,!ftp,!reboot,!write,!\
    policy,!test,!winbox,!password,!web,!sniff,!sensitive,!romon,!rest-api"
/ip firewall connection tracking
set udp-timeout=10s
/ip settings
set max-neighbor-entries=8192
/ipv6 settings
set max-neighbor-entries=8192
/interface ovpn-server server
add mac-address=FE:F1:89:43:D7:05 name=ovpn-server1
/ip address
add address=172.16.1.8 interface=loopback network=172.16.1.8
add address=172.17.0.41/29 interface=e04-burri network=172.17.0.40
add address=172.17.0.57/29 interface=e03-walter network=172.17.0.56
add address=172.17.0.50/29 interface=e02-hegglin network=172.17.0.48
add address=172.17.1.65/30 interface=e05-ap-schopf network=172.17.1.64
/ip dhcp-server network
add address=172.17.0.40/29 dns-server=172.17.0.41 gateway=172.17.0.41
add address=172.17.0.48/29 dns-server=172.17.0.49 gateway=172.17.0.49
add address=172.17.0.56/29 dns-server=172.17.0.57 gateway=172.17.0.57
add address=172.17.1.64/30 dns-server=172.16.1.8 gateway=172.17.1.65
/ip dns
set allow-remote-requests=yes servers=172.16.0.4
/ip firewall filter
add action=fasttrack-connection chain=forward comment=\
    "fasttrack established/related" connection-state=established,related
add action=accept chain=forward comment="accept established/related" \
    connection-state=established,related
/ip hotspot profile
set [ find default=yes ] html-directory=hotspot
/ip ipsec profile
set [ find default=yes ] dpd-interval=2m dpd-maximum-failures=5
/ipv6 address
add address=fd7e:907d:34ab::8/128 advertise=no interface=lo
add address=2a02:16a:fe00:ffff:ffff:ffff:ffff:ff08/128 advertise=no \
    interface=lo
add address=2a02:16a:fe00:ff84::1 interface=e02-hegglin
add address=2a02:16a:fe00:ff85::1 interface=e03-walter
add address=2a02:16a:fe00:ff86::1 interface=e04-burri
/ipv6 nd
set [ find default=yes ] advertise-dns=yes dns=\
    2a02:16a:fe00:ffff:ffff:ffff:ffff:ff08
/routing bfd configuration
add disabled=no interfaces=all min-rx=200us min-tx=200us multiplier=5
/routing bgp connection
add afi=ipv6 disabled=no instance=bgp-instance-1 local.role=ebgp multihop=yes \
    name=bgp-hegglin-v6 output.default-originate=if-installed .network=\
    bgp-networks remote.address=fe80::ce2d:e0ff:fe48:99a3%e02-hegglin \
    routing-table=main templates=default use-bfd=yes
add afi=ipv6 disabled=yes instance=bgp-instance-1 local.role=ebgp multihop=\
    yes name=bgp-burri-v6 output.default-originate=if-installed .network=\
    bgp-networks .redistribute=connected,ospf remote.address=\
    fe80::4aa9:8aff:feb8:f335%e04-burri routing-table=main templates=default \
    use-bfd=yes
add afi=ipv6 disabled=yes instance=bgp-instance-1 local.role=ebgp multihop=\
    yes name=bgp-walter-v6 output.default-originate=if-installed .network=\
    bgp-networks .redistribute=connected,ospf remote.address=\
    fe80::a55:31ff:fec0:bce3%e03-walter routing-table=main templates=default \
    use-bfd=yes
add afi=ip disabled=no instance=bgp-instance-1 local.role=ebgp multihop=no \
    name=bgp-hegglin-v4 output.default-originate=if-installed .network=\
    bgp-networks remote.address=172.17.0.49 routing-table=main templates=\
    default use-bfd=yes
add afi=ip disabled=yes instance=bgp-instance-1 local.role=ebgp multihop=no \
    name=bgp-walter-v4 output.default-originate=if-installed .network=\
    bgp-networks .redistribute=connected remote.address=172.17.0.58 \
    routing-table=main templates=default use-bfd=yes
add afi=ip disabled=yes instance=bgp-instance-1 local.role=ebgp multihop=no \
    name=bgp-burri-v4 output.default-originate=if-installed .network=\
    bgp-networks .redistribute=connected remote.address=172.17.0.42 \
    routing-table=main templates=default use-bfd=yes
/routing ospf interface-template
add area=backbone-v2 disabled=no interfaces=loopback networks=172.16.0.8 \
    priority=1
add area=backbone-v2 auth-id=1 cost=10 disabled=no interfaces=\
    e03-walter,e04-burri,e02-hegglin,e05-ap-schopf priority=1 use-bfd=no
add area=backbone-v3 disabled=no interfaces=\
    e03-walter,e04-burri,e02-hegglin,e05-ap-schopf
/system clock
set time-zone-name=Europe/Zurich
/system identity
set name=router-schopf
/system logging
add topics=poe-out
add action=remote topics=info
add action=remote topics=warning
add action=remote topics=error
add action=remote topics=critical
/system note
set show-at-login=no
/system package update
set channel=long-term
/system routerboard settings
set auto-upgrade=yes
/system watchdog
set ping-start-after-boot=30m watch-address=172.16.1.2
/tool graphing
set store-every=hour
/tool graphing interface
add interface=e02-hegglin
add interface=e03-walter
add interface=e04-burri
/tool graphing resource
add
/tool romon
set enabled=yes
/tool sniffer
set filter-interface=e05-ap-schopf filter-ip-protocol=udp filter-port=8472 \
    streaming-enabled=yes streaming-server=10.192.6.8
