# 2026-10-01 15:33:50 by RouterOS 7.23.5
# software id = A6RM-B482
#
# model = CCR1009-7G-1C-1S+
# serial number = 6E130447C663
/caps-man channel
add band=2ghz-onlyn extension-channel=disabled frequency=2412 name=24-001
add band=2ghz-onlyn extension-channel=disabled frequency=2432 name=24-005
add band=2ghz-onlyn extension-channel=disabled frequency=2452 name=24-009
add band=2ghz-onlyn extension-channel=disabled frequency=2472 name=24-013
add band=5ghz-onlyn extension-channel=Ce frequency=5180 name=5-036
add band=5ghz-onlyn extension-channel=Ce frequency=5220 name=5-044
add band=5ghz-onlyn extension-channel=Ce frequency=5260 name=5-052
add band=5ghz-onlyn extension-channel=Ce frequency=5300 name=5-060
add band=5ghz-onlyac extension-channel=Ceee frequency=5500 name=5-100
add band=5ghz-onlyac extension-channel=Ceee frequency=5580 name=5-116
add band=2ghz-onlyn extension-channel=disabled frequency=2412,2432,2452,2472 \
    name=24-autoselect reselect-interval=30m tx-power=16
add band=5ghz-onlyn extension-channel=XX frequency=5180,5220,5260,5300 name=\
    5n-autoselect reselect-interval=32m save-selected=no tx-power=13
add band=5ghz-n/ac extension-channel=XXXX frequency=5500,5580 name=\
    5ac-autoselect reselect-interval=31m save-selected=no tx-power=13
/interface bridge
add igmp-snooping=yes name=berg-lan port-cost-mode=short protocol-mode=none
add name=berg-solar protocol-mode=none
add disabled=yes name=berg-srv port-cost-mode=short
add disabled=yes name=berg-storage port-cost-mode=short
add fast-forward=no igmp-snooping=yes name=berg-wlan port-cost-mode=short \
    protocol-mode=none
add fast-forward=no igmp-snooping=yes name=berg-wlan-free port-cost-mode=\
    short protocol-mode=none
add name=berg-wlan-gerhard port-cost-mode=short protocol-mode=none
add igmp-snooping=yes name=berg-wlan-koa port-cost-mode=short protocol-mode=\
    none
add name=bridge-caps protocol-mode=none vlan-filtering=yes
add name=loopback port-cost-mode=short protocol-mode=none
add disabled=yes fast-forward=no name=trunk port-cost-mode=short
add name=wlan-mimi port-cost-mode=short protocol-mode=none
/interface ethernet
set [ find default-name=combo1 ] name=c01-keller
set [ find default-name=ether1 ] name=e01
set [ find default-name=ether2 ] name=e02
set [ find default-name=ether3 ] name=e03
set [ find default-name=ether4 ] name=e04
set [ find default-name=ether5 ] name=e05-wurm
set [ find default-name=ether6 ] disabled=yes name=e06-burri
set [ find default-name=ether7 ] advertise="10M-baseT-half,10M-baseT-full,100M\
    -baseT-half,100M-baseT-full,1G-baseT-half,1G-baseT-full" name=e07-hegglin
set [ find default-name=sfp-sfpplus1 ] name=s01
/interface ovpn-client
add auth=md5 cipher=aes128-cbc connect-to=\
    router-faesigrund.berg-turbenthal.ch mac-address=02:43:0F:F0:5D:88 name=\
    ovpn-backup-link user=backup-link
/interface eoipv6
add local-address=fd7e:907d:34ab::ffff mac-address=FE:E0:C7:B4:8F:BD name=\
    eoipv6-tunnel2 remote-address=fd7e:907d:34ab::2 tunnel-id=2
add local-address=fd7e:907d:34ab::ffff mac-address=FE:F4:AD:8F:64:D1 name=\
    eoipv6-tunnel4 remote-address=fd7e:907d:34ab::4 tunnel-id=4
/interface gre
add !keepalive name=gre-backup-link remote-address=10.195.16.1
/interface vxlan
add dont-fragment=disabled local-address=172.16.1.1 mac-address=\
    A6:F4:9C:07:0B:BC name=vxlan-client-berg port=8472 vni=1
/interface vlan
add interface=bridge-caps name=caps-vlan1-koa vlan-id=1
add interface=bridge-caps name=caps-vlan2-berg vlan-id=2
add interface=bridge-caps name=caps-vlan3-gerhard vlan-id=3
add interface=bridge-caps name=caps-vlan4-mimi vlan-id=4
add interface=bridge-caps name=caps-vlan11-free vlan-id=11
add interface=bridge-caps name=caps-vlan99-mgmt vlan-id=99
add interface=trunk name=vlan1-lan vlan-id=1
add interface=trunk name=vlan2-srv vlan-id=2
add interface=trunk name=vlan3-storage vlan-id=3
add interface=trunk name=vlan4-tinkerforge vlan-id=4
add interface=trunk name=vlan5-mgmt vlan-id=5
add interface=trunk name=vlan6-dsk-koa vlan-id=6
add interface=trunk name=vlan7-solar vlan-id=7
add interface=trunk name=vlan8-gerhard vlan-id=8
add interface=trunk name=vlan9-wlan-koa vlan-id=9
add interface=trunk name=vlan10-mimi vlan-id=10
add interface=trunk name=vlan11-free vlan-id=11
/interface bonding
add disabled=yes name=bond-ap-ax2 slaves=e03,e01,e02
/caps-man configuration
add datapath.bridge=berg-wlan-free .client-to-client-forwarding=yes name=\
    internet-free ssid=#CHFreeWiFi
/caps-man datapath
add bridge=berg-wlan-koa client-to-client-forwarding=yes name=berg-wlan-koa
add bridge=berg-wlan client-to-client-forwarding=yes name=berg-wlan
add bridge=wlan-mimi client-to-client-forwarding=yes name=mimi
add bridge=berg-wlan-gerhard client-to-client-forwarding=yes name=\
    berg-lan-gerhard
add bridge=berg-solar client-to-client-forwarding=yes name=berg-solar
/caps-man security
add authentication-types=wpa2-psk name=berg
add authentication-types=wpa2-psk name=mimi-security
add authentication-types=wpa2-psk encryption=aes-ccm,tkip name=\
    gerhard-security
add authentication-types=wpa2-psk name=berg-solar
/caps-man configuration
add channel=24-autoselect country=switzerland datapath=berg-wlan name=base-24 \
    security=berg ssid=berg
add channel=5n-autoselect country=switzerland datapath=berg-wlan name=base-5n \
    security=berg ssid=berg
add channel=5ac-autoselect country=switzerland datapath=berg-wlan name=\
    base-5ac security=berg ssid=berg
add datapath=berg-wlan-koa name=wlan-koa security=berg ssid=koa
add datapath=mimi name=wlan-mimi security=mimi-security ssid=mimi
add datapath=berg-lan-gerhard mode=ap name=gerhard-wlan security=\
    gerhard-security ssid=gerhard
add datapath=berg-solar mode=ap name=berg-solar security=berg-solar ssid=\
    berg-solar
/disk
add parent=sd1 partition-number=1 partition-offset=512 partition-size=\
    31914982912 type=partition
/interface list
add name=storage
add name=insecure
/interface lte apn
set [ find default=yes ] ip-type=ipv4 use-network-apn=no
/interface wifi datapath
add bridge=bridge-caps disabled=no name=berg-wlan vlan-id=2
add bridge=bridge-caps disabled=no name=berg-lan-gerhard vlan-id=3
add bridge=bridge-caps disabled=no name=berg-solar vlan-id=7
add bridge=bridge-caps disabled=no name=berg-wlan-koa vlan-id=1
add bridge=bridge-caps disabled=no name=berg-mimi vlan-id=4
add bridge=berg-wlan-free disabled=no name=berg-free vlan-id=11
/interface wifi interworking
add disabled=no hotspot20=yes hotspot20-dgaf=yes internet=yes \
    ipv4-availability=single-nated ipv6-availability=available name=internet \
    wan-downlink=100000 wan-status=up wan-symmetric=yes wan-uplink=100000
/interface wifi security
add authentication-types=wpa2-psk,wpa3-psk connect-priority=0 disabled=no ft=\
    yes name=berg
add authentication-types=wpa2-psk,wpa3-psk connect-priority=0 disabled=no \
    name=berg-solar
add authentication-types=wpa2-psk,wpa3-psk connect-priority=0 disabled=no ft=\
    yes name=gerhard-security
add authentication-types=wpa2-psk,wpa3-psk connect-priority=0 disabled=no ft=\
    yes name=mimi-security
add authentication-types=owe disabled=no ft=yes name=free
/interface wifi configuration
add country=Switzerland disabled=no interworking.hessid=4C:5E:0C:17:EC:15 \
    .network-type=private mode=ap name=base security=berg \
    security.connect-priority=0 .ft=yes ssid=berg
add datapath=berg-free disabled=no interworking=internet interworking.hessid=\
    4C:5E:0C:17:EC:19 .network-type=public-free name=internet-free security=\
    free security.connect-priority=0 .ft=yes ssid=#CHFreeWiFi
add datapath=berg-lan-gerhard disabled=no interworking=internet \
    interworking.hessid=4C:5E:0C:17:EC:18 .network-type=private name=\
    gerhard-wlan security=gerhard-security security.connect-priority=0 .ft=\
    yes ssid=gerhard
add datapath=berg-mimi disabled=no interworking=internet interworking.hessid=\
    4C:5E:0C:17:EC:17 .network-type=private name=berg-mimi security=\
    mimi-security security.connect-priority=0 .ft=yes ssid=mimi
/interface wifi steering
add disabled=no name=roam-free rrm=yes wnm=yes
/interface wifi configuration
add datapath=berg-solar disabled=no name=berg-solar security=berg-solar \
    security.connect-priority=0 .ft=yes ssid=berg-solar steering=roam-free
add datapath=berg-wlan-koa disabled=no interworking=internet \
    interworking.hessid=4C:5E:0C:17:EC:16 .network-type=private name=berg-koa \
    security=berg security.connect-priority=0 .ft=yes ssid=koa steering=\
    roam-free
/interface wireless security-profiles
set [ find default=yes ] supplicant-identity=MikroTik
/ip dhcp-server option
add code=224 name=ipxe-desktop value="s'http://10.14.60.249:8080/ipxe'"
add code=224 name=ipxe-sr01 value=\
    "s'http://172.31.0.10:8081/ipxe\?mac=\${net0/mac}'"
add code=66 name=option1 value="s'172.16.0.1'"
/ip hotspot profile
set [ find default=yes ] html-directory=flash/hotspot
add dns-name=hotspot.berg-turbenthal.ch hotspot-address=10.1.1.1 \
    html-directory=flash/storage/hotspot login-by=\
    mac,http-chap,https,http-pap,trial mac-auth-mode=\
    mac-as-username-and-password name=hsprof1 trial-uptime-limit=1d
/ip pool
add name=e08-pool ranges=10.192.8.10-10.192.8.254
add name=wlan-pool ranges=10.192.0.10-10.192.0.254
add name=srv-pool ranges=172.31.0.110-172.31.0.130
add name=wlan_free_pool ranges=10.192.1.2-10.192.1.254
add name=storage-pool ranges=172.31.1.100-172.31.1.254
add name=koa-pool ranges=10.192.2.10-10.192.2.254
add name=e07-pool ranges=10.192.7.100-10.192.7.254
add name=tinkerforge-pool ranges=10.192.64.100-10.192.64.254
add name=mgmt-pool ranges=10.192.65.240-10.192.65.254
add name=e05-pool ranges=10.192.5.2-10.192.5.254
add name=vlan6-pool ranges=10.192.6.10-10.192.6.20
add name=dhcp-mimi ranges=10.192.3.100-10.192.3.254
add name=e07-hegglin-pool ranges=172.17.0.11-172.17.0.12
add name=e05-wurm-pool ranges=172.17.0.35-172.17.0.36
add name=dhcp-e01 ranges=10.192.66.2-10.192.66.254
add name=lan-pool ranges=10.192.7.100-10.192.7.200
add name=dhcp_pool-bucher ranges=10.192.8.100-10.192.8.254
add name=dhcp_solar ranges=10.192.67.100-10.192.67.254
add name=dhcp_pool20 ranges=10.192.69.2
/ip dhcp-server
add address-pool=lan-pool authoritative=after-2sec-delay bootp-support=\
    dynamic disabled=yes interface=berg-lan lease-time=10m name=dhcp-lan
add address-pool=wlan-pool authoritative=after-2sec-delay interface=berg-wlan \
    lease-time=10m name=dhcp-wlan
# Interface not running
add address-pool=srv-pool bootp-support=dynamic interface=berg-srv \
    lease-time=10m name=dhcp-srv
add address-pool=wlan_free_pool authoritative=after-2sec-delay interface=\
    berg-wlan-free lease-time=10m name=dhcp-wlan-free
add address-pool=storage-pool disabled=yes interface=berg-storage lease-time=\
    10m name=dhcp-storage
add address-pool=koa-pool interface=berg-wlan-koa lease-time=10m name=\
    dhcp-koa
add address-pool=tinkerforge-pool disabled=yes interface=vlan4-tinkerforge \
    lease-time=10m name=dhcp-vlan4-tinkerforge
add address-pool=mgmt-pool bootp-lease-time=lease-time bootp-support=dynamic \
    disabled=yes interface=vlan5-mgmt lease-time=10m name=dhcp-mgmt
add address-pool=vlan6-pool disabled=yes interface=vlan6-dsk-koa lease-time=\
    10m name=dhcp-vlan6-dsk
add address-pool=dhcp-mimi interface=wlan-mimi lease-time=10m name=dhcp-mimi
# Interface not running
add address-pool=e07-hegglin-pool interface=e07-hegglin lease-time=10m name=\
    dhcp-e07-hegglin-link
add address-pool=e05-wurm-pool interface=e05-wurm lease-time=10m name=\
    dhcp-e05-wurm-link
add address-pool=dhcp_pool-bucher interface=berg-wlan-gerhard lease-time=10m \
    name=dhcp-gerhard
add address-pool=dhcp_solar interface=berg-solar name=dhcp-solar
add address-pool=dhcp_pool20 interface=e01 name=dhcp-ap-buero
/ip smb users
set [ find default=yes ] disabled=yes
/ip vrf
add disabled=yes interfaces=storage name=storage_
/ipv6 pool
add name=public-pool prefix=2a02:168:e000:100::/56 prefix-length=64
/port
set 0 baud-rate=auto
set 1 baud-rate=auto
/routing bgp instance
add as=65580 disabled=no name=bgp-backup-link
/routing bgp template
set default as=4200000005 disabled=no multihop=yes output.default-originate=\
    if-installed .redistribute=connected routing-table=main use-bfd=no
/routing id
add disabled=no id=172.16.1.1 name=id select-dynamic-id=""
/routing ospf instance
add disabled=no name=default-v2 originate-default=if-installed redistribute=\
    connected,static router-id=172.16.0.1
add disabled=no name=default-v3 originate-default=never redistribute=\
    connected,static,bgp,vpn,dhcp,modem router-id=172.16.0.1 version=3
/routing ospf area
add disabled=no instance=default-v2 name=backbone-v2
add disabled=no instance=default-v3 name=backbone-v3
/snmp community
set [ find default=yes ] addresses=172.16.0.0/12,10.192.0.0/16 name=\
    EKY9CRhXrEg4jbSbTm9x security=authorized
add addresses=10.192.0.0/16 name=public
/system logging action
set 1 disk-file-name=flash/log
set 3 cef-event-delimiter="" remote=2a02:16a:fe00:1a0::10:8 \
    remote-log-format=cef syslog-time-format=iso8601
/system script
add dont-require-permissions=no name=v4-routing owner=admin policy=read,write \
    source="  :foreach interf in=[/routing ospf-v3 interface find where passiv\
    e=no] do={\
    \n    :local hasOtherMaster 0\
    \n    :local hasClient 0\
    \n    :local interfName [/routing ospf-v3 interface get value-name=interfa\
    ce \$interf]\
    \n    #:put \$interfName\
    \n    :foreach neighbor in=[/routing ospf-v3 neighbor find where interface\
    =\$interfName] do={\
    \n      :local neighborId [/routing ospf-v3 neighbor get value-name=router\
    -id \$neighbor]\
    \n      :local myId [/routing ospf-v3 instance get value-name=router-id [/\
    routing ospf-v3 neighbor get value-name=instance \$neighbor]]\
    \n      :if (\$myId>\$neighborId) do={:set \$hasOtherMaster 1}\
    \n      :if (\$myId<\$neighborId) do={:set \$hasClient 1}\
    \n    }\
    \n    :if (\$hasOtherMaster > 0) do={\
    \n      # ensure dhcp-client is activated and network is in ospf range\
    \n      #:put (\"Go client: \".\$interfName)\
    \n      :if ([:len [/ip dhcp-client find where interface=\$interfName]] < \
    1) do={\
    \n        /ip dhcp-client add add-default-route=no dhcp-options=hostname,c\
    lientid disabled=no interface=\$interfName use-peer-dns=no use-peer-ntp=no\
    \n      } \
    \n      /ip address set disabled=yes [/ip address find where dynamic=no di\
    sabled=no interface=\$interfName]\
    \n      /ip dhcp-server set disabled=yes [/ip dhcp-server find where disab\
    led=no interface=\$interfName]\
    \n      /ip dhcp-client set disabled=no [/ip dhcp-client find where disabl\
    ed=yes interface=\$interfName]\
    \n      :local localIpAddress [/ip address find where dynamic=yes interfac\
    e=\$interfName]\
    \n      :if ([:len \$localIpAddress]=1) do={\
    \n           :local network [/ip address get value-name=network \$localIpA\
    ddress]\
    \n      :local ipAddress [/ip address get value-name=address \$localIpAddr\
    ess]\
    \n      :local slashPos [:find \$ipAddress \"/\"]\
    \n      #:put \$network\
    \n      #:put \$ipAddress\
    \n      :local ospfNet (\$network.[:pick \$ipAddress \$slashPos [:len \$ip\
    Address]])\
    \n      #:put \$ospfNet\
    \n      :if ([:len [/routing ospf network find where network=\$ospfNet]]<1\
    ) do={\
    \n/routing ospf network add network=\$ospfNet disabled=no area=backbone\
    \n      }\
    \n} else={\
    \n/ip dhcp-client release [find where interface=\$interfName]\
    \n}\
    \n   } else={\
    \n      # ensure dhcp-sever is activated and network is in ospf range\
    \n      #:put (\"Go server: \".\$interfName)\
    \n      :if ([:len [/ip dhcp-client find where add-default-route=yes ]]<1)\
    \_do={\
    \n        /ip address set disabled=no [/ip address find where disabled=yes\
    \_dynamic=no interface=\$interfName]\
    \n        /ip dhcp-server set disabled=no [/ip dhcp-server find where disa\
    bled=yes interface=\$interfName]\
    \n        /ip dhcp-client set disabled=yes [/ip dhcp-client find where dis\
    abled=no interface=\$interfName]\
    \n        :if (\$hasClient > 0) do={\
    \n          :local network [/ip address get value-name=network   [/ip addr\
    ess find where dynamic=no interface=\$interfName]] \
    \n          :local ipAddress [/ip address get value-name=address  [/ip add\
    ress find where dynamic=no interface=\$interfName]]  \
    \n          :local slashPos [:find \$ipAddress \"/\"]\
    \n          #:put \$network\
    \n          #:put \$ipAddress\
    \n          :local ospfNet (\$network.[:pick \$ipAddress \$slashPos [:len \
    \$ipAddress]])\
    \n          #:put \$ospfNet\
    \n          :if ([:len [/routing ospf network find where network=\$ospfNet\
    ]]<1) do={\
    \n            /routing ospf network add network=\$ospfNet disabled=no area\
    =backbone\
    \n          }\
    \n        }\
    \n      }\
    \n    }\
    \n  }"
add dont-require-permissions=no name=check-master owner=admin policy=\
    ftp,reboot,read,write,policy,test,password,sniff,sensitive source="{\
    \n    :global loopbackip [/ip address get value-name=address [/ip address \
    find where interface=loopback]]\
    \n    :global number ([:toip [:pick \$loopbackip 0 [:find \$loopbackip \"/\
    \"]]]-172.16.0.0)\
    \n    :global v6prefix \"fd7e:907d:34ab\"\
    \n    :local masterCount 0\
    \n    :if (\$number>0) do={\
    \n        :for master from=0 to=(\$number-1) step=1 do={\
    \n            :local masterIP (\$v6prefix.\"::\".\$master.\"/128\")\
    \n            #:put \$master\
    \n            :set \$masterCount (\$masterCount+[:len [/ipv6 route find wh\
    ere dst-address=\$masterIP]])\
    \n        }\
    \n    }\
    \n    :local isMaster 0\
    \n    :foreach addr in=[/ipv6 address find where interface=loopback] do={\
    \n        :if ([/ipv6 address get \$addr address]=(\$v6prefix.\"::ffff/128\
    \")) do={ :set \$isMaster \$addr }\
    \n    }\
    \n    :if (\$masterCount > 0) do={\
    \n        #/ip dns set servers=(\$v6prefix.\"::fffe\")\
    \n        /ip address remove [find where interface~\"^gre6-tunnel\"]\
    \n        #/ipv6 dhcp-client remove [find where interface=gre6-master-tunn\
    el]\
    \n        /interface gre6 remove [find where name~\"^gre6-tunnel\"]\
    \n        /interface eoipv6 remove [find where name~\"^eoipv6-tunnel\"]\
    \n        :if (\$isMaster!=0) do={/ipv6 address remove \$isMaster}\
    \n        :foreach addr in=[/ipv6 address find where interface=loopback] d\
    o={\
    \n            :if ([/ipv6 address get \$addr address]=(\$v6prefix.\"::ffff\
    /128\")) do={\
    \n                /ipv6 address remove \$addr\
    \n            }\
    \n        }\
    \n        :if ([:len [/interface eoipv6 find where name=\"eoipv6-master-tu\
    nnel\"]]=0) do={\
    \n            /interface gre6 add local-address=(\$v6prefix.\"::\".\$numbe\
    r) remote-address=(\$v6prefix.\"::ffff\") name=\"gre6-master-tunnel\"\
    \n            /interface eoipv6 add local-address=(\$v6prefix.\"::\".\$num\
    ber) remote-address=(\$v6prefix.\"::ffff\") name=\"eoipv6-master-tunnel\" \
    tunnel-id=\$number\
    \n            /ip address remove [find address=((172.16.1.2+(\$number-1)*4\
    ).\"/30\")]\
    \n            #/ip address add address=((172.16.1.2+(\$number-1)*4).\"/30\
    \") interface=gre6-master-tunnel\
    \n            #/routing ospf network remove [find network=((172.16.1.0+(\$\
    number-1)*4).\"/30\")]\
    \n            #/routing ospf network add area=backbone network=((172.16.1.\
    0+(\$number-1)*4).\"/30\")\
    \n            /interface wireless cap set caps-man-addresses=\"\" discover\
    y-interfaces=eoipv6-master-tunnel enabled=yes interfaces=[/interface wirel\
    ess find where interface-type!=virtual] certificate=none\
    \n            /ipv6 dhcp-client add interface=gre6-master-tunnel pool-name\
    =local-v6-pool pool-prefix-length=60 use-peer-dns=no\
    \n        }\
    \n        /ipv6 address set from-pool=local-v6-pool [/ipv6 address find fr\
    om-pool=public-pool]\
    \n        /caps-man manager set enabled=no\
    \n    } else={\
    \n        /ip address remove [find address=((172.16.1.2+(\$number-1)*4).\"\
    /30\")]\
    \n        #/routing ospf network remove [find network=((172.16.1.0+(\$numb\
    er-1)*4).\"/30\")]\
    \n        /interface gre6 remove [find where name=\"gre6-master-tunnel\"]\
    \n        /interface eoipv6 remove [find where name=\"eoipv6-master-tunnel\
    \"]\
    \n        :if (\$isMaster=0) do={\
    \n            /ipv6 address add address=(\$v6prefix.\"::ffff/128\") interf\
    ace=loopback\
    \n            /caps-man radio provision [find where !interface]\
    \n            /caps-man manager set enabled=yes\
    \n            /interface wireless cap set discovery-interfaces=loopback\
    \n        }\
    \n        :for tunnel from=0 to=100 step=1 do={\
    \n            #:put (\"Tunnel: \".\$tunnel)\
    \n            :if (\$number != \$tunnel) do={\
    \n                :local tunnelPrefix (\$v6prefix.\"::\".\$tunnel.\"/128\"\
    )\
    \n                :local tunnelIP (\$v6prefix.\"::\".\$tunnel)\
    \n                #:put (\"Check : \".\$tunnelIP)\
    \n                :if ([:len [/ipv6 route find where dst-address=\$tunnelP\
    refix]]>0) do={\
    \n                    #:put (\"Found: \".\$tunnel)\
    \n    \
    \n                    :if ([:len [/ip dns static find where name=(\"statio\
    n-\".\$tunnel.\".lan\")]] = 0) do={\
    \n                      /ip dns static add address=\$tunnelIP name=(\"stat\
    ion-\".\$tunnel.\".lan\")\
    \n                    }\
    \n                    :if ([:len [/interface gre6 find remote-address=\$tu\
    nnelIP]]=0) do={\
    \n                      /interface gre6 add local-address=(\$v6prefix.\"::\
    ffff\") remote-address=\$tunnelIP name=(\"gre6-tunnel\".\$tunnel)\
    \n                      #/ip address remove [find address=((172.16.1.1+(\$\
    tunnel-1)*4).\"/30\")]\
    \n                      #/ip address add address=((172.16.1.1+(\$tunnel-1)\
    *4).\"/30\") interface=(\"gre6-tunnel\".\$tunnel)\
    \n                      #/routing ospf network remove [find network=((172.\
    16.1.0+(\$tunnel-1)*4).\"/30\")]\
    \n                      #/routing ospf network add area=backbone network=(\
    (172.16.1.0+(\$tunnel-1)*4).\"/30\")\
    \n                    }\
    \n                    :if ([:len [/interface eoipv6 find remote-address=\$\
    tunnelIP]]=0) do={\
    \n                      /interface eoipv6 add local-address=(\$v6prefix.\"\
    ::ffff\") remote-address=\$tunnelIP name=(\"eoipv6-tunnel\".\$tunnel) tunn\
    el-id=\$tunnel\
    \n                    }\
    \n                }\
    \n            }\
    \n        }\
    \n    }\
    \n  :foreach interf in=[/interface ethernet find master-port=none] do={\
    \n    :local hasOtherMaster 0\
    \n    :local hasClient 0\
    \n    :local interfName [/interface ethernet get value-name=name \$interf]\
    \n    :foreach neighbor in=[/routing ospf-v3 neighbor find where interface\
    =\$interfName] do={\
    \n      :local neighborId [/routing ospf-v3 neighbor get value-name=router\
    -id \$neighbor]\
    \n      :local myId [/routing ospf-v3 instance get value-name=router-id [/\
    routing ospf-v3 neighbor get value-name=instance \$neighbor]]\
    \n      :if (\$myId>\$neighborId) do={:set \$hasOtherMaster 1}\
    \n      :if (\$myId<\$neighborId) do={:set \$hasClient 1}\
    \n    }\
    \n    :if (\$hasOtherMaster > 0) do={\
    \n      # ensure dhcp-client is activated and network is in ospf range\
    \n      #:put (\"Go client: \".\$interfName)\
    \n      :if ([:len [/ip dhcp-client find where interface=\$interfName]] < \
    1) do={\
    \n        /ip dhcp-client add add-default-route=no dhcp-options=hostname,c\
    lientid disabled=no interface=\$interfName use-peer-dns=no use-peer-ntp=no\
    \n      } \
    \n      /ip address set disabled=yes [/ip address find where dynamic=no di\
    sabled=no interface=\$interfName]\
    \n      /ip dhcp-server set disabled=yes [/ip dhcp-server find where disab\
    led=no interface=\$interfName]\
    \n      /ip dhcp-client set disabled=no [/ip dhcp-client find where disabl\
    ed=yes interface=\$interfName]\
    \n      :local localIpAddress [/ip address find where dynamic=yes interfac\
    e=\$interfName]\
    \n      :if ([:len \$localIpAddress]=1) do={\
    \n          :local network [/ip address get value-name=network \$localIpAd\
    dress]\
    \n          :local ipAddress [/ip address get value-name=address \$localIp\
    Address]\
    \n          :local slashPos [:find \$ipAddress \"/\"]\
    \n          #:put \$network\
    \n          #:put \$ipAddress\
    \n          :local ospfNet (\$network.[:pick \$ipAddress \$slashPos [:len \
    \$ipAddress]])\
    \n          #:put \$ospfNet\
    \n          :if ([:len [/routing ospf network find where network=\$ospfNet\
    ]]<1) do={\
    \n        /routing ospf network add network=\$ospfNet disabled=no area=bac\
    kbone\
    \n          }\
    \n    } else={\
    \n        /ip dhcp-client release [find where interface=\$interfName]\
    \n    }\
    \n    } else={\
    \n      # ensure dhcp-sever is activated and network is in ospf range\
    \n      #:put (\"Go server: \".\$interfName)\
    \n      :if ([:len [/ip dhcp-client find where add-default-route=yes ]]<1)\
    \_do={\
    \n        /ip address set disabled=no [/ip address find where dynamic=no d\
    isabled=yes interface=\$interfName]\
    \n        /ip dhcp-server set disabled=no [/ip dhcp-server find where disa\
    bled=yes interface=\$interfName]\
    \n        /ip dhcp-client set disabled=yes [/ip dhcp-client find where dis\
    abled=no interface=\$interfName]\
    \n        :if (\$hasClient > 0) do={\
    \n          :local network [/ip address get value-name=network   [/ip addr\
    ess find where dynamic=no interface=\$interfName]] \
    \n          :local ipAddress [/ip address get value-name=address  [/ip add\
    ress find where dynamic=no interface=\$interfName]]  \
    \n          :local slashPos [:find \$ipAddress \"/\"]\
    \n          #:put \$network\
    \n          #:put \$ipAddress\
    \n          :local ospfNet (\$network.[:pick \$ipAddress \$slashPos [:len \
    \$ipAddress]])\
    \n          #:put \$ospfNet\
    \n          :if ([:len [/routing ospf network find where network=\$ospfNet\
    ]]<1) do={\
    \n            /routing ospf network add network=\$ospfNet disabled=no area\
    =backbone\
    \n          }\
    \n        }\
    \n      }\
    \n    }\
    \n  }\
    \n\
    \n}"
/user group
set read policy="local,telnet,ssh,read,test,winbox,password,web,sniff,sensitiv\
    e,api,romon,rest-api,!ftp,!reboot,!write,!policy"
add name=dashboard policy="read,write,test,winbox,web,api,rest-api,!local,!tel\
    net,!ssh,!ftp,!reboot,!policy,!password,!sniff,!sensitive,!romon"
add name=prometheus policy="read,api,!local,!telnet,!ssh,!ftp,!reboot,!write,!\
    policy,!test,!winbox,!password,!web,!sniff,!sensitive,!romon,!rest-api"
/caps-man access-list
add action=reject disabled=yes signal-range=-120..-75 ssid-regexp="" time=\
    0s-1d,sun,mon,tue,wed,thu,fri,sat
/caps-man manager
set enabled=yes package-path=sd1-part1/upgrade upgrade-policy=\
    suggest-same-version
/caps-man provisioning
add action=create-dynamic-enabled comment="2.4 GHz" hw-supported-modes=gn \
    identity-regexp=ap-klr-solar master-configuration=base-24 name-format=\
    prefix-identity name-prefix=cap slave-configurations=\
    internet-free,wlan-koa,wlan-mimi,gerhard-wlan,berg-solar
add action=create-dynamic-enabled comment="2.4 GHz" hw-supported-modes=gn \
    identity-regexp=ap-2og-aussen master-configuration=base-24 name-format=\
    prefix-identity name-prefix=cap slave-configurations=\
    internet-free,wlan-koa,wlan-mimi,gerhard-wlan,berg-solar
add action=create-dynamic-enabled comment="5 GHz AC" hw-supported-modes=ac \
    master-configuration=base-5n name-format=prefix-identity name-prefix=cap \
    slave-configurations=internet-free,wlan-koa,wlan-mimi,gerhard-wlan
add action=create-dynamic-enabled comment="5 GHz N" hw-supported-modes=an \
    master-configuration=base-5n name-format=prefix-identity name-prefix=cap \
    slave-configurations=internet-free,wlan-koa,wlan-mimi,gerhard-wlan
add action=create-dynamic-enabled comment="2.4 GHz" hw-supported-modes=gn \
    master-configuration=base-24 name-format=prefix-identity name-prefix=cap \
    slave-configurations=internet-free,wlan-koa,wlan-mimi,gerhard-wlan
/interface bridge port
add bridge=trunk disabled=yes frame-types=admit-only-vlan-tagged \
    ingress-filtering=no interface=s01 internal-path-cost=10 path-cost=10
add bridge=berg-lan ingress-filtering=no interface=vlan1-lan \
    internal-path-cost=10 path-cost=10
add bridge=berg-srv ingress-filtering=no interface=vlan2-srv \
    internal-path-cost=10 path-cost=10
add bridge=berg-storage ingress-filtering=no interface=vlan3-storage \
    internal-path-cost=10 path-cost=10
add bridge=berg-solar interface=vlan7-solar
add bridge=trunk disabled=yes frame-types=admit-only-vlan-tagged interface=\
    bond-ap-ax2 trusted=yes
add bridge=berg-wlan-koa interface=vlan9-wlan-koa
add bridge=berg-wlan-gerhard interface=vlan8-gerhard
add bridge=wlan-mimi interface=vlan10-mimi
add bridge=berg-wlan-free interface=vlan11-free
add bridge=bridge-caps interface=vxlan-client-berg
add bridge=berg-wlan-koa interface=caps-vlan1-koa
add bridge=berg-wlan interface=caps-vlan2-berg
add bridge=berg-wlan-gerhard interface=caps-vlan3-gerhard
add bridge=wlan-mimi interface=caps-vlan4-mimi
add bridge=berg-wlan-free interface=caps-vlan11-free
/ip firewall connection tracking
set udp-timeout=10s
/ip neighbor discovery-settings
set discover-interface-list=all
/ip settings
set max-neighbor-entries=8192
/ipv6 settings
set accept-redirects=no accept-router-advertisements=no max-neighbor-entries=\
    8192 soft-max-neighbor-entries=8191
/interface bridge vlan
add bridge=trunk tagged=s01,bond-ap-ax2,trunk vlan-ids=1
add bridge=trunk tagged=s01,bond-ap-ax2,trunk vlan-ids=2
add bridge=trunk tagged=s01,bond-ap-ax2,trunk vlan-ids=3
add bridge=trunk tagged=s01,bond-ap-ax2,trunk vlan-ids=4
add bridge=trunk tagged=s01,bond-ap-ax2,trunk vlan-ids=5
add bridge=trunk tagged=s01,bond-ap-ax2,trunk vlan-ids=6
add bridge=trunk tagged=s01,bond-ap-ax2,trunk vlan-ids=7
add bridge=trunk tagged=s01,bond-ap-ax2,trunk vlan-ids=8
add bridge=trunk tagged=s01,bond-ap-ax2,trunk vlan-ids=9
add bridge=trunk tagged=s01,bond-ap-ax2,trunk vlan-ids=10
add bridge=trunk tagged=bond-ap-ax2,s01,trunk vlan-ids=11
add bridge=bridge-caps tagged=vxlan-client-berg vlan-ids=1-100
/interface list member
add interface=berg-storage list=storage
add interface=berg-wlan-free list=insecure
add interface=berg-solar list=insecure
/interface ovpn-server server
add auth=sha1,md5 mac-address=FE:18:D8:2A:9E:A8 name=ovpn-server1
/interface vxlan vteps
add interface=vxlan-client-berg remote-ip=172.16.1.6
add interface=vxlan-client-berg remote-ip=172.16.1.53
add interface=vxlan-client-berg remote-ip=172.16.1.57
add interface=vxlan-client-berg remote-ip=172.16.1.59
add interface=vxlan-client-berg remote-ip=172.16.1.61
add interface=vxlan-client-berg remote-ip=172.16.1.62
add interface=vxlan-client-berg remote-ip=172.16.1.63
add interface=vxlan-client-berg remote-ip=172.16.1.64
/interface wifi capsman
set enabled=yes interfaces=caps-vlan99-mgmt package-path=sd1-part1/upgrade \
    require-peer-certificate=no upgrade-policy=suggest-same-version
/interface wifi provisioning
add action=create-dynamic-enabled disabled=no master-configuration=base \
    name-format=cap-%I-%r slave-configurations=\
    berg-koa,berg-mimi,gerhard-wlan,internet-free slave-name-format=%m-%v
/interface wireless cap
set discovery-interfaces=berg-srv
/ip address
add address=10.192.0.1/24 interface=berg-wlan network=10.192.0.0
add address=10.192.1.1/24 interface=berg-wlan-free network=10.192.1.0
add address=10.192.2.1/24 interface=berg-wlan-koa network=10.192.2.0
add address=10.192.3.1/24 interface=wlan-mimi network=10.192.3.0
add address=172.17.0.33/29 disabled=yes interface=e05-wurm network=\
    172.17.0.32
add address=172.16.0.1 interface=loopback network=172.16.0.1
add address=10.192.69.1/30 disabled=yes interface=e01 network=10.192.69.0
add address=172.16.255.1 interface=loopback network=172.16.255.1
add address=172.16.1.1 interface=loopback network=172.16.1.1
add address=10.192.8.1/24 interface=berg-wlan-gerhard network=10.192.8.0
add address=10.192.67.1/24 comment=Solar interface=berg-solar network=\
    10.192.67.0
add address=172.17.1.13/30 interface=c01-keller network=172.17.1.12
add address=172.17.1.25/30 interface=s01 network=172.17.1.24
add address=172.17.1.37/30 interface=e01 network=172.17.1.36
add address=172.17.1.41/30 interface=e05-wurm network=172.17.1.40
add address=172.17.1.58/30 interface=gre-backup-link network=172.17.1.56
/ip cloud
set ddns-enabled=yes
/ip dhcp-client
add add-default-route=no disabled=yes interface=c01-keller name=c01-keller \
    use-peer-dns=no use-peer-ntp=no
/ip dhcp-relay
add add-relay-info=yes dhcp-server=172.31.0.10 interface=vlan6-dsk-koa name=\
    dsk-koa-relay relay-info-remote-id=srv-router
add dhcp-server=172.31.0.10 interface=berg-srv name=srv-relay
add add-relay-info=yes dhcp-server=10.192.6.10 interface=berg-lan name=\
    bootp-relay relay-info-remote-id=""
/ip dhcp-server alert
add disabled=no interface=berg-lan
/ip dhcp-server lease
add address=10.7.7.2 always-broadcast=yes disabled=yes mac-address=\
    B8:AE:ED:7C:A4:5F server=dhcp-lan
add address=172.31.0.3 mac-address=B8:27:EB:DF:B2:73 server=dhcp-srv
add address=172.31.0.73 mac-address=18:A9:05:4F:F8:64 server=dhcp-srv
add address=172.31.0.17 comment=sr08 mac-address=00:0D:B9:35:B7:9C server=\
    dhcp-srv
add address=172.31.0.18 comment=sr09 mac-address=00:0D:B9:36:6C:4C server=\
    dhcp-srv
add address=172.31.0.11 comment=sr02 mac-address=18:A9:05:4B:94:2A server=\
    dhcp-srv
add address=172.31.0.12 comment=sr03 mac-address=18:A9:05:4F:F8:5C server=\
    dhcp-srv
add address=172.31.0.75 always-broadcast=yes comment=sr06-lom mac-address=\
    00:25:90:4A:D6:70 server=dhcp-srv
add address=172.31.1.16 mac-address=00:25:90:4A:B9:31 server=dhcp-storage
add address=172.31.1.15 always-broadcast=yes mac-address=00:25:90:4A:C2:55 \
    server=dhcp-storage
add address=172.31.1.11 mac-address=18:A9:05:4B:94:2C server=dhcp-storage
add address=172.31.1.12 mac-address=18:A9:05:4F:F8:5E server=dhcp-storage
add address=172.31.0.2 mac-address=6C:3B:6B:34:78:17 server=dhcp-srv
add address=172.31.1.2 always-broadcast=yes mac-address=6C:3B:6B:34:78:17 \
    server=dhcp-storage
add address=10.192.7.29 mac-address=00:24:A6:00:25:FD server=dhcp-lan
add address=10.192.7.16 mac-address=A4:EE:57:FC:E7:FB server=dhcp-lan
add address=172.31.0.13 comment=sr04 mac-address=00:23:8B:17:1D:65 server=\
    dhcp-srv
add address=10.192.64.11 comment=strg-keller mac-address=70:B3:D5:50:93:BA \
    server=dhcp-vlan4-tinkerforge
add address=10.192.7.20 comment="Musicbox Wecker" mac-address=\
    B8:27:EB:6C:BB:74 server=dhcp-lan
add address=10.192.65.10 mac-address=6C:3B:6B:34:78:17 server=dhcp-mgmt
add address=10.192.65.11 mac-address=E4:8D:8C:C1:50:FD server=dhcp-mgmt
add address=10.192.65.20 client-id=1:e4:8d:8c:6b:80:e9 mac-address=\
    E4:8D:8C:6B:80:E9 server=dhcp-mgmt
add address=10.192.64.23 comment="tf 1OG B\FCro" mac-address=\
    40:D8:55:02:AA:A2 server=dhcp-vlan4-tinkerforge
add address=10.192.65.21 mac-address=6C:3B:6B:77:0B:EA server=dhcp-mgmt
add address=10.192.65.12 mac-address=E4:8D:8C:81:3E:EF server=dhcp-mgmt
add address=10.192.64.15 mac-address=40:D8:55:02:AB:31 server=\
    dhcp-vlan4-tinkerforge
add address=10.192.65.15 mac-address=C4:AD:34:4F:E3:7C server=dhcp-mgmt
add address=10.192.65.14 mac-address=B8:69:F4:D6:9F:63 server=dhcp-mgmt
add address=10.192.64.16 mac-address=40:D8:55:02:AA:E5 server=\
    dhcp-vlan4-tinkerforge
add address=10.192.64.14 mac-address=40:D8:55:02:AB:EB server=\
    dhcp-vlan4-tinkerforge
add address=10.192.64.17 mac-address=40:D8:55:02:AB:43 server=\
    dhcp-vlan4-tinkerforge
add address=10.192.64.18 mac-address=40:D8:55:02:AC:39 server=\
    dhcp-vlan4-tinkerforge
add address=10.192.64.20 mac-address=40:D8:55:02:AB:C5 server=\
    dhcp-vlan4-tinkerforge
add address=10.192.64.21 mac-address=40:D8:55:02:AC:37 server=\
    dhcp-vlan4-tinkerforge
add address=10.192.64.22 mac-address=40:D8:55:02:AC:3E server=\
    dhcp-vlan4-tinkerforge
add address=10.192.64.24 mac-address=40:D8:55:02:AB:9C server=\
    dhcp-vlan4-tinkerforge
add address=10.192.64.12 mac-address=40:D8:55:02:AA:F4 server=\
    dhcp-vlan4-tinkerforge
add address=10.192.64.19 mac-address=40:D8:55:02:AA:CC server=\
    dhcp-vlan4-tinkerforge
add address=10.192.64.13 mac-address=40:D8:55:02:AB:3F server=\
    dhcp-vlan4-tinkerforge
add address=10.192.7.21 client-id=1:b4:22:0:29:e7:49 mac-address=\
    B4:22:00:29:E7:49 server=dhcp-lan
add address=172.31.0.15 comment=sr06 mac-address=5C:BA:2C:1D:90:56 server=\
    dhcp-srv
add address=10.192.65.17 client-id=1:2c:c8:1b:ba:4a:fc mac-address=\
    2C:C8:1B:BA:4A:FC server=dhcp-mgmt
add address=10.192.64.25 mac-address=40:D8:55:02:AE:4E server=\
    dhcp-vlan4-tinkerforge
add address=10.192.64.26 mac-address=40:D8:55:02:AE:54 server=\
    dhcp-vlan4-tinkerforge
add address=10.192.65.16 client-id=1:2c:c8:1b:ba:39:1a mac-address=\
    2C:C8:1B:BA:39:1A server=dhcp-mgmt
add address=172.31.0.116 client-id=\
    ff:d:96:8f:69:0:2:0:0:ab:11:8a:14:d9:67:3f:3c:53:9d comment=sr07-e1000 \
    mac-address=A8:A1:59:D9:8F:6D server=dhcp-srv
add address=172.31.0.16 mac-address=64:9D:99:B1:67:8E server=dhcp-srv
add address=10.192.65.13 mac-address=C4:AD:34:A1:06:0E server=dhcp-mgmt
add address=10.192.65.22 client-id=1:6c:3b:6b:11:fd:38 mac-address=\
    6C:3B:6B:11:FD:38 server=dhcp-mgmt
add address=10.192.65.23 client-id=1:e4:8d:8c:53:ac:45 mac-address=\
    E4:8D:8C:53:AC:45 server=dhcp-mgmt
add address=10.192.65.24 client-id=1:48:a9:8a:98:42:65 mac-address=\
    48:A9:8A:98:42:65 server=dhcp-mgmt
/ip dhcp-server network
add address=10.14.0.0/16 caps-manager=10.14.10.1 dns-server=172.16.1.1 \
    gateway=10.14.10.1 netmask=16 ntp-server=77.245.18.26
add address=10.192.0.0/24 dns-server=172.16.1.1 gateway=10.192.0.1
add address=10.192.1.0/24 dns-server=172.16.1.1 gateway=10.192.1.1
add address=10.192.2.0/24 dns-server=172.31.88.2 domain=berg-turbenthal.ch \
    gateway=10.192.2.1 netmask=24
add address=10.192.3.0/24 dns-server=172.16.1.1 gateway=10.192.3.1
add address=10.192.5.0/24 dns-server=172.16.1.1 gateway=10.192.5.1
add address=10.192.6.0/24 boot-file-name=netboot.xyz-snp.efi dns-server=\
    172.16.1.1 domain=berg-turbenthal.ch gateway=10.192.6.1 next-server=\
    172.16.1.1
add address=10.192.7.0/24 dns-server=172.16.1.1 gateway=10.192.7.1 netmask=24 \
    next-server=10.192.6.10
add address=10.192.8.0/24 dns-server=172.16.1.1 gateway=10.192.8.1
add address=10.192.64.0/24 dns-server=172.16.1.1 gateway=10.192.64.1
add address=10.192.65.0/24 caps-manager=172.16.0.1 dns-server=172.16.1.1 \
    gateway=10.192.65.1
add address=10.192.66.0/24 dns-server=172.16.1.1 gateway=10.192.66.1
add address=10.192.67.0/24 dns-server=172.16.1.1 gateway=10.192.67.1
add address=10.192.69.0/30 dns-server=172.16.1.1 gateway=10.192.69.1
add address=172.17.0.8/29 dns-server=172.16.1.1 gateway=172.17.0.9
add address=172.17.0.32/29 dns-server=172.16.1.1 gateway=172.17.0.33
add address=172.17.0.40/29 dns-server=172.16.1.1 gateway=172.17.0.41
add address=172.31.0.0/24 boot-file-name=\
    "http://172.31.0.10:8081/ipxe\?mac=\${mac}&uuid=\${uuid}" dns-server=\
    172.16.1.1 domain=srv.berg-turbenthal.ch gateway=172.31.0.1 netmask=24
add address=172.31.1.0/24 dns-server=172.16.1.1
/ip dns
set allow-remote-requests=yes servers=\
    172.31.88.2,2a02:16a:fe00:1a0::10:5,2001:1620:2777:1::10,172.16.1.4
/ip dns static
add address=172.31.0.10 name=sr01.srv.berg-turbenthal.ch ttl=10m type=A
add address=172.31.0.11 name=sr02.srv.berg-turbenthal.ch ttl=10m type=A
add address=172.31.0.12 name=sr03.srv.berg-turbenthal.ch ttl=10m type=A
add address=172.31.0.13 name=sr04.srv.berg-turbenthal.ch ttl=10m type=A
add address=172.31.0.14 name=sr05.srv.berg-turbenthal.ch ttl=10m type=A
add address=172.31.0.15 name=sr06.srv.berg-turbenthal.ch ttl=10m type=A
add address=172.31.0.16 name=sr07.srv.berg-turbenthal.ch ttl=10m type=A
add address=172.31.0.17 name=sr08.srv.berg-turbenthal.ch ttl=10m type=A
add address=172.31.0.18 name=sr09.srv.berg-turbenthal.ch ttl=10m type=A
add address=172.31.0.11 name=kube-api.srv.berg.turbenthal.ch ttl=10m type=A
add address=172.31.0.11 disabled=yes regexp=.*.kube.berg-turbenthal.ch ttl=\
    10m type=A
add address=172.31.0.12 disabled=yes regexp=.*.kube.berg-turbenthal.ch ttl=\
    10m type=A
add address=172.31.0.15 disabled=yes regexp=.*.kube.berg-turbenthal.ch ttl=\
    10m type=A
add address=10.14.50.17 name=ananke.berg-turbenthal.ch ttl=10m type=A
add address=10.14.60.249 name=royalarchive.lan ttl=10m type=A
add address=172.31.1.0 name=test.berg-turbenthal.ch ttl=10h type=A
add address=10.14.60.249 disabled=yes name=matchbox.berg-turbenthal.ch ttl=\
    10m type=A
add address=fd7e:907d:34ab::2 name=station-2.lan type=AAAA
add address=fd7e:907d:34ab::26 name=station-26.lan type=AAAA
add address=fd7e:907d:34ab::41 name=station-41.lan type=AAAA
add address=fd7e:907d:34ab::42 name=station-42.lan type=AAAA
add address=fd7e:907d:34ab::60 name=station-60.lan type=AAAA
add address=fd7e:907d:34ab::61 name=station-61.lan type=AAAA
add address=fd7e:907d:34ab::43 name=station-43.lan type=AAAA
add address=fd7e:907d:34ab::28 name=station-28.lan type=AAAA
add address=fd7e:907d:34ab::44 name=station-44.lan type=AAAA
add address=fd7e:907d:34ab::24 name=station-24.lan type=AAAA
add address=fd7e:907d:34ab::62 name=station-62.lan type=AAAA
add address=fd7e:907d:34ab::20 name=station-20.lan type=AAAA
add address=fd7e:907d:34ab::27 name=station-27.lan type=AAAA
add address=fd7e:907d:34ab::29 name=station-29.lan type=AAAA
add address=fd7e:907d:34ab::33 name=station-33.lan type=AAAA
add address=fd7e:907d:34ab::23 name=station-23.lan type=AAAA
add address=fd7e:907d:34ab::37 name=station-37.lan type=AAAA
add address=fd7e:907d:34ab::8 name=station-8.lan type=AAAA
add address=172.16.1.1 disabled=yes regexp=.*.ing.nuc01.lkw.teamkoenig.ch \
    ttl=10m type=A
add address=fd7e:907d:34ab::7 name=station-7.lan type=AAAA
add address=fd7e:907d:34ab::40 name=station-40.lan type=AAAA
add address=fd7e:907d:34ab::45 name=station-45.lan type=AAAA
add address=fd7e:907d:34ab::63 name=station-63.lan type=AAAA
add address=fd7e:907d:34ab::84 name=station-84.lan type=AAAA
add address=fd7e:907d:34ab::83 name=station-83.lan type=AAAA
add address=fd7e:907d:34ab::85 name=station-85.lan type=AAAA
add address=fd7e:907d:34ab::4 name=station-4.lan type=AAAA
add address=172.31.88.0 name=ing.berg-turbenthal.ch ttl=10m type=A
add address=172.31.0.12 disabled=yes name=jenkins.berg-turbenthal.ch ttl=10m \
    type=A
add address=172.31.0.12 regexp=.*f8.berg-turbenthal.ch ttl=10m type=A
add address=2a02:168:e000:1c0::10:2 name=sr02.srv.berg-turbenthal.ch type=\
    AAAA
add address=2a02:168:e000:1c0::10:3 name=sr03.srv.berg-turbenthal.ch type=\
    AAAA
add address=fd7e:907d:34ab::31 name=station-31.lan type=AAAA
add address=fd7e:907d:34ab::82 name=station-82.lan type=AAAA
add address=fd7e:907d:34ab::46 name=station-46.lan type=AAAA
add address=10.10.0.2 disabled=yes name=lkw.teamkoenig.ch ttl=10m type=A
add address=fd7e:907d:34ab::49 name=station-49.lan type=AAAA
add address=172.31.72.0 regexp=.*.dev.berg-turbenthal.ch ttl=10m type=A
add address=fd7e:907d:34ab::5 name=station-5.lan type=AAAA
add address=fd7e:907d:34ab::3 name=station-3.lan type=AAAA
add address=172.31.72.0 disabled=yes name=prometheus.berg-turbenthal.ch ttl=\
    15m type=A
add address=172.31.0.18 name=nodered.berg-turbenthal.ch ttl=15m type=A
add address=172.31.72.3 name=emby.berg-turbenthal.ch ttl=15m type=A
add address=fd7e:907d:34ab::47 name=station-47.lan type=AAAA
add address=172.31.88.0 disabled=yes regexp=.*.lkw.teamkoenig.ch ttl=15m \
    type=A
add address=fd7e:907d:34ab::11 name=station-11.lan type=AAAA
add address=172.31.0.17 regexp=.*.home.berg-turbenthal.ch ttl=10m type=A
add address=fd7e:907d:34ab::6 name=station-6.lan type=AAAA
add address=172.31.88.0 name=nexus.berg-turbenthal.ch ttl=15m type=A
add address=172.16.1.1 disabled=yes name=api.kube.berg-turbenthal.ch type=A
add address=172.31.0.11 name=api.kube.berg-turbenthal.ch type=A
add address=172.31.0.12 name=api.kube.berg-turbenthal.ch type=A
add address=172.31.0.13 name=api.kube.berg-turbenthal.ch type=A
/ip firewall address-list
add address=172.31.32.0/19 disabled=yes list=bgp-networks
add address=172.16.0.0/12 list=local
add address=172.31.0.11 list=kube-server
add address=172.31.0.12 list=kube-server
add address=172.31.0.15 list=kube-server
add address=172.31.0.16 list=kube-server
add address=172.31.128.0/20 list=kubernetes
add address=172.31.144.0/20 list=kubernetes
add address=172.31.0.0/23 disabled=yes list=bgp-networks
add address=172.31.88.0/24 disabled=yes list=bgp-networks
add address=172.31.0.14 list=kube-main
add address=172.31.0.16 list=kube-main
add address=172.31.0.13 list=kube-main
add address=10.0.0.0/8 list=local
/ip firewall filter
add action=passthrough chain=unused-hs-chain comment=\
    "place hotspot rules here"
add action=accept chain=forward connection-state=established,related
add action=accept chain=forward in-interface=berg-wlan-free out-interface=\
    berg-wlan
add action=accept chain=forward protocol=igmp
add action=accept chain=forward dst-address=172.31.88.0 dst-port=80,443 \
    in-interface-list=insecure protocol=tcp
add action=accept chain=input dst-port=53 protocol=udp
add action=accept chain=forward dst-address=172.31.88.2 dst-port=53 protocol=\
    udp
add action=reject chain=forward dst-address-list=local in-interface-list=\
    insecure log=yes reject-with=icmp-admin-prohibited
/ip firewall nat
add action=passthrough chain=unused-hs-chain comment=\
    "place hotspot rules here" disabled=yes
add action=masquerade chain=srcnat dst-address=10.96.0.1
add action=dst-nat chain=dstnat dst-address=10.96.0.1 dst-port=443 protocol=\
    tcp to-ports=6443
add action=masquerade chain=srcnat disabled=yes dst-address=192.168.88.0/24
add action=dst-nat chain=dstnat disabled=yes dst-address=8.8.8.8 dst-port=53 \
    protocol=udp to-addresses=172.16.0.1
add action=masquerade chain=srcnat disabled=yes dst-address=172.31.72.0/24 \
    src-address=172.31.0.0/24
add action=dst-nat chain=dstnat disabled=yes dst-address=77.109.174.22 \
    dst-port=443 protocol=tcp to-addresses=172.31.72.0 to-ports=443
add action=accept chain=srcnat disabled=yes
add action=dst-nat chain=dstnat dst-address=172.16.1.1 dst-port=6443 \
    protocol=tcp to-addresses=172.31.0.14 to-ports=6443
add action=masquerade chain=srcnat disabled=yes dst-address=172.31.0.0/24 \
    dst-port=6443 protocol=tcp src-address=172.31.0.0/24
add action=dst-nat chain=dstnat dst-address=172.31.76.1 dst-port=443 \
    protocol=tcp to-addresses=172.31.0.12 to-ports=6443
/ip hotspot
add address-pool=wlan_free_pool interface=berg-wlan-free name=hotspot1 \
    profile=hsprof1
/ip hotspot user
add name=admin
add name=B4:CE:F6:09:98:69
/ip hotspot walled-garden
add dst-host=status.kube.berg-turbenthal.ch server=hotspot1
/ip ipsec profile
set [ find default=yes ] dpd-interval=2m dpd-maximum-failures=5
/ip route
add disabled=yes dst-address=0.0.0.0/0 gateway=10.192.65.1
/ip smb shares
set [ find default=yes ] directory=/flash/pub
/ip ssh
set ciphers=aes-gcm,aes-ctr,aes-cbc,3des-cbc,null forwarding-enabled=remote
/ip tftp
add disabled=yes ip-addresses=0.0.0.0/0 real-filename=storage/undionly.kpxe \
    req-filename=undionly.kpxe
add disabled=yes ip-addresses=0.0.0.0/0 real-filename=storage/sr01.kpxe \
    req-filename=sr01.kpxe
add ip-addresses=0.0.0.0/0 real-filename=sd1-part1/undionly.kpxe \
    req-filename=http://*
add real-filename=sd1-part1/tftp/netboot.xyz.kpxe req-filename=\
    netboot.xyz.kpxe
add real-filename=sd1-part1/tftp/netboot.xyz-undionly.kpxe req-filename=\
    netboot.xyz-undionly.kpxe
add real-filename=sd1-part1/tftp/netboot.xyz.efi req-filename=netboot.xyz.efi
add real-filename=sd1-part1/tftp/netboot.xyz-snp.efi req-filename=\
    netboot.xyz-snp.efi
/ipv6 address
add address=2a02:16a:fe00:1ff::1/128 advertise=no interface=loopback
add address=2a02:16a:fe00:104::1 interface=berg-wlan-gerhard
add address=2a02:16a:fe00:103::1 interface=berg-wlan-koa
add address=2a02:16a:fe00:122::1 interface=berg-solar
add address=2a02:16a:fe00:106::1 interface=berg-wlan
add address=2a02:16a:fe00:105::1 interface=berg-wlan-free
add address=2a02:16a:fe00:101::1 interface=wlan-mimi
add address=2a02:16a:fe00:ffff:ffff:ffff:ffff:ff01/128 advertise=no \
    interface=lo
/ipv6 firewall address-list
add address=2a02:168:e000:100::/64 disabled=yes list=bgp-networks
add address=2a02:168:e000:1a0::10:0/108 disabled=yes list=bgp-networks
add address=2a02:16a:fe00:1a0::10:0/128 list=public
add address=2a02:16a:fe00:1a0::10:5/128 list=public
add address=2a02:16a:fe00::/48 list=local
add address=2a02:168:e000::/48 list=local
/ipv6 firewall filter
add action=accept chain=forward dst-address-list=public in-interface-list=\
    insecure
add action=reject chain=forward dst-address-list=local in-interface-list=\
    insecure reject-with=icmp-admin-prohibited
/ipv6 nd
set [ find default=yes ] disabled=yes reachable-time=1m retransmit-interval=\
    2m
add interface=vlan6-dsk-koa
add disabled=yes interface=berg-srv
add advertise-dns=yes disabled=yes hop-limit=64 interface=berg-lan
# automatic dns option advertising is not started, re-apply dns config
add advertise-dns=yes advertise-mac-address=no dns=2a02:16a:fe00:1a0::10:5 \
    interface=berg-wlan-koa
# automatic dns option advertising is not started, re-apply dns config
add advertise-dns=yes interface=berg-solar
# automatic dns option advertising is not started, re-apply dns config
add advertise-dns=yes interface=berg-wlan
# automatic dns option advertising is not started, re-apply dns config
add advertise-dns=yes interface=berg-wlan-gerhard
# automatic dns option advertising is not started, re-apply dns config
add advertise-dns=yes interface=berg-wlan-free
# automatic dns option advertising is not started, re-apply dns config
add advertise-dns=yes interface=wlan-mimi
/routing bfd configuration
add disabled=no interfaces=all min-rx=200ms min-tx=1s multiplier=20
/routing bgp connection
add as=65580 disabled=yes instance=bgp-backup-link local.role=ebgp name=bgp1 \
    output.redistribute=ospf remote.address=10.195.16.1 .as=4200000000 \
    routing-table=main
/routing filter rule
add chain=storage comment="upgrade-notes:\
    \naction log is not available in v7" disabled=yes rule=""
add chain=storage disabled=yes rule=\
    "if (bgp-as-path-slow-legacy \"64521\") { set gw 172.31.1.11; }"
add chain=storage disabled=yes rule=\
    "if (bgp-as-path-slow-legacy \"64522\") { set gw 172.31.1.12; }"
add chain=storage disabled=yes rule=\
    "if (bgp-as-path-slow-legacy \"64525\") { set gw 172.31.1.15; }"
add chain=storage disabled=yes rule=\
    "if (bgp-as-path-slow-legacy \"64526\") { set gw 172.31.1.16; }"
add chain=storage disabled=yes rule=\
    "if (bgp-path-len in 2-4294967295) { reject; }"
add chain=ospf-out disabled=yes rule=\
    "if (dst in 172.31.72.0 && dst-len == 22) { reject; }"
add chain=ospf-out disabled=no rule=\
    "if (dst in 172.31.80.0/20 && dst-len in 20-32) { accept; }"
add chain=ospf-out disabled=no rule=\
    "if (dst in 172.31.76.0/22 && dst-len in 22-32) { accept; }"
add chain=ospf-out disabled=no rule=\
    "if (dst in 172.31.96.0/22 && dst-len in 22-32) { accept; }"
add chain=ospf-out disabled=no rule=\
    "if (dst in 172.31.104.0/22 && dst-len in 22-32) { accept; }"
add chain=ospf-out disabled=no rule=\
    "if (dst in 2a02:168:e000:100::/56 && dst-len in 0-128) { accept; }"
add chain=ospf-out disabled=no rule="if (protocol bgp) { reject; }"
add chain=calico-out disabled=no rule="if (bgp-path-len > 1) {reject }"
/routing ospf interface-template
add area=backbone-v2 disabled=no interfaces=s01,c01-keller,e01,e05-wurm
add area=backbone-v3 disabled=no interfaces=s01,e05-wurm,c01-keller,e01
add area=backbone-v2 cost=50 disabled=yes interfaces=gre-backup-link type=\
    ptp-unnumbered
add area=backbone-v3 cost=50 disabled=yes interfaces=gre-backup-link type=ptp
/snmp
set enabled=yes
/system clock
set time-zone-name=Europe/Zurich
/system identity
set name=srv-router-berg
/system logging
add disabled=yes topics=pim
add disabled=yes topics=dhcp
add disabled=yes topics=route,bgp
add action=remote topics=info
add action=remote topics=warning
add action=remote topics=error
add action=remote topics=critical
/system ntp client
set enabled=yes
/system ntp client servers
add address=0.ch.pool.ntp.org
add address=1.ch.pool.ntp.org
/system package update
set channel=long-term
/system routerboard settings
set auto-upgrade=yes
/system scheduler
add disabled=yes interval=1m name=check-master on-event=\
    "/system script run check-master" policy=\
    ftp,reboot,read,write,policy,test,password,sniff,sensitive,romon \
    start-time=startup
add interval=23h name=backup on-event=\
    "/system/backup/save name=sd1-part1/full-backup" policy=read,sensitive \
    start-date=2024-03-24 start-time=05:19:24
/tool e-mail
set certificate-verification=no from=\
    Router-Berg<router-berg@berg-turbenthal.ch> server=aspmx.l.google.com
/tool graphing
set store-every=hour
/tool graphing interface
add interface=e05-wurm
add interface=e07-hegglin
add interface=s01
/tool graphing resource
add
/tool netwatch
add disabled=no down-script="tool e-mail send to=andreas.koenig@berg-turbentha\
    l.ch subject=\"sr01 down\"" host=172.31.0.11 interval=1m test-script="" \
    timeout=10s type=simple up-script="tool e-mail send to=andreas.koenig@berg\
    -turbenthal.ch subject=\"sr01 up\""
add disabled=yes down-script="tool e-mail send to=andreas.koenig@berg-turbenth\
    al.ch subject=\"sr07 down\"" host=172.31.0.16 interval=1m test-script="" \
    timeout=10s type=simple up-script="tool e-mail send to=andreas.koenig@berg\
    -turbenthal.ch subject=\"sr07 up\""
add disabled=no down-script="tool e-mail send to=andreas.koenig@berg-turbentha\
    l.ch subject=\"sr03 down\"" host=172.31.0.12 interval=1m timeout=10s \
    type=simple up-script="tool e-mail send to=andreas.koenig@berg-turbenthal.\
    ch subject=\"sr03 up\""
add disabled=no down-script="tool e-mail send to=andreas.koenig@berg-turbentha\
    l.ch subject=\"sr04 down\"" host=172.31.0.13 interval=1m timeout=10s \
    type=simple up-script="tool e-mail send to=andreas.koenig@berg-turbenthal.\
    ch subject=\"sr04 up\""
add disabled=yes down-script="tool e-mail send to=andreas.koenig@berg-turbenth\
    al.ch subject=\"sr06 down\"" host=172.31.0.15 interval=1m test-script="" \
    timeout=10s type=simple up-script="tool e-mail send to=andreas.koenig@berg\
    -turbenthal.ch subject=\"sr06 up\""
add disabled=yes down-script="tool e-mail send to=andreas.koenig@berg-turbenth\
    al.ch subject=\"oberon down\"" host=10.14.50.13 interval=1m test-script=\
    "" timeout=10s type=simple up-script="tool e-mail send to=andreas.koenig@b\
    erg-turbenthal.ch subject=\"oberon up\""
/tool sniffer
set filter-interface=caps-vlan1-koa filter-mac-address=\
    46:C3:62:FD:13:2C/FF:FF:FF:FF:FF:FF filter-operator-between-entries=and \
    streaming-enabled=yes streaming-server=10.192.6.8
