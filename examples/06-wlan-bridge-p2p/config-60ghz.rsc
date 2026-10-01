# 2026-10-01 10:56:19 by RouterOS 7.20.6
# software id = UI9B-A8ZR
#
# model = RBwAPG-60ad-A
# serial number = HDD087PFBQV
/interface bridge
add admin-mac=18:FD:74:F5:43:A2 auto-mac=no comment=defconf name=bridge \
    port-cost-mode=short protocol-mode=none
/interface w60g
set [ find ] disabled=no isolate-stations=no mode=station-bridge name=\
    wlan60-1 put-stations-in-bridge=bridge ssid=hegglin-schopf
/interface lte apn
set [ find default=yes ] ip-type=ipv4 use-network-apn=no
/interface wireless security-profiles
set [ find default=yes ] supplicant-identity=MikroTik
/ip smb users
set [ find default=yes ] disabled=yes
/system logging action
set 3 cef-event-delimiter="" remote=2a02:16a:fe00:1a0::10:8 \
    remote-log-format=cef syslog-time-format=iso8601
/interface bridge port
add bridge=bridge comment=defconf ingress-filtering=no interface=ether1 \
    internal-path-cost=10 path-cost=10
add bridge=bridge interface=wlan60-1 internal-path-cost=10 path-cost=10
/ip firewall connection tracking
set udp-timeout=10s
/ipv6 settings
set forward=no max-neighbor-entries=8192
/interface ovpn-server server
add auth=sha1,md5 mac-address=FE:43:BB:80:B9:06 name=ovpn-server1
/ip dhcp-client
add interface=bridge
/ip ipsec profile
set [ find default=yes ] dpd-interval=2m dpd-maximum-failures=5
/ip smb shares
set [ find default=yes ] directory=/flash/pub
/routing bfd configuration
add disabled=no interfaces=all min-rx=200ms min-tx=200ms multiplier=5
/system clock
set time-zone-name=Asia/Yerevan
/system identity
set name=schopf-hegglin
/system leds
set 0 leds=led1,led2,led3,led4
/system logging
add action=remote topics=info
/system routerboard settings
set auto-upgrade=yes
/tool romon
set enabled=yes
