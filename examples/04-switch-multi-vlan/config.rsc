# 2026-10-01 16:58:07 by RouterOS 7.20.8
# software id = MDMB-RXN5
#
# model = CRS326-24G-2S+
# serial number = 945509C5976A
/interface bridge
add ingress-filtering=no name=switch port-cost-mode=short protocol-mode=mstp \
    pvid=5 vlan-filtering=yes
/interface ethernet
set [ find default-name=ether1 ] name=e01-1.01-eingang-l
set [ find default-name=ether2 ] name=e02-1.02-eingang-r
set [ find default-name=ether3 ] name=e03-1.03-vorraum-tuere-l
set [ find default-name=ether4 ] name=e04-1.04-vorraum-tuere-r
set [ find default-name=ether5 ] name=e05-1.05-bad-l
set [ find default-name=ether6 ] name=e06-1.06-bad-r
set [ find default-name=ether7 ] name=e07-1.07-kueche-l
set [ find default-name=ether8 ] name=e08-1.08-kueche-r
set [ find default-name=ether9 ] name=e09-1.09-wohnz-l
set [ find default-name=ether10 ] name=e10-1.10-wohnz-r
set [ find default-name=ether11 ] name=e11-1.11-buero-tuer-l
set [ find default-name=ether12 ] name=e12-1.12-buero-tuer-r
set [ find default-name=ether13 ] name=e13-1.13-buero-ecke-l-pc
set [ find default-name=ether14 ] name=e14-1.14-buero-ecke-r-phone
set [ find default-name=ether15 ] name=e15-1.15-buero-wand-l
set [ find default-name=ether16 ] name=e16-1.16-buero-wand-r
set [ find default-name=ether17 ] name=e17-1.17-schlupf-l
set [ find default-name=ether18 ] name=e18-1.18-schupf-r
set [ find default-name=ether19 ] name=e19-1.19-z1-wand-l
set [ find default-name=ether20 ] name=e20-1.20-z1-wand-r
set [ find default-name=ether21 ] name=e21-1.21-z1-fenster-l
set [ find default-name=ether22 ] name=e22-1.22-z1-fenster-r
set [ find default-name=ether23 ] name=e23-1.23-z2-tuer-l
set [ find default-name=ether24 ] name=e24-1.24-z2-tuer-r
set [ find default-name=sfp-sfpplus1 ] name=s01-1og-sw2
set [ find default-name=sfp-sfpplus2 ] name=s02-klr-sw3
/interface vlan
add interface=switch name=vlan1-lan vlan-id=1
add interface=switch name=vlan6-work vlan-id=6
/interface lte apn
set [ find default=yes ] ip-type=ipv4 use-network-apn=no
/ip pool
add name=dhcp-work ranges=10.192.6.2-10.192.6.254
add name=dhcp-lan ranges=10.192.5.100-10.192.5.254
/ip dhcp-server
add address-pool=dhcp-work interface=vlan6-work lease-time=10m name=dhcp-work
add address-pool=dhcp-lan interface=vlan1-lan lease-time=10m name=dhcp-lan
/ip smb users
set [ find default=yes ] disabled=yes
/port
set 0 name=serial0
/routing bgp instance
add as=4200000013 name=bgp-instance-1 vrf=main
/routing bgp template
set default as=4200000013 disabled=no output.default-originate=if-installed \
    .network=bgp-networks .redistribute=connected,bgp routing-table=main \
    use-bfd=yes
/routing ospf instance
add disabled=no name=default-v2 redistribute=connected
add disabled=no name=default-v3 redistribute=connected version=3
/routing ospf area
add disabled=no instance=default-v2 name=backbone-v2
add disabled=no instance=default-v3 name=backbone-v3
/routing pimsm instance
add disabled=no name=pimsm-instance1 vrf=main
/system logging action
set 3 cef-event-delimiter="" remote=2a02:16a:fe00:1a0::10:8 \
    remote-log-format=cef syslog-time-format=iso8601
/user group
add name=prometheus policy="read,api,!local,!telnet,!ssh,!ftp,!reboot,!write,!\
    policy,!test,!winbox,!password,!web,!sniff,!sensitive,!romon,!rest-api"
/interface bridge port
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e01-1.01-eingang-l internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e02-1.02-eingang-r internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e03-1.03-vorraum-tuere-l internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e04-1.04-vorraum-tuere-r internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e05-1.05-bad-l internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e06-1.06-bad-r internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e07-1.07-kueche-l internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e08-1.08-kueche-r internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e09-1.09-wohnz-l internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e10-1.10-wohnz-r internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e11-1.11-buero-tuer-l internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e12-1.12-buero-tuer-r internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e13-1.13-buero-ecke-l-pc internal-path-cost=10 path-cost=10 \
    pvid=6
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e14-1.14-buero-ecke-r-phone internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e15-1.15-buero-wand-l internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e16-1.16-buero-wand-r internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e17-1.17-schlupf-l internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e18-1.18-schupf-r internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e19-1.19-z1-wand-l internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e20-1.20-z1-wand-r internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e21-1.21-z1-fenster-l internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e22-1.22-z1-fenster-r internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e23-1.23-z2-tuer-l internal-path-cost=10 path-cost=10
add bridge=switch frame-types=admit-only-untagged-and-priority-tagged \
    interface=e24-1.24-z2-tuer-r internal-path-cost=10 path-cost=10
/interface ethernet switch l3hw-settings
set ipv6-hw=yes
/ip firewall connection tracking
set udp-timeout=10s
/ip neighbor discovery-settings
set discover-interface-list=!dynamic
/ip settings
set max-neighbor-entries=8192
/interface bridge vlan
add bridge=switch tagged=switch untagged="e01-1.01-eingang-l,e02-1.02-eingang-\
    r,e03-1.03-vorraum-tuere-l,e04-1.04-vorraum-tuere-r,e05-1.05-bad-l,e06-1.0\
    6-bad-r,e07-1.07-kueche-l,e08-1.08-kueche-r,e09-1.09-wohnz-l,e10-1.10-wohn\
    z-r,e11-1.11-buero-tuer-l,e12-1.12-buero-tuer-r,e15-1.15-buero-wand-l,e16-\
    1.16-buero-wand-r,e17-1.17-schlupf-l,e18-1.18-schupf-r,e19-1.19-z1-wand-l,\
    e20-1.20-z1-wand-r,e21-1.21-z1-fenster-l,e22-1.22-z1-fenster-r,e23-1.23-z2\
    -tuer-l,e24-1.24-z2-tuer-r,e14-1.14-buero-ecke-r-phone" vlan-ids=1
add bridge=switch tagged=switch untagged=e13-1.13-buero-ecke-l-pc vlan-ids=6
add bridge=switch tagged=switch vlan-ids=5
/interface ethernet switch
set 0 l3-hw-offloading=yes
/interface ovpn-server server
add auth=sha1,md5 mac-address=FE:45:45:7B:AD:13 name=ovpn-server1
/ip address
add address=172.17.1.17/30 interface=s02-klr-sw3 network=172.17.1.16
add address=172.16.1.54 interface=lo network=172.16.1.54
add address=10.192.6.1/24 interface=vlan6-work network=10.192.6.0
add address=172.17.1.22/30 interface=s01-1og-sw2 network=172.17.1.20
add address=10.192.5.1/24 interface=vlan1-lan network=10.192.5.0
add address=192.168.88.2/24 disabled=yes interface=vlan1-lan network=\
    192.168.88.0
/ip dhcp-server network
add address=10.192.5.0/24 boot-file-name=netboot.xyz-snp.efi dns-server=\
    172.16.1.1 gateway=10.192.5.1 next-server=172.16.1.1
add address=10.192.6.0/24 boot-file-name=netboot.xyz-snp.efi dns-server=\
    172.31.88.1 gateway=10.192.6.1 next-server=172.16.1.1
/ip dns
set allow-remote-requests=yes servers=172.16.1.1
/ip dns static
add address=172.31.0.14 name=api.kube.berg-turbenthal.ch type=A
add address=172.31.0.16 name=api.kube.berg-turbenthal.ch type=A
add address=172.31.0.13 name=api.kube.berg-turbenthal.ch type=A
/ip ipsec profile
set [ find default=yes ] dpd-interval=2m dpd-maximum-failures=5
/ip route
add disabled=yes distance=1 dst-address=0.0.0.0/0 gateway=10.192.65.1 \
    pref-src="" routing-table=main scope=30 suppress-hw-offload=no \
    target-scope=10
add check-gateway=ping disabled=no distance=1 dst-address=10.220.3.1/32 \
    gateway=10.192.5.107 routing-table=main suppress-hw-offload=no
/ip service
set www-ssl disabled=no
/ip smb shares
set [ find default=yes ] directory=/flash/pub
/ipv6 address
add address=fd7e:907d:34ab::54/128 advertise=no interface=lo
add address=2a02:16a:fe00:110::1 interface=vlan1-lan
add address=2a02:16a:fe00:102::1 interface=vlan6-work
add address=2a02:16a:fe00:ffff:ffff:ffff:ffff:ff54/128 advertise=no \
    interface=lo
/ipv6 nd
set [ find default=yes ] dns=2a02:16a:fe00:1a0::10:5 hop-limit=64 interface=\
    vlan1-lan
add advertise-mac-address=no dns=2a02:16a:fe00:1a0::10:3 hop-limit=64 \
    interface=vlan6-work
/routing bfd configuration
add disabled=no
add disabled=no interfaces=s01-1og-sw2,s02-klr-sw3
/routing bgp connection
add afi=ip disabled=yes instance=bgp-instance-1 local.role=ebgp name=\
    bgp-klr-sw1-v4 output.default-originate=if-installed .network=\
    bgp-networks .redistribute=connected,bgp remote.address=172.17.1.18 \
    routing-table=main templates=default use-bfd=yes
add afi=ipv6 disabled=yes instance=bgp-instance-1 local.role=ebgp name=\
    bgp-klr-sw1-v6 output.default-originate=if-installed .network=\
    bgp-networks .redistribute=connected,bgp remote.address=\
    fe80::c6ad:34ff:fea1:6da%s02-klr-sw3 routing-table=main templates=default \
    use-bfd=yes
add afi=ip disabled=no instance=bgp-instance-1 local.role=ebgp name=\
    bgp-1og-sw2-v4 output.network=bgp-networks .redistribute=connected,bgp \
    remote.address=172.17.1.21 routing-table=main templates=default use-bfd=\
    yes
add afi=ipv6 disabled=no instance=bgp-instance-1 local.role=ebgp name=\
    bgp-1og-sw2-v6 remote.address=fe80::c6ad:34ff:fe4f:e394%s01-1og-sw2 \
    templates=default
/routing gmp
add disabled=no groups=239.255.255.250 interfaces=vlan1-lan
/routing igmp-proxy mfc
add disabled=yes downstream-interfaces=s01-1og-sw2 group=239.255.255.250 \
    source=0.0.0.0 upstream-interface=vlan1-lan
/routing ospf interface-template
add area=backbone-v2 disabled=no interfaces=s02-klr-sw3,s01-1og-sw2
add area=backbone-v3 disabled=no interfaces=s02-klr-sw3,s01-1og-sw2
add area=backbone-v2 disabled=no interfaces=vlan1-lan
/routing pimsm interface-template
add disabled=no instance=pimsm-instance1 interfaces=vlan1-lan \
    join-tracking-support=yes
/system clock
set time-zone-name=Asia/Yerevan
/system identity
set name=1og-sw1
/system logging
add action=remote topics=info
add action=remote topics=warning
add action=remote topics=error
add action=remote topics=critical
/system ntp client
set enabled=yes
/system ntp client servers
add address=ch.pool.ntp.org
/system package update
set channel=long-term
/system routerboard settings
set auto-upgrade=yes
/tool romon
set enabled=yes
/tool sniffer
set filter-interface=e14-1.14-buero-ecke-r-phone filter-mac-address=\
    24:4B:FE:52:66:D1/FF:FF:FF:FF:FF:FF streaming-enabled=yes \
    streaming-server=10.192.6.8
