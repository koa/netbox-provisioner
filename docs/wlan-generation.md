# WLAN/CAPsMAN-Generierung (Stand: größtenteils nicht implementiert)

Code: `backend/src/device/ros/mod.rs` (`WirelessDeviceData`/`WifiDeviceData`-Modelle,
`WifiDeviceDataTarget::generate_from`), `backend/src/device/ros/l2/mod.rs` (`L2Port::CapsMan`/`Cap`/
`CapsManMgmt`/`CapMgmt`), `mikrotik-model/src/hwconfig.rs` (`generate_wifi`/`generate_wlan`,
`find_wlan_method`).

Diese Datei ist bewusst ehrlich darüber, was **tatsächlich** generiert wird, statt den Eindruck zu
erwecken, WLAN-Provisionierung sei fertig — das ist sie nicht.

## Zwei RouterOS-Pakete, zwei getrennte Modelle

RouterOS hat zwei unabhängige, sich gegenseitig ausschließende WLAN-Stacks, daher zwei getrennte
`mikrotik_model!`-Definitionen in `device/ros/mod.rs`:

- **`WirelessDeviceData`** (`interface/wireless/cap`) — der ältere, klassische CAPsMAN-Stack
  (Paket `wireless`), für Geräte wie `CRS109-8G-1S-2HnD` (siehe `hw_facts.rs::build_wireless_ports`,
  nutzt `generate_wlan`).
- **`WifiDeviceData`** (`interface/wifi/{cap,capsman,datapath,configuration,provisioning}`) — der
  neuere WiFiWave2-Stack (Paket `wifi-qcom`), für Geräte wie `C52iG-5HaxD2HaxD`
  (`hw_facts.rs::build_wifi_ports`, nutzt `generate_wifi`). `find_wlan_method` entscheidet pro
  Board-Modell, welcher der beiden (falls überhaupt einer) zutrifft.

`DeviceCfg::fetch_with_board_model` liest zur Laufzeit nach, welches der beiden Pakete tatsächlich
installiert ist (`installed_packages.contains(b"wireless")` bzw. `b"wifi-qcom"`), und befüllt nur
den entsprechenden `*_current`/`*_target`-Zweig — ein Gerät ohne eines der beiden Pakete bekommt
schlicht `None` für beide, kein Fehler.

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

## Was fehlt: die eigentliche CAPsMAN-/WiFi-Konfiguration

`WifiDeviceDataTarget::generate_from` ist ein `todo!("generate CAPsMan/wifi config for the mgmt
VLAN - not part of L2Setup completion")`. Der bereits vorhandene, aber auskommentierte Entwurf zeigt
die beabsichtigte Richtung:

- `/interface/wifi/datapath` (`dp-caps`) gebunden an die Management-Bridge.
- `/interface/wifi/configuration` (`capsman`) mit `manager = Capsman`.
- `/interface/wifi/cap` mit `discovery-interfaces` = die Management-Schnittstelle, `enabled = yes`.
- `/interface/wifi/provisioning` mit `action = CreateDynamicEnabled`.

Das ist **nicht aktiv** — der `todo!()` panict, sobald ein Gerät sowohl `wifi-qcom` installiert hat
als auch `wlan_ap_of()` etwas liefert. `WirelessDeviceDataTarget::generate_from` (der ältere
CAPsMAN-Stack) ist sogar noch leerer — ein reiner No-Op ohne jeden Entwurf.

**Praktische Konsequenz**: Ein Szenario, das einen echten Access Point mit aktivem WLAN durchgängig
End-to-End provisionieren will, scheitert heute spätestens an dieser Stelle. Die VXLAN-Transport-
und L2-Vorarbeit ist vollständig nutzbar, sobald dieser `todo!()` durch eine echte Implementierung
ersetzt wird.
