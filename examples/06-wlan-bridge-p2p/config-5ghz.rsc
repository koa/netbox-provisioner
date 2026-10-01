# 2026-10-01 09:01:25 by RouterOS 7.20.8
# software id = D971-FX10
#
# model = RBDynaDishG-5HacDr3
# serial number = 705D073DF70E
/interface bridge
add name=lan port-cost-mode=short protocol-mode=none
/interface wireless
set [ find default-name=wlan1 ] band=5ghz-onlyac channel-width=\
    20/40/80mhz-XXXX country=no_country_set disabled=no frequency=auto \
    frequency-mode=manual-txpower mode=station-bridge nv2-security=enabled \
    ssid=hegglin-bachmann station-roaming=enabled wireless-protocol=\
    nv2-nstreme-802.11
/interface ethernet
set [ find default-name=ether1 ] loop-protect=off
/interface wireless nstreme
set wlan1 enable-nstreme=yes
/interface lte apn
set [ find default=yes ] ip-type=ipv4 use-network-apn=no
/interface wireless security-profiles
set [ find default=yes ] authentication-types=wpa-psk,wpa2-psk group-ciphers=\
    tkip,aes-ccm mode=dynamic-keys supplicant-identity=MikroTik \
    unicast-ciphers=tkip,aes-ccm
/system logging action
set 3 cef-event-delimiter="" remote=2a02:16a:fe00:1a0::10:8 \
    remote-log-format=cef syslog-time-format=iso8601
/interface bridge port
add bridge=lan ingress-filtering=no interface=wlan1 internal-path-cost=10 \
    path-cost=10
add bridge=lan hw=no ingress-filtering=no interface=ether1 \
    internal-path-cost=10 path-cost=10
/ip firewall connection tracking
set udp-timeout=10s
/ip neighbor discovery-settings
set discover-interface-list=!dynamic
/ip settings
set max-neighbor-entries=8192
/ipv6 settings
set forward=no max-neighbor-entries=8192
/interface ovpn-server server
add auth=sha1,md5 mac-address=FE:BA:57:A7:4A:48 name=ovpn-server1
/interface wireless align
set receive-all=yes
/ip dhcp-client
add interface=lan
/ip ipsec profile
set [ find default=yes ] dpd-interval=2m dpd-maximum-failures=5
/ipv6 nd
set [ find default=yes ] advertise-dns=no
/routing bfd configuration
add disabled=no interfaces=all min-rx=200ms min-tx=200ms multiplier=5
/system clock
set time-zone-name=Europe/Zurich
/system identity
set name=bachmann-hegglin-2
/system logging
add action=remote topics=info
/system ntp client
set mode=broadcast
/system package update
set channel=long-term
/system routerboard settings
set auto-upgrade=yes
/tool romon
set enabled=yes
