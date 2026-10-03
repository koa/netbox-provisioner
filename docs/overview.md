# Config-Generierungs-Pipeline: Überblick

Dieses Verzeichnis beschreibt, wie `backend/src/device/ros/` aus NetBox-Topologiedaten eine
RouterOS-Konfiguration ableitet. Es beschreibt **Verhalten und Design-Entscheidungen**, nicht die
Rust-Typen selbst — die sind im Code (`device/ros/mod.rs`, `device/ros/l2/mod.rs`) meist gut genug
benannt, dass eine zusätzliche Typ-Referenz hier nur veralten würde.

## Die Pipeline (`device/ros/graphql.rs::DeviceCfg`)

1. **Fetch**: `DeviceCfg::fetch`/`fetch_with_board_model` liest den **Ist-Zustand** direkt vom
   Zielgerät (`BaseDeviceDataCurrent::fetch`, plus optional `WirelessDeviceDataCurrent`/
   `WifiDeviceDataCurrent`, je nachdem welche Pakete installiert sind). Das Board-Modell (z.B.
   `CCR1009-7G-1C-1S+`) bestimmt die erwartete Ethernet-Port-Liste (`hw_facts.rs`,
   siehe [interface-naming.md](interface-naming.md)).
2. **Generate (Soll)**: `DeviceCfg::generate_from` baut den **Ziel-Zustand**
   (`BaseDeviceDataTarget` + optional `WirelessDeviceDataTarget`/`WifiDeviceDataTarget`) rein aus
   der NetBox-Topologie (`DeviceAccess` und die anderen `topology::access::*`-Typen). Das passiert
   in vier Schritten, in dieser Reihenfolge, weil spätere Schritte auf dem Ergebnis früherer
   aufbauen:
   - `set_name_and_loopback` — Identity + Loopback-IP.
   - `L2Setup::new` — die komplette Schicht-2-Topologie (Bridges, VLANs, welche physischen Ports
     wohin gehören). Siehe [l2-generation.md](l2-generation.md).
   - `setup_ip_addresses` / `setup_ospf` — IP-Adressen, DHCP-Server/-Client, OSPF — operieren auf
     `L2Setup::logical_interfaces()`, einer vereinheitlichten Sicht über alle drei L2-Domänen-Arten,
     und müssen daher selbst nicht wissen, ob eine logische Schnittstelle ein Einzelport, eine
     flache Bridge oder ein VLAN auf einer getaggten Bridge ist. Siehe
     [ospf-generation.md](ospf-generation.md).
   - `setup_wlan_ap` — VXLAN-Transport für WLAN-Management-/Client-VLANs, falls das Gerät ein
     Access Point ist. Siehe [vxlan-generation.md](vxlan-generation.md) und
     [wlan-generation.md](wlan-generation.md).
   - `setup_l2` — realisiert den bereits berechneten `L2Setup` als tatsächliche
     Bridge-/Bridge-Port-/Bridge-VLAN-/Interface-VLAN-Einträge und benennt dabei jeden physischen
     Ethernet-Port auf seinen generierten Namen um. Legt außerdem die VXLAN-Interfaces für
     Port-Gruppen an, die an einem VXLAN terminiert sind (siehe
     [vxlan-generation.md](vxlan-generation.md)).
3. **Diff**: `DeviceCfg::generate_mutations` vergleicht Ist (Schritt 1) gegen Soll (Schritt 2) und
   erzeugt eine Liste von `ResourceMutation`s (Add/Set/Remove pro RouterOS-Pfad). Das ist generischer
   `mikrotik-model`-Mechanismus, nicht spezifisch für diese Pipeline.
4. **Apply**: siehe `mikrotik-model`-Repo (`apply_mutation`/`apply_mutations_via_script`) — nicht
   Teil dieses docs-Ordners, da es keine NetBox-spezifische Logik enthält.

## Zentrale Idee: drei unabhängige L2-Domänen-Arten

Eine NetBox-Topologie wird nicht als eine flache Liste von Interfaces behandelt, sondern in genau
drei Kategorien pro physischem "Root"-Gerät (meist: der Switch/Router selbst) klassifiziert:

- **Einzelport** (`single_ports`): ein Port, keine Bridge nötig.
- **Flache Bridge** (`simple_bridges`): mehrere Ports, aber nur eine L2-Domäne (kein VLAN-Filtering).
- **Getaggte Bridge** (`tagged_bridges`): VLAN-Filtering-Bridge, sobald mehr als eine VLAN-Ebene
  oder ein VXLAN/CAPsMAN-Attachment im Spiel ist.

Diese Klassifizierung passiert einmalig in `L2Setup::new` und wird von allen nachgelagerten
Schritten (IP, DHCP, OSPF, Bridge-Realisierung) weiterverwendet. Details: siehe
[l2-generation.md](l2-generation.md).

## Namensgenerierung ist austauschbar

Alle Namen (Interface, VLAN, VXLAN, Bridge) werden über das `NameGenerator`-Trait erzeugt, nicht
hart codiert. Produktiv wird `EndpointNameGenerator` verwendet (sprechende Namen aus Port-Kurzname +
Label/Kabel-Fernende); Tests nutzen oft `KeepNameGenerator` (triviale, deterministische Namen).
Siehe [interface-naming.md](interface-naming.md).

## Bekannte Lücken (Stand dieser Dokumentation)

- **WLAN/CAPsMAN-Konfiguration selbst ist nicht implementiert**: `WifiDeviceDataTarget::generate_from`
  ist ein `todo!()`. Nur die VXLAN-Transport-Seite (siehe [vxlan-generation.md](vxlan-generation.md))
  und die L2-Klassifizierung der WLAN-VLANs (`L2Port::CapsMan`/`Cap`/`CapsManMgmt`/`CapMgmt`) sind
  bereits fertig. Siehe [wlan-generation.md](wlan-generation.md) für den genauen Stand.
- Patch-Panel-Ketten (`FrontPort`/`RearPort`) werden von `InterfaceAccess::connected_interfaces`
  (cable walk) zwar generisch aufgelöst, aber der Multi-CHR-Testharness verkabelt bisher nur direkte
  Interface-zu-Interface-Kabel.
