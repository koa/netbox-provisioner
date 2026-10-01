# 2026-10-01 15:27:53 by RouterOS 7.20.8
# software id = KCR4-RU56
#
# model = RB962UiGS-5HacT2HnT
# serial number = 6737068FAE81
/interface bridge
add name=switch port-cost-mode=short protocol-mode=none
/interface wireless
# managed by CAPsMAN
# channel: 2412/20/gn(13dBm), SSID: berg, CAPsMAN forwarding
set [ find default-name=wlan1 ] ssid=MikroTik
# managed by CAPsMAN
# channel: 5180/20-Ce/an/P(11dBm), SSID: berg, CAPsMAN forwarding
set [ find default-name=wlan2 ] ssid=MikroTik
/interface ethernet
set [ find default-name=ether1 ] name=e01-uplink
set [ find default-name=ether2 ] name=e02-fronius
set [ find default-name=ether3 ] name=e03-byd
set [ find default-name=sfp1 ] advertise=\
    10M-baseT-half,10M-baseT-full,100M-baseT-half,100M-baseT-full
/interface vlan
add interface=switch name=vlan5-mgmt vlan-id=5
/interface wireless security-profiles
set [ find default=yes ] supplicant-identity=MikroTik
/ip pool
add name=dhcp_pool0 ranges=10.192.68.20-10.192.68.254
/ip dhcp-server
add address-pool=dhcp_pool0 interface=switch name=dhcp1
/routing bgp instance
add as=4200000010 name=bgp-instance-1 vrf=main
/routing bgp template
set default as=4200000010 disabled=no output.default-originate=if-installed \
    .redistribute=connected routing-table=main use-bfd=yes
/routing id
add disabled=no id=172.16.1.53 name=id select-dynamic-id=any select-from-vrf=\
    main
/routing ospf instance
add disabled=no name=default-v2 redistribute=connected
add disabled=no name=default-v3 redistribute=connected version=3
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
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e02-fronius internal-path-cost=10 path-cost=10 pvid=7
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e03-byd internal-path-cost=10 path-cost=10 pvid=7
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=ether4 internal-path-cost=10 path-cost=10 pvid=7
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=ether5 internal-path-cost=10 path-cost=10 pvid=7
/ip firewall connection tracking
set udp-timeout=10s
/interface ovpn-server server
add mac-address=FE:69:C7:B6:0E:31 name=ovpn-server1
/interface wifi cap
set caps-man-addresses=2a02:16a:fe00:ffff:ffff:ffff:ffff:ff01
/interface wireless cap
# 
set caps-man-addresses=2a02:16a:fe00:ffff:ffff:ffff:ffff:ff01 enabled=yes \
    interfaces=wlan1,wlan2
/ip address
add address=172.17.1.6/30 interface=e01-uplink network=172.17.1.4
add address=10.192.68.1/24 interface=switch network=10.192.68.0
add address=172.16.1.53 interface=lo network=172.16.1.53
/ip dhcp-server lease
add address=10.192.68.10 client-id=1:0:3:ac:4e:8f:70 comment=fronius \
    mac-address=00:03:AC:4E:8F:70 server=dhcp1
add address=10.192.68.11 client-id=1:40:d6:3c:aa:4c:ee comment="BYD Batterie" \
    mac-address=40:D6:3C:AA:4C:EE server=dhcp1
/ip dhcp-server network
add address=10.192.68.0/24 dns-server=172.16.1.53 gateway=10.192.68.1
/ip dns
set allow-remote-requests=yes servers=172.16.1.1
/ip firewall address-list
add address=192.168.0.0/16 list=internal
add address=172.16.0.0/12 list=internal
add address=10.192.0.0/12 list=internal
/ip firewall filter
add action=reject chain=forward dst-address-list=!internal in-interface=\
    switch reject-with=icmp-admin-prohibited src-mac-address=\
    40:D6:3C:AA:4C:EE
/ip hotspot profile
set [ find default=yes ] html-directory=hotspot
/ip ipsec profile
set [ find default=yes ] dpd-interval=2m dpd-maximum-failures=5
/ipv6 address
add address=fd7e:907d:34ab::53/128 advertise=no interface=lo
add address=2a02:16a:fe00:1c1::1 interface=switch
add address=2a02:16a:fe00:ffff:ffff:ffff:ffff:ff53/128 advertise=no \
    interface=lo
/ipv6 nd
set [ find default=yes ] interface=switch
/routing bfd configuration
add disabled=no
/routing bgp connection
add afi=ip disabled=yes instance=bgp-instance-1 local.role=ebgp name=\
    bgp-klr-sw2 remote.address=172.17.1.5 templates=default
/routing ospf interface-template
add area=backbone-v2 disabled=no interfaces=e01-uplink
add area=backbone-v3 disabled=no interfaces=e01-uplink
/system clock
set time-zone-name=Europe/Zurich
/system identity
set name=ap-klr-solar
/system logging
add action=remote topics=info
add action=remote topics=warning
add action=remote topics=error
add action=remote topics=critical
/system package update
set channel=long-term
/system routerboard settings
# Firmware upgraded successfully, please reboot for changes to take effect!
set auto-upgrade=yes
/tool romon
set enabled=yes
/tool sniffer
set filter-interface=switch filter-mac-address=\
    40:D6:3C:AA:4C:EE/FF:FF:FF:FF:FF:FF streaming-enabled=yes \
    streaming-server=10.192.6.10
