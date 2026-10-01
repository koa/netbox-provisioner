# 2026-10-01 08:54:27 by RouterOS 7.23.7
# software id = 0IRN-GUJ7
#
# model = CRS309-1G-8S+
# serial number = HH90ADGH74C
/interface bridge
add name=switch protocol-mode=none vlan-filtering=yes
/interface ethernet
set [ find default-name=sfp-sfpplus1 ] name=s01-sr02
set [ find default-name=sfp-sfpplus2 ] name=s02-sr03
set [ find default-name=sfp-sfpplus3 ] name=s03-sr04
set [ find default-name=sfp-sfpplus7 ] name=s07-1og-sw1
set [ find default-name=sfp-sfpplus8 ] name=s08-klr-sw-1
/interface vlan
add interface=switch name=vlan1-srv vlan-id=1
/routing bgp instance
add as=4200000002 name=bgp-instance-1 router-id=172.16.1.51
/routing bgp template
set default as=4200000002 disabled=no output.default-originate=if-installed \
    .redistribute=ospf,bgp routing-table=main use-bfd=no
/routing ospf instance
add disabled=no name=default-v2 redistribute=connected,bgp
add disabled=no name=default-v3 redistribute=connected,bgp version=3
/routing ospf area
add disabled=no instance=default-v2 name=backbone-v2
add disabled=no instance=default-v3 name=backbone-v3
/system logging action
set 3 cef-event-delimiter="" remote=2a02:16a:fe00:1a0::10:8 \
    remote-log-format=cef syslog-time-format=iso8601
/user group
add name=prometheus policy="read,api,!local,!telnet,!ssh,!ftp,!reboot,!write,!\
    policy,!test,!winbox,!password,!web,!sniff,!sensitive,!romon,!rest-api"
/interface bridge port
add bridge=switch interface=s01-sr02
add bridge=switch interface=s02-sr03
add bridge=switch interface=s03-sr04
/interface ethernet switch l3hw-settings
set autorestart=yes ipv6-hw=yes
/ip neighbor discovery-settings
set discover-interface-list=!dynamic
/interface bridge vlan
add bridge=switch untagged=s01-sr02,s02-sr03,s03-sr04 vlan-ids=1
/interface ethernet switch
set switch1 l3-hw-offloading=yes
/interface ovpn-server server
add mac-address=FE:A3:66:7C:DC:51 name=ovpn-server1
/ip address
add address=172.17.1.33/30 interface=s08-klr-sw-1 network=172.17.1.32
add address=172.16.1.58 interface=lo network=172.16.1.58
add address=172.17.1.18/30 interface=s07-1og-sw1 network=172.17.1.16
add address=172.31.0.1/24 interface=vlan1-srv network=172.31.0.0
/ip dhcp-client
add interface=s08-klr-sw-1 name=sfp-sfpplus8
/ip dns
set allow-remote-requests=yes servers=172.16.1.1
/ip hotspot profile
set [ find default=yes ] html-directory=hotspot
/ip ipsec profile
set [ find default=yes ] dpd-interval=2m dpd-maximum-failures=5
/ipv6 address
add address=2a02:168:e000:100::1 advertise=no disabled=yes interface=\
    vlan1-srv
add address=2a02:16a:fe00:100::1 interface=vlan1-srv
add address=2a02:16a:fe00:ffff:ffff:ffff:ffff:ff58/128 advertise=no \
    interface=lo
/ipv6 nd
# automatic dns option advertising is not started, re-apply dns config
set [ find default=yes ] advertise-dns=yes dns=2a02:16a:fe00:100::1 \
    hop-limit=64 interface=vlan1-srv
/routing bgp connection
add afi=ip as=4200000002 cisco-vpls-nlri-len-fmt=auto-bits connect=yes \
    disabled=no instance=bgp-instance-1 listen=yes local.address=172.31.0.1 \
    .role=ibgp multihop=yes name=sr03 output.default-originate=never \
    .network=bgp-networks .redistribute=connected remote.address=172.31.0.12 \
    .port=179 routing-table=main templates=default use-bfd=no
add afi=ipv6 as=4200000002 connect=yes disabled=no instance=bgp-instance-1 \
    local.address=2a02:16a:fe00:100::1 .port=179 .role=ibgp name=sr03-v6 \
    remote.address=2a02:16a:fe00:100::12 .port=1179 templates=default
add afi=ip as=4200000002 cisco-vpls-nlri-len-fmt=auto-bits connect=yes \
    disabled=no instance=bgp-instance-1 listen=yes local.address=172.31.0.1 \
    .role=ibgp multihop=yes name=sr04 output.default-originate=never \
    .network=bgp-networks .redistribute=connected remote.address=172.31.0.13 \
    .port=179 routing-table=main templates=default use-bfd=no
add afi=ipv6 as=4200000002 connect=yes disabled=no instance=bgp-instance-1 \
    local.address=2a02:16a:fe00:100::1 .port=179 .role=ibgp name=sr04-v6 \
    remote.address=2a02:16a:fe00:100::13 .port=1179 templates=default
add afi=ip as=4200000002 cisco-vpls-nlri-len-fmt=auto-bits connect=yes \
    disabled=no instance=bgp-instance-1 listen=yes local.address=172.31.0.1 \
    .role=ibgp multihop=yes name=sr02 output.default-originate=never \
    .network=bgp-networks .redistribute=connected remote.address=172.31.0.11 \
    .port=179 routing-table=main templates=default use-bfd=no
add afi=ipv6 as=4200000002 connect=yes disabled=no instance=bgp-instance-1 \
    local.address=2a02:16a:fe00:100::1 .port=179 .role=ibgp name=sr02-v6 \
    remote.address=2a02:16a:fe00:100::11 .port=1179 templates=default
/routing filter rule
add chain=calico-out disabled=yes rule="if (bgp-path-len > 3) {reject }"
/routing ospf interface-template
add area=backbone-v2 disabled=no interfaces=s08-klr-sw-1,s07-1og-sw1
add area=backbone-v3 disabled=no interfaces=s08-klr-sw-1,s07-1og-sw1
/system clock
set time-zone-name=Europe/Zurich
/system identity
set name=klr-sw-3
/system logging
add action=remote topics=info
add action=remote topics=warning
add action=remote topics=error
add action=remote topics=critical
/system package update
set channel=long-term
/system routerboard settings
# Firmware upgraded successfully, please reboot for changes to take effect!
set auto-upgrade=yes enter-setup-on=delete-key
/tool sniffer
set filter-interface=s02-sr03 filter-mac-protocol=ipv6 streaming-enabled=yes \
    streaming-server=10.192.6.8
