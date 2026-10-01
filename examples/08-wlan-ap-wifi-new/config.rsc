# 2026-10-01 15:31:58 by RouterOS 7.20.8
# software id = 3FGV-MD8P
#
# model = cAPGi-5HaxD2HaxD
# serial number = HKE0AV969HA
/interface bridge
add name=bridge-caps protocol-mode=none vlan-filtering=yes
/interface vxlan
add dont-fragment=disabled mac-address=02:56:CA:68:2B:28 name=\
    vxlan-client-berg port=8472 vni=1
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
add bridge=bridge-caps interface=vxlan-client-berg
/ip neighbor discovery-settings
set discover-interface-list=!dynamic
/interface bridge vlan
add bridge=bridge-caps tagged=vxlan-client-berg vlan-ids=1-20,99
/interface vxlan vteps
add interface=vxlan-client-berg remote-ip=172.16.1.1
/interface wifi cap
set caps-man-addresses="" discovery-interfaces=vlan99-mgmt enabled=yes
/interface wifi capsman
set package-path=sd1-part1/upgrade require-peer-certificate=no \
    upgrade-policy=suggest-same-version
/interface wifi provisioning
add action=create-dynamic-enabled disabled=no master-configuration=capsman
/ip address
add address=172.16.1.62 interface=lo network=172.16.1.62
/ip dhcp-client
add add-default-route=no interface=ether1
/ipv6 address
add address=2a02:16a:fe00:ffff:ffff:ffff:ffff:ff62/128 advertise=no \
    interface=lo
/routing ospf interface-template
add area=backbone-v2 cost=100 disabled=no interfaces=ether1
add area=backbone-v3 cost=100 disabled=no interfaces=ether1
/system clock
set time-zone-name=Europe/Zurich
/system identity
set name=ap-schopf
/system logging
add action=remote topics=info
add action=remote topics=warning
add action=remote topics=error
add action=remote topics=critical
/system package update
set channel=long-term
/system routerboard settings
set auto-upgrade=yes
