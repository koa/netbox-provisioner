# Interface-, VLAN-, VXLAN- und Bridge-Namensgenerierung

Code: `backend/src/device/ros/l2/mod.rs` (`NameGenerator`-Trait, `EndpointNameGenerator`,
`KeepNameGenerator`), `mikrotik-model/src/hwconfig.rs` (`EthernetNamePattern::short_name`).

## Warum ein austauschbares `NameGenerator`-Trait

Jeder Name, der am Ende in der generierten RouterOS-Config auftaucht (physischer Port-Name, VLAN-
Interface-Name, VXLAN-Interface-Name, Bridge-Name), kommt aus einer Implementierung von
`NameGenerator`, nicht aus fest verdrahteter Logik in `L2Setup::new`. Das hält die
L2-Klassifizierungslogik (siehe [l2-generation.md](l2-generation.md)) komplett unabhängig davon,
*wie* etwas heißen soll — nur *dass* es einen Namen bekommt.

Zwei Implementierungen existieren:

- **`KeepNameGenerator`**: triviale, deterministische Namen (`vlan-{id}`, `bridge`,
  `vxlan-{vxlan-id}`, behält den NetBox-Interface-Namen unverändert). Wird in Tests verwendet, wo
  Lesbarkeit der generierten Namen irrelevant, aber Determinismus wichtig ist.
- **`EndpointNameGenerator`**: die produktiv verwendete Implementierung, erzeugt sprechende Namen
  aus Hardware-Kurzname + Kontext (Label oder Kabel-Fernende). Siehe unten.

## `EndpointNameGenerator::generate_interface_name`

Baut den Namen aus bis zu zwei Teilen, durch `-` verbunden:

1. **Port-Kurzname** (`InterfaceAccess::external_port().short_name()`), z.B. `e03` für den dritten
   Ethernet-Port (`EthernetNamePattern::short_name`: `e{idx:02}` für Ether, `s{idx:02}` für
   SFP/SFP+, `c{idx:02}` für Combo, `q{idx:02}-{n}` für QSFP+-Breakout). Fehlt, wenn das Interface
   kein physischer Port ist (z.B. eine reine VLAN-Schnittstelle ohne eigenen Port).
2. **Kontext-Teil**, mit klarer Priorität:
   - Ist auf dem NetBox-Interface ein **Label** gesetzt (z.B. "AP Keller", "Bachmann Dachboden"),
     wird das verwendet (`to_case(Case::Kebab)` → `ap-keller`). Das ist bewusst die stärkste
     Signalquelle: die meisten Kunden-/Haushalts-Links sind in der Praxis so dokumentiert, NetBox
     modelliert für sie selten ein eigenes Fernende-Gerät.
   - Sonst wird das **Kabel-Fernende** über `cable_port().walk_cable(...)` verfolgt (läuft auch
     durch Patch-Panel-Ketten, `FrontPort`/`RearPort`). Gibt es **genau ein** eindeutiges Fernende,
     wird daraus (in dieser Reihenfolge) der erste greifbare Teil genommen:
     1. dessen eigener Port-Kurzname, falls das Fernende selbst ein externer Port ist,
     2. sonst der Gerätename des Fernende-Devices,
     3. sonst der rohe Interface-Name des Fernende-Ports.
     Gibt es **kein** oder **mehrere** Fernenden (Mehrdeutigkeit), wird dieser Teil weggelassen.
3. Ist am Ende **gar kein** Teil übrig (kein Port, kein Label, kein eindeutiges Fernende), fällt
   der Generator auf den rohen NetBox-Interface-Namen zurück (`to_case(Case::Kebab)`).

Design-Hinweis aus dem Code: Es wird bewusst nur der **erste** passende Fernende-Deskriptor
verwendet, nicht mehrere kombiniert — eine frühere Version erzeugte redundante Drei-Teil-Namen wie
`e02-hegglin-ether1` statt des beabsichtigten `e02-hegglin`.

## VLAN-Namen (`generate_vlan_name`)

- Hat die NetBox-VLAN einen Namen: `vlan-{name-kebab}-{numerische-id}`, z.B. `vlan-gaeste-120`.
- Sonst: `vlan-{numerische-id}`.
- Die numerische ID selbst kommt entweder direkt aus NetBox, oder ist — falls die VLAN-Ebene keine
  echte NetBox-VLAN-ID hat (z.B. die synthetische "kein VLAN"-Ebene) — eine von `L2Setup::new`
  vergebene **synthetische** ID ab `60000` aufwärts, die garantiert nicht mit einer bereits
  verwendeten realen VLAN-ID kollidiert (siehe `next_synthetic_vlan_id` in
  [l2-generation.md](l2-generation.md)).

## VXLAN-Namen (`generate_vxlan_name`)

- `vxlan-{l2vpn-name-kebab}`, z.B. `vxlan-client-berg` für das L2VPN `client-berg` — so heißen die
  VXLAN-Interfaces auch in den echten Exporten (`examples/05`, `08`, `09`).
- Hat das L2VPN keinen Namen: `vxlan-{numerische-vxlan-id}`.
- Namen sind gerätelokal; für die Interoperabilität zählt nur die VNI.

## Bridge-Namen (`generate_main_bridge`)

- Gibt es ein eindeutiges Root-Device für die Bridge (die in NetBox modellierte Bridge, deren
  Mitglieder die Ports sind): `bridge-{bridgename-kebab}`.
- Sonst (die Standard-Bridge des Geräts, siehe
  [l2-generation.md](l2-generation.md#standard-bridge)): schlicht `bridge`.

## Sonderfall: `CAPS_BRIDGE_NAME`

Auf einem WLAN-Access-Point heißt die Standard-Bridge nicht `bridge`, sondern fest `bridge-caps`
(Konstante `CAPS_BRIDGE_NAME` in `device/ros/mod.rs`). Auf diese Bridge zeigt der CAP-Datapath
(`/interface/wifi/datapath bridge=bridge-caps`, siehe `examples/08-wlan-ap-wifi-new`). Weil die
WLAN-Clients, das VXLAN und lokale LAN-Ports desselben Haushalts-VLANs im selben L2 liegen müssen,
ist das genau die Standard-Bridge und keine zusätzliche zweite Bridge. Der Name wird deshalb nicht
über `NameGenerator` erzeugt.

## Toter Code, nicht zu verwechseln

`InterfaceAccess::interface_name()` (in `topology/access/interface.rs`) ist eine ältere, einfachere
Namensgenerierung (Port-Kurzname + sanitiertes Label, inkl. Umlaut-Transliteration ä→ae etc.), hat
aber **keine Aufrufstellen mehr** im Code — sie wurde durch `EndpointNameGenerator` ersetzt. Nicht
als aktuelle Namenslogik missverstehen.
