# 2026-10-01 08:51:16 by RouterOS 7.23.7
# software id = M8GX-KDAQ
#
# model = CCR1009-8G-1S-1S+
# serial number = 70C806108A95
/caps-man channel
add band=2ghz-g/n extension-channel=disabled frequency=2412,2432,2452,2472 \
    name=24-autoselect save-selected=yes
add band=5ghz-onlyn extension-channel=XXXX frequency=5180,5220,5260,5300 \
    name=5n-autoselect save-selected=yes
add band=5ghz-onlyac extension-channel=XXXX frequency=5500,5580 name=\
    5ac-autoselect save-selected=yes
/interface bridge
add name=lan port-cost-mode=short
add name=loopback port-cost-mode=short
add igmp-snooping=yes name=wlan-client port-cost-mode=short
/interface ethernet
set [ find default-name=ether1 ] name=e01-ap-keller
set [ find default-name=ether2 ] name=e02
set [ find default-name=ether3 ] name=e03
set [ find default-name=ether4 ] name=e04
set [ find default-name=ether5 ] name=e05-bachmann-dachboden
set [ find default-name=ether6 ] name=e06
set [ find default-name=ether7 ] name=e07
set [ find default-name=ether8 ] name=e08
set [ find default-name=sfp1 ] disabled=yes loop-protect=off name=\
    fiber-uplink-crossover7
set [ find default-name=sfp-sfpplus1 ] name=fiber-uplink-fiber7
/interface ovpn-client
add cipher=aes256-cbc connect-to=impacthub-colab.panter.biz mac-address=\
    02:0E:60:E7:D6:20 name=ovpn-colab-sihlquai port=444 user=koa-home
/interface ovpn-server
add name=ovpn-styghalde user=styghalde
add name=ovpn-tk user=ovpn-teamkoenig
add name=ovpn-wegmann user=wegmann-ungarn
/interface gre
add !keepalive name=gre-backup-link remote-address=10.195.16.9
add local-address=85.195.193.66 name=gre-colab remote-address=212.51.155.234
add disabled=yes local-address=10.195.16.1 name=gre-lkw remote-address=\
    10.195.16.8
add allow-fast-path=no local-address=77.109.174.57 name=gre-tunnel-selnau \
    remote-address=213.160.32.164
/caps-man interface
add disabled=no l2mtu=1600 mac-address=6C:3B:6B:77:0E:80 master-interface=\
    none name=cap1 radio-mac=6C:3B:6B:77:0E:80 radio-name=6C3B6B770E80
add disabled=no l2mtu=1600 mac-address=6C:3B:6B:77:0E:7F master-interface=\
    none name=cap2 radio-mac=6C:3B:6B:77:0E:7F radio-name=6C3B6B770E7F
/interface ipipv6
add !keepalive local-address=2a02:16a:fe00:ffff:ffff:ffff:ffff:ff00 name=\
    ipipv6-loch remote-address=2a02:16a:fe01:ffff:ffff:ffff:ffff:ff00
/interface 6to4
add !keepalive local-address=10.195.16.1 name=6to4-tunnel-lkw remote-address=\
    10.195.16.8
/caps-man datapath
add bridge=wlan-client client-to-client-forwarding=yes name=wlan-faesigrund
/caps-man security
add authentication-types=wpa-psk,wpa2-psk name=default-faesigrund
/caps-man configuration
add channel=24-autoselect country=switzerland datapath=wlan-faesigrund name=\
    faesigrund-24 security=default-faesigrund ssid=faesigrund
add channel=5n-autoselect country=switzerland datapath=wlan-faesigrund name=\
    faesigrund-5n security=default-faesigrund ssid=faesigrund
add channel=5ac-autoselect country=switzerland datapath=wlan-faesigrund name=\
    faesigrund-5ac security=default-faesigrund ssid=faesigrund
/disk
add parent=sd1 partition-number=1 partition-offset=512 partition-size=\
    31104957952 type=partition
/interface list
add exclude=dynamic name=discover
add name=uplink
/interface wireless security-profiles
set [ find default=yes ] supplicant-identity=MikroTik
/ip dhcp-server
# Interface not running
add interface=ovpn-tk lease-time=10m name=dhcp1
/ip ipsec peer
add address=213.160.32.164/32 exchange-mode=ike2 name=panter-selnau
add address=212.51.155.234/32 exchange-mode=ike2 name=panter-colab
add address=178.238.174.80/32 disabled=yes exchange-mode=ike2 name=dag-vpn \
    passive=yes
add address=2a02:16a:fe01:ffff:ffff:ffff:ffff:ff00/128 exchange-mode=ike2 \
    local-address=2a02:16a:fe00:ffff:ffff:ffff:ffff:ff00 name=vpn-michaud
/ip ipsec profile
set [ find default=yes ] dh-group=ecp521,modp8192,modp4096,modp2048,modp1024 \
    dpd-interval=2m dpd-maximum-failures=5 enc-algorithm=aes-128
/ip ipsec proposal
set [ find default=yes ] enc-algorithms=aes-128-cbc,blowfish,twofish
add auth-algorithms=sha512,sha256,sha1 enc-algorithms=\
    aes-256-cbc,aes-256-ctr,aes-256-gcm,aes-128-cbc name=panter-vpn
add auth-algorithms=sha512,sha256,sha1 enc-algorithms=\
    aes-256-cbc,aes-256-ctr,aes-256-gcm,aes-192-cbc,aes-128-cbc name=\
    panter-selnau pfs-group=modp8192
/ip pool
add name=wlan-client-pool ranges=10.195.253.0-10.195.255.254
add name=e05-bachmann-dachboden-pool ranges=172.17.0.5-172.17.0.6
add name=lan.pool ranges=10.195.0.100-10.195.0.254
/ip dhcp-server
add address-pool=wlan-client-pool interface=wlan-client lease-time=10m name=\
    dhcp-client-wlan
add address-pool=e05-bachmann-dachboden-pool interface=e05-bachmann-dachboden \
    lease-time=10m name=dhcp-e05-bachmann-dachboden-link
add address-pool=lan.pool bootp-support=dynamic interface=lan lease-time=10m \
    name=dhcp2
/routing bgp instance
add as=4200000000 name=bgp-instance-1 vrf=main
add as=65531 name=bgp-instance-2 router-id=10.196.0.2
add as=65537 name=bgp-instance-3 router-id=172.16.0.4
/routing bgp template
set default as=65530 disabled=yes output.network=bgp-networks .redistribute=\
    ospf
/routing ospf instance
add disabled=no name=default-v2 originate-default=if-installed \
    out-filter-chain=ospf-out redistribute=connected,bgp router-id=172.16.0.4
add disabled=no name=default-v3 originate-default=if-installed \
    out-filter-chain=ospf-out redistribute=connected,bgp,dhcp router-id=\
    172.16.0.4 version=3
/routing ospf area
add disabled=no instance=default-v2 name=backbone-v2
add disabled=no instance=default-v3 name=backbone-v3
/routing table
add disabled=no fib name=fiber7
/routing bgp template
add as=65531 disabled=no name=bgp-vpn-colab output.filter-chain=bgp-colab \
    .network=bgp-networks .redistribute=connected,ospf,bgp routing-table=main
add as=65537 disabled=no name=bgp-vpn-familie output.filter-chain=vpn-familie \
    .redistribute=connected,ospf,bgp routing-table=main
add as=4200000000 disabled=no name=bgp-pirg output.default-originate=always \
    .redistribute=connected,ospf,bgp routing-table=main use-bfd=yes
/system logging action
set 3 cef-event-delimiter="" remote=2a02:16a:fe00:1a0::10:8 \
    remote-log-format=cef syslog-time-format=iso8601
/system script
add dont-require-permissions=no name=cfg-update owner=admin policy=read,write \
    source=":local WANinterface \"fiber-uplink\"\
    \n\
    \n:global greold4addr\
    \n:local ipv4addr\
    \n\
    \n\
    \n# Get WAN interface IP address\
    \n:set ipv4addr [/ip address get [/ip address find interface=\$WANinterfac\
    e] address]\
    \n:set ipv4addr [:pick [:tostr \$ipv4addr] 0 [:find [:tostr \$ipv4addr] \"\
    /\"]]\
    \n\
    \n\
    \n:if ([:len \$ipv4addr] = 0) do={\
    \n   :log error (\"Could not get IP for interface \" . \$WANinterface)\
    \n   :error (\"Could not get IP for interface \" . \$WANinterface)\
    \n}\
    \n\
    \n:if ([:typeof \$greold4addr] = \"nothing\" || \$greold4addr != \$ipv4add\
    r) do={\
    \n  :log warning (\"Updating local gre IPv4 address\" . \" Client IPv4 add\
    ress to new IP \" . \$ipv4addr . \"...\")\
    \n  /interface/gre/set local-address=\$ipv4addr [find where name=gre-colab\
    ]\
    \n  /interface/gre/set local-address=\$ipv4addr [find where name=gre-tunne\
    l-selnau]\
    \n  /ip/ipsec/policy/set src-address=\"\$ipv4addr/32\" [find where peer=pa\
    nter-colab]\
    \n  /ip/ipsec/policy/set src-address=\"\$ipv4addr/32\" [find where peer=pa\
    nter-selnau]\
    \n  :global greold4addr \$ipv4addr\
    \n} else={\
    \n#  :log warning \"[gre update] No changes needed\"\
    \n}\
    \n"
/user group
add name=dashboard policy="read,write,test,winbox,web,api,rest-api,!local,!tel\
    net,!ssh,!ftp,!reboot,!policy,!password,!sniff,!sensitive,!romon"
add name=prometheus policy="read,api,!local,!telnet,!ssh,!ftp,!reboot,!write,!\
    policy,!test,!winbox,!password,!web,!sniff,!sensitive,!romon,!rest-api"
/caps-man manager
set enabled=yes package-path=/sd1-part1/upgrade upgrade-policy=\
    suggest-same-version
/caps-man provisioning
add action=create-dynamic-enabled comment="5 GHz AC" hw-supported-modes=ac \
    master-configuration=faesigrund-5ac name-format=prefix-identity \
    name-prefix=cap
add action=create-dynamic-enabled comment="5 GHz N" hw-supported-modes=an \
    master-configuration=faesigrund-5n name-format=prefix-identity \
    name-prefix=cap
add action=create-dynamic-enabled comment="2.4 GHz" hw-supported-modes=gn \
    master-configuration=faesigrund-24 name-format=prefix-identity \
    name-prefix=cap
/interface bridge port
add bridge=lan ingress-filtering=no interface=e01-ap-keller \
    internal-path-cost=10 path-cost=10
add bridge=lan ingress-filtering=no interface=e02 internal-path-cost=10 \
    path-cost=10
add bridge=lan ingress-filtering=no interface=e03 internal-path-cost=10 \
    path-cost=10
add bridge=lan ingress-filtering=no interface=e04 internal-path-cost=10 \
    path-cost=10
/interface list member
add interface=loopback list=discover
add interface=wlan-client list=discover
add interface=ovpn-tk list=discover
add interface=ovpn-wegmann list=discover
add interface=lan list=discover
add interface=*F01FE9 list=discover
add interface=e01-ap-keller list=discover
add interface=e05-bachmann-dachboden list=discover
add interface=fiber-uplink-crossover7 list=uplink
add interface=fiber-uplink-fiber7 list=uplink
/interface ovpn-server server
add auth=sha1,sha256,sha384,sha512 certificate=ca cipher=\
    blowfish128,aes128-cbc,aes192-cbc,aes256-cbc default-profile=\
    default-encryption disabled=no mac-address=FE:4F:9E:16:E6:4C name=\
    ovpn-server1
/ip address
add address=10.195.252.1/22 interface=wlan-client network=10.195.252.0
add address=10.0.38.2/30 interface=gre-colab network=10.0.38.0
add address=172.16.0.4 interface=lo network=172.16.0.4
add address=172.17.0.2/29 interface=e05-bachmann-dachboden network=172.17.0.0
add address=10.195.0.1/24 interface=lan network=10.195.0.0
add address=10.0.38.10/30 interface=gre-tunnel-selnau network=10.0.38.8
add address=172.16.1.4 interface=lo network=172.16.1.4
add address=10.195.17.1/24 interface=*35 network=10.195.17.0
add address=172.17.1.57/30 interface=gre-backup-link network=172.17.1.56
add address=172.17.1.61/30 interface=ipipv6-loch network=172.17.1.60
/ip cloud
set ddns-enabled=yes
/ip dhcp-client
add default-route-distance=3 default-route-tables=main disabled=yes \
    interface=fiber-uplink-crossover7 name=fiber-uplink-crossover7 script=":lo\
    g dhcp \$lease-address\
    \n:if (\$bound=1) do={/ip firewall address-list add address=\$lease-addres\
    s list=ingress} else={/ip firewall address-list remove address=\$lease-add\
    ress list=ingress}\
    \n/system script run ddns-update"
add default-route-distance=5 default-route-tables=main interface=\
    fiber-uplink-fiber7 name=fiber-uplink-fiber7
/ip dhcp-server network
add address=10.195.0.0/24 dns-server=10.195.0.1 gateway=10.195.0.1
add address=10.195.1.0/24 dns-server=10.195.1.1 gateway=10.195.1.1
add address=10.195.2.0/24 dns-server=10.195.2.1 gateway=10.195.2.1
add address=10.195.3.0/24 dns-server=10.195.3.1 gateway=10.195.3.1
add address=10.195.4.0/24 dns-server=10.195.4.1 gateway=10.195.4.1
add address=10.195.5.0/24 dns-server=10.195.5.1 gateway=10.195.5.1
add address=10.195.10.0/24 dns-server=172.16.0.1,213.144.129.20 gateway=\
    10.195.10.1
add address=10.195.252.0/22 dns-server=10.195.252.1 gateway=10.195.252.1
add address=172.17.0.0/29 dns-server=172.17.0.1 gateway=172.17.0.1
/ip dns
set allow-remote-requests=yes
/ip dns static
add address=fd58:9c23:3615:: name=station-.lan type=AAAA
add address=fd58:9c23:3615::60 name=station-60.lan type=AAAA
add address=fd58:9c23:3615::42 name=station-42.lan type=AAAA
add address=fd58:9c23:3615::61 name=station-61.lan type=AAAA
add address=fd7e:907d:34ab::60 name=station-60.lan type=AAAA
add address=fd7e:907d:34ab::6 name=station-6.lan type=AAAA
add address=fd7e:907d:34ab::7 name=station-7.lan type=AAAA
add address=fd7e:907d:34ab::10 name=station-10.lan type=AAAA
add address=fd7e:907d:34ab::26 name=station-26.lan type=AAAA
add address=fd7e:907d:34ab::30 name=station-30.lan type=AAAA
add address=fd7e:907d:34ab::37 name=station-37.lan type=AAAA
add address=fd7e:907d:34ab::41 name=station-41.lan type=AAAA
add address=fd7e:907d:34ab::44 name=station-44.lan type=AAAA
add address=172.31.88.0 disabled=yes name=ing.berg-turbenthal.ch ttl=15m \
    type=A
/ip firewall address-list
add address=10.192.0.0/12 list=internal
add address=172.16.0.0/12 list=internal
add address=192.168.0.0/16 list=internal
add address=82.197.188.0/24 comment="NOC RANGE" list=NOC-ACCESSv4
add address=213.144.129.128/26 comment="FIBER7 MGMT RANGE" list=NOC-ACCESSv4
add address=10.0.0.0/8 list=internal
/ip firewall filter
add action=fasttrack-connection chain=forward connection-state=\
    established,related
add action=accept chain=forward dst-port=443 in-interface-list=uplink \
    protocol=tcp
add action=accept chain=input dst-port=80 in-interface-list=uplink protocol=\
    tcp
add action=accept chain=input comment="allow WireGuard" dst-port=34338 \
    protocol=udp
add action=accept chain=input dst-address=224.0.0.1
add action=accept chain=input dst-port=500 protocol=udp
add action=accept chain=input ipsec-policy=in,ipsec
add action=accept chain=forward dst-address=239.44.0.0/24 in-interface-list=\
    uplink
add action=accept chain=forward connection-state=established
add action=accept chain=forward in-interface-list=uplink protocol=igmp
add action=accept chain=input in-interface-list=uplink log=yes protocol=igmp
add action=tarpit chain=input comment="drop ssh brute forcers" dst-port=22 \
    in-interface-list=uplink protocol=tcp src-address-list=ssh_blacklist
add action=add-src-to-address-list address-list=ssh_blacklist \
    address-list-timeout=10m chain=input connection-state=new dst-port=22 \
    protocol=tcp src-address-list=ssh_stage3
add action=add-src-to-address-list address-list=ssh_stage3 \
    address-list-timeout=1m chain=input connection-state=new dst-port=22 \
    protocol=tcp src-address-list=ssh_stage2
add action=add-src-to-address-list address-list=ssh_stage2 \
    address-list-timeout=1m chain=input connection-state=new dst-port=22 \
    protocol=tcp src-address-list=ssh_stage1
add action=add-src-to-address-list address-list=ssh_stage1 \
    address-list-timeout=1m chain=input connection-state=new dst-port=22 \
    protocol=tcp
add action=accept chain=forward connection-state=established
add action=accept chain=input protocol=igmp
add action=accept chain=input comment="ipsec input" ipsec-policy=in,ipsec
add action=accept chain=input protocol=ipsec-esp
add action=accept chain=forward comment="ipsec out" ipsec-policy=out,ipsec
add action=accept chain=forward connection-state=related
add action=accept chain=forward protocol=icmp
add action=accept chain=input connection-state=established
add action=accept chain=input connection-state=related
add action=accept chain=input protocol=icmp
add chain=input comment="permit remote access" src-address-list=NOC-ACCESSv4
add chain=input comment="permit bandwidth test inbound to local router" \
    protocol=udp src-port=64000-65535
add action=accept chain=input dst-port=22 protocol=tcp
add action=accept chain=input dst-port=1194 protocol=tcp
add action=accept chain=input dst-port=4500 protocol=udp
add action=accept chain=input dst-address=224.0.0.0/4 protocol=udp
add action=reject chain=forward dst-address-list=internal log=yes log-prefix=\
    "wrong out" out-interface-list=uplink reject-with=\
    icmp-network-unreachable
add action=reject chain=input in-interface-list=uplink log=yes log-prefix=\
    "final reject" reject-with=icmp-admin-prohibited
/ip firewall nat
add action=masquerade chain=srcnat disabled=yes out-interface-list=uplink \
    src-address=172.16.0.0/12
add action=masquerade chain=srcnat out-interface-list=uplink
add action=masquerade chain=srcnat disabled=yes out-interface-list=uplink \
    src-address=192.168.0.0/16
add action=dst-nat chain=dstnat disabled=yes dst-port=22 in-interface-list=\
    uplink protocol=tcp to-addresses=10.14.50.29 to-ports=22
add action=dst-nat chain=dstnat dst-port=80 in-interface-list=uplink \
    protocol=tcp to-addresses=172.31.88.3 to-ports=80
add action=dst-nat chain=dstnat disabled=yes dst-port=80 in-interface-list=\
    uplink protocol=tcp to-addresses=172.31.88.0 to-ports=80
add action=dst-nat chain=dstnat dst-port=443 in-interface-list=uplink \
    protocol=tcp to-addresses=172.31.88.3 to-ports=443
add action=dst-nat chain=dstnat disabled=yes dst-port=443 in-interface-list=\
    uplink protocol=tcp to-addresses=172.31.88.0 to-ports=443
add action=dst-nat chain=dstnat disabled=yes dst-address=77.109.174.22 \
    dst-port=443 protocol=tcp to-addresses=172.31.72.0 to-ports=443
add action=dst-nat chain=dstnat disabled=yes dst-port=6443 in-interface-list=\
    uplink protocol=tcp to-addresses=172.31.0.12 to-ports=6443
add action=dst-nat chain=dstnat disabled=yes dst-port=6444 in-interface-list=\
    uplink protocol=tcp to-addresses=172.31.0.18 to-ports=6443
add action=masquerade chain=srcnat dst-address=172.31.76.0/24 protocol=tcp \
    src-address=10.0.0.0/8
add action=masquerade chain=srcnat dst-address=172.31.76.0/24 protocol=tcp \
    src-address=172.16.0.0/12
/ip firewall service-port
set sip disabled=yes
/ip hotspot profile
set [ find default=yes ] html-directory=flash/hotspot
/ip ipsec identity
add peer=panter-selnau
add peer=panter-colab
add peer=dag-vpn
add peer=vpn-michaud
/ip ipsec policy
add dst-address=212.51.155.234/32 peer=panter-colab proposal=panter-vpn \
    src-address=85.195.193.66/32
add disabled=yes dst-address=178.238.174.80/32 peer=dag-vpn src-address=\
    0.0.0.0/0 tunnel=yes
add dst-address=213.160.32.164/32 peer=panter-selnau proposal=panter-selnau \
    src-address=77.109.174.57/32 tunnel=yes
add dst-address=2a02:16a:fe01::/48 peer=vpn-michaud src-address=\
    2a02:16a:fe00::/48 tunnel=yes
/ip service
set ftp disabled=yes
set telnet disabled=yes
/ip ssh
set ciphers=aes-gcm,aes-ctr,aes-cbc,3des-cbc forwarding-enabled=both
/ipv6 address
add address=fd7e:907d:34ab::4/128 advertise=no interface=lo
add address=fd7e:907d:34ab::fffe/128 advertise=no interface=loopback
add address=fd7e:907d:34ab::ff00/128 advertise=no interface=loopback
add disabled=yes from-pool=public-pool interface=e01-ap-keller
add address=fd7e:907d:34ab::400:0/127 advertise=no interface=gre-lkw
add address=2a02:16a:fe00:700:e68d:8cff:fef3:68fd eui-64=yes interface=\
    wlan-client
add address=2a02:16a:fe00::1/128 advertise=no interface=lo
add address=2a02:16a:fe00:ffff:ffff:ffff:ffff:ff00/128 advertise=no \
    interface=lo
add address=2a02:16a:fe00:ffff:ffff:ffff:ffff:ff04/128 advertise=no \
    interface=lo
/ipv6 dhcp-client
add add-default-route=yes default-route-distance=2 default-route-tables=main \
    disabled=yes interface=fiber-uplink-crossover7 pool-name=crossover-pool \
    pool-prefix-length=56 request=address,prefix
add add-default-route=yes default-route-tables=main interface=\
    fiber-uplink-fiber7 pool-name=fiber7-pool pool-prefix-length=56 request=\
    address,prefix
/ipv6 dhcp-server
add interface=loopback lease-time=10m name=server1 prefix-pool=public-pool
/ipv6 firewall address-list
add address=2001:1620:2777:10::/64 comment="NOC RANGE" list=NOC-ACCESSv6
add address=2001:1620:2777:11::/64 comment="FIBER7 MGMT RANGE" list=\
    NOC-ACCESSv6
add address=2a02:16a:fe00:1a0::10:1/128 list=http-server
add address=2a02:168:e000:1a0::10:0/128 list=http-server
add address=2a02:168:e000:603::10:0/128 list=http-server
/ipv6 firewall filter
add action=accept chain=forward connection-state=established,related
add action=accept chain=forward protocol=icmpv6
add action=accept chain=forward comment="http ingress" dst-address-list=\
    http-server dst-port=80,443 protocol=tcp
add action=accept chain=forward dst-address=2a02:168:e000:603::10:0/128 \
    dst-port=80,443 protocol=tcp
add action=accept chain=forward dst-address=2a02:16a:fe00::1/128
add action=accept chain=input connection-state=established
add action=accept chain=input connection-state=related
add action=accept chain=input protocol=icmpv6
add chain=input comment="permit NTP" protocol=udp src-port=123
add chain=input comment="permit remote access" src-address-list=NOC-ACCESSv6
add chain=input comment="allow DHCPv6 (UDP 546)" dst-port=546 protocol=udp
add action=drop chain=input comment="drop ssh brute forcers" dst-port=22 \
    in-interface=fiber-uplink-fiber7 protocol=tcp src-address-list=\
    ssh_blacklist
add action=add-src-to-address-list address-list=ssh_blacklist \
    address-list-timeout=10m chain=input connection-state=new dst-port=22 \
    in-interface=fiber-uplink-fiber7 protocol=tcp src-address-list=ssh_stage3
add action=add-src-to-address-list address-list=ssh_stage1 \
    address-list-timeout=1m chain=input connection-state=new dst-port=22 \
    in-interface=fiber-uplink-fiber7 protocol=tcp
add action=add-src-to-address-list address-list=ssh_stage2 \
    address-list-timeout=1m chain=input connection-state=new dst-port=22 \
    in-interface=fiber-uplink-fiber7 protocol=tcp src-address-list=ssh_stage1
add action=add-src-to-address-list address-list=ssh_stage3 \
    address-list-timeout=1m chain=input connection-state=new dst-port=22 \
    in-interface=fiber-uplink-fiber7 log=yes protocol=tcp src-address-list=\
    ssh_stage2
add action=accept chain=input comment=SSH dst-port=22 protocol=tcp
add action=drop chain=forward comment="drop ssh brute forcers" dst-port=22 \
    in-interface=fiber-uplink-fiber7 protocol=tcp src-address-list=\
    ssh_blacklist
add action=add-src-to-address-list address-list=ssh_blacklist \
    address-list-timeout=10m chain=forward connection-state=new dst-port=22 \
    in-interface=fiber-uplink-fiber7 protocol=tcp src-address-list=ssh_stage3
add action=add-src-to-address-list address-list=ssh_stage2 \
    address-list-timeout=1m chain=forward connection-state=new dst-port=22 \
    in-interface=fiber-uplink-fiber7 protocol=tcp src-address-list=ssh_stage1
add action=add-src-to-address-list address-list=ssh_stage3 \
    address-list-timeout=1m chain=forward connection-state=new dst-port=22 \
    in-interface=fiber-uplink-fiber7 protocol=tcp src-address-list=ssh_stage2
add action=add-src-to-address-list address-list=ssh_stage1 \
    address-list-timeout=1m chain=forward connection-state=new dst-port=22 \
    in-interface=fiber-uplink-fiber7 protocol=tcp
add action=accept chain=input dst-address=\
    2a02:16a:fe00:ffff:ffff:ffff:ffff:ff00/128 protocol=ipsec-esp
add action=accept chain=input dst-address=\
    2a02:16a:fe00:ffff:ffff:ffff:ffff:ff00/128 dst-port=500 protocol=udp
add action=reject chain=forward in-interface-list=uplink reject-with=\
    icmp-admin-prohibited
add action=reject chain=input in-interface-list=uplink reject-with=\
    icmp-admin-prohibited
add chain=input comment="allow DNS resolution from local router" protocol=udp \
    src-port=53
/ipv6 firewall mangle
add action=mark-routing chain=prerouting disabled=yes new-routing-mark=fiber7 \
    src-address=2a02:16a:fe00::/48
/ipv6 nd
set [ find default=yes ] disabled=yes
add advertise-dns=yes interface=wlan-client
/lcd
set time-interval=hour
/ppp secret
add local-address=10.195.16.1 name=ovpn-teamkoenig remote-address=10.195.16.4 \
    service=ovpn
add local-address=10.3.64.1 name=wegmann-ungarn profile=default-encryption
add local-address=10.195.16.1 name=ovpn-hefenhausen remote-address=\
    10.195.16.2 service=ovpn
add local-address=10.195.16.1 name=ovpn-styghalde remote-address=10.195.16.3 \
    service=ovpn
add local-address=10.195.16.1 name=ovpn-teamkoenig-2 profile=\
    default-encryption remote-address=10.195.16.5 service=ovpn
add local-address=10.195.16.1 name=ovpn-bm remote-address=10.195.16.6 \
    service=ovpn
add local-address=10.195.16.1 name=ovpn-teamkoenig-1 profile=\
    default-encryption remote-address=10.195.16.7 service=ovpn
add local-address=10.195.16.1 name=ovpn-teamkoenig-3 profile=\
    default-encryption remote-address=10.195.16.8 service=ovpn
add local-address=10.195.16.1 name=backup-link remote-address=10.195.16.9 \
    service=ovpn
add local-address=10.195.16.1 name=ovpn-michaud remote-address=10.195.16.10 \
    service=ovpn
/routing bfd configuration
add disabled=no interfaces=e05-bachmann-dachboden
/routing bgp connection
add afi=ip cisco-vpls-nlri-len-fmt=auto-bits disabled=no instance=\
    bgp-instance-2 local.address=10.0.38.2 .role=ebgp name=colab \
    output.filter-chain=bgp-colab .network=bgp-networks remote.address=\
    10.0.38.1 .as=65530 .port=179 routing-table=main templates=bgp-vpn-colab
add afi=ip cisco-vpls-nlri-len-fmt=auto-bits connect=yes disabled=no \
    input.filter=vpn-familie instance=bgp-instance-3 listen=yes \
    local.address=10.195.16.1 .role=ebgp name=hefenhausen \
    output.filter-chain=vpn-familie .no-client-to-client-reflection=yes \
    remote.address=10.195.16.2 .as=65538 .port=179 routing-table=main \
    templates=bgp-vpn-familie
add afi=ip cisco-vpls-nlri-len-fmt=auto-bits connect=yes disabled=no \
    input.filter=vpn-familie instance=bgp-instance-3 listen=yes \
    local.address=10.195.16.1 .role=ebgp name=styghalde output.filter-chain=\
    vpn-familie .no-client-to-client-reflection=no remote.address=10.195.16.3 \
    .as=65539 .port=179 routing-table=main templates=bgp-vpn-familie
add cisco-vpls-nlri-len-fmt=auto-bits connect=yes disabled=no input.filter=\
    vpn-familie instance=bgp-instance-3 listen=yes local.role=ebgp name=\
    teamkoenig output.filter-chain=vpn-familie remote.address=10.195.16.5 \
    .as=65540 .port=179 routing-table=main templates=bgp-vpn-familie
add afi=ip,ipv6 cisco-vpls-nlri-len-fmt=auto-bits connect=yes disabled=yes \
    input.filter=vpn-familie instance=bgp-instance-3 listen=yes \
    local.address=10.195.16.1 .role=ebgp name=bm-wil output.filter-chain=\
    vpn-familie .no-client-to-client-reflection=yes .redistribute=\
    connected,ospf remote.address=10.195.16.6 .as=64511 .port=179 \
    routing-table=main templates=bgp-vpn-familie
add afi=ip,ipv6 cisco-vpls-nlri-len-fmt=auto-bits connect=yes disabled=no \
    input.filter=vpn-familie instance=bgp-instance-3 listen=yes \
    local.address=10.195.16.1 .role=ebgp name=bm-wil output.filter-chain=\
    vpn-familie .no-client-to-client-reflection=yes remote.address=\
    10.195.16.6 .as=65541 .port=179 routing-table=main templates=\
    bgp-vpn-familie
add cisco-vpls-nlri-len-fmt=auto-bits disabled=no input.filter=vpn-familie \
    instance=bgp-instance-3 local.address=10.195.16.1 .role=ebgp name=\
    teamkoenig-lkw output.filter-chain=vpn-familie .redistribute=\
    connected,ospf,bgp remote.address=10.195.16.8 .as=65543 .port=179 \
    routing-table=main templates=bgp-vpn-familie
add afi=ipv6 cisco-vpls-nlri-len-fmt=auto-bits connect=yes disabled=no \
    input.filter=vpn-familie instance=bgp-instance-3 listen=yes \
    local.address=fe80::3:ac3:1001%6to4-tunnel-lkw .role=ebgp name=\
    teamkoenig-lkw-v6 output.default-originate=always .filter-chain=\
    vpn-familie .no-client-to-client-reflection=yes .redistribute=\
    connected,ospf,bgp remote.address=fe80::3:ac3:1008%6to4-tunnel-lkw .as=\
    65543 routing-table=main templates=bgp-vpn-familie
add cisco-vpls-nlri-len-fmt=auto-bits connect=yes disabled=no input.filter=\
    bgp-colab instance=bgp-instance-2 listen=yes local.address=10.0.38.10 \
    .role=ebgp name=selnau output.filter-chain=bgp-colab .network=\
    bgp-networks .no-client-to-client-reflection=yes remote.address=10.0.38.9 \
    .as=65570 .port=179 routing-table=main templates=bgp-vpn-colab
add afi=ip disabled=yes instance=bgp-instance-1 local.role=ebgp name=\
    bgp-dachboden-v4 output.default-originate=if-installed .filter-chain=\
    ospf-out .redistribute=connected,bgp remote.address=172.17.0.1 \
    routing-table=main templates=bgp-pirg use-bfd=yes
add afi=ipv6 disabled=yes instance=bgp-instance-1 local.address=\
    fe80::6e3b:6bff:fe1c:1511%e05-bachmann-dachboden .role=ebgp name=\
    bgp-dachboden-v6 output.default-originate=if-installed .filter-chain=\
    ospf-out .redistribute=connected,bgp,dhcp remote.address=\
    fe80::7a9a:18ff:fe5f:2fbe%e05-bachmann-dachboden .as=4200000001 \
    routing-table=main templates=bgp-pirg use-bfd=yes
add as=4200000000 disabled=no instance=bgp-instance-1 local.role=ebgp name=\
    backup-link output.redistribute=ospf,bgp remote.address=10.195.16.9 .as=\
    65580 routing-table=main vrf=main
add as=4200000000 disabled=no instance=bgp-instance-1 local.role=ebgp name=\
    bgp-loch output.filter-chain=vpn-familie .redistribute=connected,ospf,bgp \
    remote.address=172.17.1.62 .as=4200000016 routing-table=main templates=\
    bgp-vpn-familie vrf=main
/routing filter rule
add chain=bgp-colab disabled=no rule=\
    "if (dst in 10.0.38.0/24 && dst-len in 24-32) { reject; }"
add chain=bgp-colab disabled=no rule="if (dst == 10.192.6.0/24) { accept; }"
add chain=bgp-colab disabled=no rule=\
    "if (dst in 172.31.96.0/22 && dst-len in 22-32) { accept; }"
add chain=bgp-colab disabled=no rule=\
    "if (dst in 172.31.88.0/22  && dst-len in 22-32) { accept; }"
add chain=bgp-colab disabled=no rule="if (dst == 172.31.0.0/24) { accept; }"
add chain=bgp-colab disabled=no rule=\
    "if (dst in 10.0.0.0/9 && dst-len in 16-32) { accept; }"
add chain=bgp-colab comment="Switch for Development" disabled=no rule=\
    "if (dst in 10.192.65.0/24 && dst-len == 24) { accept; }"
add chain=bgp-colab disabled=no rule=\
    "if (dst in 100.0.0.0/8 && dst-len in 8-32) { accept; }"
add chain=bgp-colab disabled=no rule="reject;"
add chain=no-upc disabled=no rule="if (dst == 172.25.220.0/22) { reject; }"
add chain=no-upc disabled=no rule=\
    "if (dst in 62.2.16.0/20 && dst-len in 20-32) { reject; }"
add chain=no-upc disabled=no rule=accept
add chain=vpn-familie disabled=no rule=\
    "if (dst in 10.220.0.0/14 && dst-len in 14-30) { accept; }"
add chain=vpn-familie disabled=no rule=\
    "if (dst in 172.31.0.0/16 && dst-len in 16-32) { accept; }"
add chain=vpn-familie disabled=no rule=\
    "if (dst in 10.192.0.0/13 && dst-len in 13-32) { accept; }"
add chain=vpn-familie disabled=no rule=\
    "if (dst in 10.16.0.0/16 && dst-len in 13-32) { accept; }"
add chain=vpn-familie disabled=no rule=\
    "if (dst in 10.10.0.0/16 && dst-len in 13-32) { accept; }"
add chain=vpn-familie disabled=no rule=\
    "if (dst in 172.16.32.0/19 && dst-len in 19-32) { accept; }"
add chain=vpn-familie disabled=yes rule=\
    "if (dst in 10.10.252.0/22 && dst-len in 22-32) { accept; }"
add chain=vpn-familie disabled=no rule=\
    "if (dst in 172.16.2.0/24 && dst-len in 24-32) { accept; }"
add chain=vpn-familie disabled=no rule=\
    "if (dst in 172.16.0.0/23 && dst-len == 32) { accept; }"
add chain=vpn-familie disabled=no rule=\
    "if (dst in 10.216.0.0/16 && dst-len in 24-32) { accept; }"
add chain=vpn-familie disabled=no rule=\
    "if (dst in 2a02:16a:fe00::/48 && dst-len in 0-128) { accept; }"
add chain=vpn-familie disabled=no rule=\
    "if (dst in fd7e:907d:34ab::/64 && dst-len in 0-128) { accept; }"
add chain=vpn-familie disabled=no rule=\
    "if (dst in ::/0 && dst-len == 0) { accept; }"
add chain=vpn-familie disabled=no rule="reject;"
add chain=ospf-out comment="join colab routes" disabled=no rule=\
    "if (dst in 10.0.0.0/12 && dst-len in 15-32) { reject; }"
add chain=ospf-out disabled=no rule=\
    "if (dst == 2a02:16a:fe00::/48) { reject; }"
add chain=ospf-out disabled=no rule=accept
/routing ospf interface-template
add area=backbone-v2 disabled=no interfaces=e05-bachmann-dachboden
add area=backbone-v3 disabled=no interfaces=e05-bachmann-dachboden
add area=backbone-v2 cost=50 disabled=no interfaces=gre-backup-link type=\
    ptp-unnumbered
add area=backbone-v3 cost=50 disabled=no interfaces=gre-backup-link type=ptp
/system clock
set time-zone-name=Europe/Zurich
/system identity
set name=router-faesigrund-keller
/system logging
add disabled=yes prefix=dhcp topics=dhcp
add disabled=yes topics=ovpn
add disabled=yes topics=ppp
add disabled=yes topics=dhcp
add disabled=yes topics=dns
add disabled=yes topics=ipsec,!debug
add disabled=yes prefix=bgp topics=bgp
add action=remote topics=info
add action=remote topics=warning
add action=remote topics=error
add action=remote topics=critical
/system ntp client
set enabled=yes
/system ntp client servers
add address=pool.ntp.org
/system package update
set channel=long-term
/system routerboard settings
set auto-upgrade=yes
/system scheduler
add interval=17m23s name=cfg-update on-event="/system/script/run cfg-update" \
    policy=read,write start-date=2024-05-05 start-time=05:06:25
/system watchdog
set ping-start-after-boot=30m watchdog-timer=no
/tool e-mail
set certificate-verification=no from=\
    Router-Faesigrund<router-faesigrund@berg-turbenthal.ch> server=\
    aspmx.l.google.com
/tool netwatch
add disabled=no down-script=\
    "/ip dhcp-client release [find where interface=fiber-uplink]" host=\
    77.109.174.1 interval=1m test-script="" timeout=1s type=simple up-script=\
    ""
add disabled=yes down-script=\
    "/ipv6 dhcp-client release [find where interface=fiber-uplink]" host=\
    2001:1620:2777:23::2 interval=1m timeout=1s type=simple
add disabled=no host=2001:1620:1700:1101::7 interval=1m timeout=10s type=\
    simple
add disabled=no down-script="tool e-mail send to=andreas.koenig@berg-turbentha\
    l.ch subject=\"bachmann-dachboden down\"" host=172.16.0.5 interval=1m \
    timeout=10s type=simple up-script="tool e-mail send to=andreas.koenig@berg\
    -turbenthal.ch subject=\"bachmann-dachboden up\""
add disabled=no down-script="tool e-mail send to=andreas.koenig@berg-turbentha\
    l.ch subject=\"hegglin down\"" host=172.16.1.2 interval=1m test-script="" \
    timeout=10s type=simple up-script="tool e-mail send to=andreas.koenig@berg\
    -turbenthal.ch subject=\"hegglin up\""
add disabled=no down-script="tool e-mail send to=andreas.koenig@berg-turbentha\
    l.ch subject=\"koenig down\"" host=172.16.1.1 interval=1m test-script="" \
    timeout=10s type=simple up-script="tool e-mail send to=andreas.koenig@berg\
    -turbenthal.ch subject=\"koenig up\""
add disabled=no down-script="tool e-mail send to=andreas.koenig@berg-turbentha\
    l.ch subject=\"wurm down\"" host=172.16.0.6 interval=1m timeout=10s type=\
    simple up-script="tool e-mail send to=andreas.koenig@berg-turbenthal.ch su\
    bject=\"wurm up\""
add disabled=no down-script="tool e-mail send to=andreas.koenig@berg-turbentha\
    l.ch subject=\"burri down\"" host=172.16.0.7 interval=1m timeout=10s \
    type=simple up-script="tool e-mail send to=andreas.koenig@berg-turbenthal.\
    ch subject=\"burri up\""
add disabled=no down-script="tool e-mail send to=andreas.koenig@berg-turbentha\
    l.ch subject=\"schopf down\"" host=172.16.1.8 interval=1m test-script="" \
    timeout=10s type=simple up-script="tool e-mail send to=andreas.koenig@berg\
    -turbenthal.ch subject=\"schopf up\""
add disabled=no down-script="tool e-mail send to=andreas.koenig@berg-turbentha\
    l.ch subject=\"walter down\"" host=172.16.1.9 interval=1m test-script="" \
    timeout=10s type=simple up-script="tool e-mail send to=andreas.koenig@berg\
    -turbenthal.ch subject=\"walter up\""
add disabled=no down-script="/ip/dhcp-client/release fiber-uplink" host=\
    8.8.8.8 name="Google Internet" test-script="" type=simple up-script=""
/tool romon
set enabled=yes
