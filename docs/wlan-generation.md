# WLAN/CAPsMAN-Generierung

Code: `backend/src/device/ros/mod.rs` (`WirelessDeviceData`/`WifiDeviceData`-Modelle,
`WifiDeviceDataTarget::generate_from`), `backend/src/device/ros/l2/mod.rs` (`L2Port::CapsMan`/`Cap`/
`CapsManMgmt`/`CapMgmt`), `mikrotik-model/src/hwconfig.rs` (`generate_wifi`/`generate_wlan`).

Generiert wird der neuere WiFi-Stack (`/interface/wifi`) mit CAPsMAN und lokalem Forwarding auf
dem CAP, im Lab erprobt mit einem wAP ac (`RBwAPG-5HacD2HnD`, `wifi-qcom-ac`) als CAP, erst mit
dem WLAN lokal auf dem wAP (Controller lab-np2), dann mit dem WLAN per VXLAN auf dem Controller
lab-core (CCR2004). Alte APs mit dem `wireless`-Paket (im Lab ein SXT ac und ein RB912) werden
CAPs des klassischen CAPsMAN (`/caps-man`) auf demselben Controller, siehe
[Klassischer CAPsMAN-Stack](#klassischer-capsman-stack).

## Zwei RouterOS-Pakete, zwei getrennte Modelle

RouterOS hat zwei unabhängige, sich gegenseitig ausschließende WLAN-Stacks, daher zwei getrennte
`mikrotik_model!`-Definitionen in `device/ros/mod.rs`:

- **`WirelessDeviceData`** (`interface/wireless/cap`) — der ältere, klassische CAPsMAN-Stack
  (Paket `wireless`), für Geräte wie `CRS109-8G-1S-2HnD` (siehe `hw_facts.rs::build_wireless_ports`,
  nutzt `generate_wlan`).
- **`WifiDeviceData`** (`interface/wifi/{cap,capsman,datapath,configuration,provisioning}`) — der
  neuere WiFiWave2-Stack (Paket `wifi-qcom` bzw. `wifi-qcom-ac`), für Geräte wie
  `C52iG-5HaxD2HaxD` oder den wAP ac. Die Radios übernimmt die Generierung vom Gerät
  (`hw_facts.rs::build_wifi_ports` ist noch ungenutzt).

Welches Paket die Radios eines Boards brauchen, steht in `hw_facts.rs::radio_package` (dort gehört
alles Modellspezifische hin). `needed_radio_package` (`device/ros/mod.rs`) gibt es nur zurück,
wenn NetBox die Radios nutzt: ein Radio mit RF-Rolle, in einer Bridge, mit VLAN oder Adresse, oder
das Gerät ist AP einer Wireless LAN Group. Nur dann liest `DeviceCfg::fetch_with_board_model` das
passende Modell (`WirelessDeviceData` und `W60GDeviceData` für `wireless`, `WifiDeviceData` für
`wifi-qcom*`), und das Paket muss installiert sein, sonst schlägt die Generierung mit dem Hinweis
auf das Update fehl. Das RouterOS-Update installiert genau `routeros` und dieses Paket und
deinstalliert alle anderen. Ein Controller liest `WifiDeviceData` auch ohne Radios: CAP und
CAPsMAN von `/interface/wifi` sind ab RouterOS 7.13 im Basispaket.

## WiFi: Controller und CAP

Das Modell ist geroutet: Der CAP findet den Controller an dessen Loopback
(`caps-man-addresses`), ein L2-Pfad oder eine Management-VLAN zum Controller ist nicht nötig. Der
CAP bridged den Verkehr lokal.

- **Controller** (`device.wlan_controller_of()`): `/interface/wifi/capsman enabled=yes`, pro WLAN
  eine `/interface/wifi/configuration` (Name aus der SSID, `country` aus der Region des Standorts
  des Controllers, wie bei den Funkstrecken; WPA Personal als `wpa2-psk,wpa3-psk` mit
  `security.passphrase`, Open als `owe`). Eine `/interface/wifi/provisioning`-Regel
  (`create-dynamic-enabled`) gibt jedem Radio das erste WLAN als Master, die übrigen als virtuelle
  Interfaces (`slave-configurations`).
- **CAP** (`device.wlan_ap_of()`): `/interface/wifi/cap enabled=yes` mit der Loopback des Controllers.
  Jedes Radio bekommt `configuration.manager=capsman` und einen `/interface/wifi/datapath` mit dem
  Namen und der `bridge` seiner Bridge; über `slaves-datapath` gilt der auch für die virtuellen
  Interfaces. Seine Bridge ist die, in die NetBox es legt, sonst die Bridge mit der VLAN des ersten
  WLANs, das eine hat (`L2Setup::bridge_of_wlan`, auf einem AP die Standard-Bridge `bridge-caps`).
  RouterOS legt die Radios damit selbst als dynamische Bridge-Ports an, `setup_l2` lässt ihren
  statischen Bridge-Port deshalb weg. Die Radios werden wie Ethernet-Ports nach L2 benannt (`w01`,
  `w02`). Alle übrigen Felder eines Radios bleiben, wie das Gerät sie hat, ebenso alle Radios eines
  Geräts, das kein CAP ist.
- **WLAN mit VLAN**: Der CAP setzt die VLAN, nicht der Controller (der kennt die Boards seiner
  CAPs nicht). Ein Radio hat deshalb eine VLAN: WLANs einer Gruppe mit verschiedenen VLANs werden
  abgelehnt (`SetupError::MultipleWlanVlans`). Wie, hängt vom Radio-Paket ab
  (`RadioPackage::tags_vlans` in `hw_facts.rs`):
  - `wifi-qcom` (802.11ax): Der lokale Datapath bekommt `vlan-id`, RouterOS macht die Radios zu
    getaggten Mitgliedern der VLAN (dynamischer Eintrag in `/interface/bridge/vlan`, das Radio
    taggt selbst).
  - `wifi-qcom-ac` (802.11ac, z.B. wAP ac): Diese Chips können das nicht („802.11ac chipsets do not
    support this type of VLAN tagging“, MikroTik-WiFi-Handbuch). Mit `vlan-id` assoziieren Clients
    zwar, aber ihr Verkehr kommt nicht in die Bridge (im Lab: keine DHCP-Lease, `tx-drop` der
    Radios steigt). Die Radios bekommen deshalb keinen Datapath, sondern sind statische
    Access-Ports der Bridge (`pvid` = WLAN-VLAN, untagged in `/interface/bridge/vlan`), die
    `setup_l2` anlegt. Virtuelle Interfaces (weitere WLANs) erben das nicht; deshalb wäre auch dort
    nur eine VLAN möglich.

  Liegt die
  VLAN in einer VLAN-Gruppe mit VNI, trägt das VXLAN sie zum Controller (siehe
  [vxlan-generation.md](vxlan-generation.md)), und das Netz endet dort auf einem virtuellen
  Interface mit IP und `dhcp`. Der AP selbst bekommt in dieser VLAN kein VLAN-Interface: das gibt es
  nur, wo das Gerät eine Adresse oder einen DHCP-Client in der VLAN hat
  (`TaggedBridgeEntry::has_l3`).
- `/interface/wifi/capsman` meldet `interfaces`, `upgrade-policy` usw. erst, nachdem sie einmal
  gesetzt wurden (auch auf ihren Standardwert). Sie sind deshalb `k`: Wo CAPsMAN aus ist, bleiben
  sie dem Gerät, der Controller bekommt `interfaces=all`, `upgrade-policy=suggest-same-version`
  und `require-peer-certificate=no` explizit.

Ist ein Gerät nicht mehr Controller oder CAP, wird `WifiDeviceData` trotzdem gelesen, solange
CAPsMAN oder CAP noch an sind oder die Zertifikate noch da sind, die CAPsMAN für sich generiert hat
(`/interface/wifi/capsman` nennt sie in `generated-ca-certificate`/`generated-certificate`, auch
ausgeschaltet). Die Generierung schaltet dann beides aus, entfernt Configurations, Provisioning und
Datapaths und diese beiden Zertifikate. Verweise auf einen Datapath (`datapath`,
`datapath.bridge`, `slaves-datapath`) und `datapath.vlan-id` löscht das Modell mit `!feld`
(Flag `unset`): `datapath=""` ignoriert RouterOS ohne Fehler, `datapath.bridge=""` lehnt es als
mehrdeutig ab. Das Entfernen eines Datapath sortiert `sort_mutations` nach den Updates, die die
Verweise darauf lösen (`ResourceMutation::releases`). Andere Zertifikate bleiben unangetastet. Der Trust Store
taugt nicht als Merkmal: Unter 7.20 generierte CAPsMAN-Zertifikate stehen in `all`, unter 7.24 in
`capsman`. Nach dem Entfernen zeigt `/interface/wifi/capsman` die alten IDs (`*1`) weiter an; das ist
RouterOS-Zustand ohne Wirkung.

Nicht umgesetzt: die Management-VLAN der Gruppe (`wlan_mgmt`), die das geroutete Modell nicht
braucht, und WLANs, deren VLANs auf einem AP in verschiedenen Bridges liegen (ein Radio hat nur
einen Datapath).

## Was tatsächlich generiert wird: nur die L2-Seite

In der L2-Klassifizierung (`L2Setup::new`, siehe [l2-generation.md](l2-generation.md)) werden
WLAN-VLANs bereits korrekt eingeplant:

- Ist das Gerät **Access Point** (`device.wlan_ap_of()`): die Management-VLAN der Gruppe wird als
  `L2Port::CapMgmt` eingetragen, jede WLAN-VLAN eines einzelnen WLANs als `L2Port::Cap { wlan }`.
- Ist das Gerät **Controller** (`device.wlan_controller_of()`): analog, aber als
  `L2Port::CapsManMgmt` bzw. `L2Port::CapsMan { wlan }`.

Diese Einträge beeinflussen bereits korrekt, ob eine VLAN-Ebene als `TaggedBridge` klassifiziert
wird (jede VXLAN/CAPsMan/Cap-Anbindung erzwingt das, siehe
[l2-generation.md](l2-generation.md#schritt-4-klassifizierung-pro-root--welche-der-drei-l2-domänen-arten)),
und der VXLAN-Transport für diese VLANs wird vollständig generiert (siehe
[vxlan-generation.md](vxlan-generation.md)). `setup_l2` selbst ignoriert diese vier `L2Port`-Varianten
beim Realisieren der Bridge-Mitgliedschaft aber explizit (`L2Port::CapsMan {..} | Cap {..} |
CapsManMgmt | CapMgmt | Ip(_) => {}` in `setup_l2`s VLAN-Schleife) — sie tragen nur zur
VLAN-Zusammensetzung bei, erzeugen selbst aber keinen Bridge-Mitgliedsport (naheliegend: ein
CAPsMAN-WLAN ist kein physischer Ethernet-Port, der gebridged werden könnte).

## Klassischer CAPsMAN-Stack

Code: `CapsManDeviceDataTarget` und `WirelessDeviceDataTarget::setup_legacy_cap`
(`device/ros/mod.rs`), `Topology::is_legacy_cap`.

Ein AP einer Wireless LAN Group, der in NetBox `wlan<N>`-Ports hat (Radios des `wireless`-Pakets),
ist ein **Legacy-CAP**. RouterOS 7 hat für diese Radios nur den alten CAPsMAN, nicht den von
`/interface/wifi`; der Controller einer Gruppe mit Legacy-CAPs betreibt deshalb beide.

- **Legacy-CAP**: `/interface/wireless/cap enabled=yes` mit der Loopback des Controllers und den
  Radios aus NetBox (nach L2 benannt, `w01`). Kein `bridge`: CAPsMAN leitet den Verkehr der Clients
  zum Controller (`local-forwarding=no`). Der CAP bekommt deshalb weder die WLAN-VLAN noch das
  VXLAN, und er ist kein VTEP (`VlanAccess::devices` lässt ihn aus). Ein Radio, das nicht vom Board
  kommt (5-GHz-Karte im miniPCIe-Slot eines RB912, `wlan2`), übernimmt die Generierung vom Gerät.
- **Controller**: braucht das Paket `wireless` (`needed_radio_package` gibt es auch einem Board
  ohne eigene Radios). `/caps-man/manager enabled=yes`; pro WLAN eine `/caps-man/security`
  (`wpa2-psk`, `aes-ccm`), ein `/caps-man/datapath` (`bridge` = die Bridge der WLAN-VLAN,
  `vlan-mode=use-tag`, `vlan-id`, `local-forwarding=no`) und eine `/caps-man/configuration` (SSID,
  `country` wie bei `/interface/wifi`, `mode=ap`), alle nach der SSID benannt. Eine
  Provisioning-Regel gibt jedem Radio das erste WLAN als Master und die übrigen als Slaves.

RouterOS legt die Radios als dynamische Interfaces `capN` an und macht sie zu Bridge-Ports mit
`pvid` = WLAN-VLAN. Untagged-Mitglied der VLAN (dynamischer Eintrag „added by pvid“) wird ein Port
erst, wenn er läuft, und ein Legacy-Radio läuft erst mit einem Client.

Ein AP, dessen Radios NetBox nicht kennt, behält seine CAP-Konfiguration (ein CAP eines CAPsMAN,
den NetBox nicht kennt). Andere Geräte haben den CAP aus. Radios mit RF-Rolle sind Enden von
Punkt-zu-Punkt-Funkstrecken (siehe [netbox-modeling.md](netbox-modeling.md)), Radios ohne RF-Rolle
behalten ihre Konfiguration.
