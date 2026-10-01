# 2026-10-01 17:27:38 by RouterOS 7.20.8
# software id = BVMW-0V2Z
#
# model = cAPGi-5HaxD2HaxD
# serial number = HGQ09J3AHAZ
/interface bridge
add name=bridge
add name=bridge-caps protocol-mode=none vlan-filtering=yes
/interface wifi
set [ find default-name=wifi1 ] configuration.mode=station .ssid=hamue \
    disabled=no security.authentication-types=wpa2-psk,wpa3-psk
# managed by CAPsMAN A6:F4:9C:07:0B:BC%vlan99-mgmt, traffic processing on CAP
# mode: AP, SSID: berg, channel: 2472/ax/eC
set [ find default-name=wifi2 ] configuration.manager=capsman .mode=ap \
    disabled=no
/interface vxlan
add dont-fragment=disabled local-address=172.16.1.61 mac-address=\
    26:F0:38:58:93:48 name=vxlan-client-berg port=8472 vni=1
/interface vlan
add interface=bridge-caps name=vlan99-mgmt vlan-id=99
/interface ethernet switch
set 0 cpu-flow-control=yes
/interface wifi datapath
add bridge=bridge-caps disabled=no name=dp-caps
/interface wifi configuration
add datapath=dp-caps disabled=no manager=capsman name=capsman
/routing ospf instance
add disabled=no name=default-v2 originate-default=if-installed redistribute=\
    connected,static
add disabled=no name=default-v3 originate-default=if-installed redistribute=\
    connected,static version=3
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
add bridge=bridge disabled=yes interface=ether1
add bridge=bridge interface=ether2
add bridge=bridge-caps interface=vxlan-client-berg
/ip neighbor discovery-settings
set discover-interface-list=!dynamic
/interface bridge vlan
add bridge=bridge-caps tagged=vxlan-client-berg vlan-ids=1-4094
/interface ovpn-server server
add mac-address=FE:E8:8B:BA:1A:5B name=ovpn-server1
/interface vxlan vteps
add interface=vxlan-client-berg remote-ip=172.16.1.1
add interface=vxlan-client-berg remote-ip=172.16.1.6
add interface=vxlan-client-berg remote-ip=172.16.1.53
add interface=vxlan-client-berg remote-ip=172.16.1.57
add interface=vxlan-client-berg remote-ip=172.16.1.59
add interface=vxlan-client-berg remote-ip=172.16.1.62
add interface=vxlan-client-berg remote-ip=172.16.1.63
add interface=vxlan-client-berg remote-ip=172.16.1.64
/interface wifi cap
set caps-man-addresses="" discovery-interfaces=vlan99-mgmt enabled=yes
/interface wifi capsman
set package-path=sd1-part1/upgrade require-peer-certificate=no \
    upgrade-policy=suggest-same-version
/interface wifi provisioning
add action=create-dynamic-enabled address-ranges=\
    172.16.1.1-172.16.1.3,172.16.2.1-172.16.2.1,::1-::2 disabled=yes \
    master-configuration=capsman
add action=create-dynamic-enabled disabled=yes master-configuration=capsman
/ip address
add address=10.172.12.66/30 interface=ether1 network=10.172.12.64
add address=172.16.1.61 interface=lo network=172.16.1.61
/ip dhcp-client
add default-route-tables=main disabled=yes interface=bridge
add default-route-distance=120 default-route-tables=main interface=wifi1 \
    use-peer-ntp=no
/ip firewall nat
add action=masquerade chain=srcnat out-interface=wifi1
/ip ipsec profile
set [ find default=yes ] dpd-interval=2m dpd-maximum-failures=5
/ipv6 address
add address=2a02:16a:fe00:ffff:ffff:ffff:ffff:ff61/128 advertise=no \
    interface=lo
/routing ospf interface-template
add area=backbone-v2 cost=100 disabled=no interfaces=ether1
add area=backbone-v3 cost=100 disabled=no interfaces=ether1
/system clock
set time-zone-name=Asia/Yerevan
/system identity
set name=ap-mimi-dg
/system logging
add action=remote topics=info
add action=remote topics=warning
add action=remote topics=error
add action=remote topics=critical
/system note
set show-at-login=no
/system package update
set channel=long-term
