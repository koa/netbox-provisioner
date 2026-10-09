# Funkbrücken-Generierung (5 GHz und 60 GHz)

Code: `backend/src/device/ros/mod.rs` (`WirelessDeviceDataTarget::generate_from`,
`W60GDeviceDataTarget::generate_from`), `backend/src/device/ros/hw_facts.rs` (Boards, Funk-Ports),
`backend/src/device/ros/l2/mod.rs` (`L2Setup::placement_of_port`), Modelle in `mikrotik-model`
(`ros_model/interface.txt` `/interface/wireless`, `ros_model/interface/W60g.txt`).
Referenz-Konfigurationen: `examples/06-wlan-bridge-p2p`.

## Aufbau einer Brücke

Eine Funkbrücke ist eine transparente L2-Strecke: eine flache Bridge aus `ether1` und dem Funk-Port,
auf beiden Seiten. Das L2 dafür entsteht wie überall aus dem NetBox-Modell (siehe
[l2-generation.md](l2-generation.md)). Der Funk-Port wird dabei wie ein Ethernet-Port behandelt und
bekommt einen eigenen Namen (`w01`).

Zwei RouterOS-Funktypen kommen vor, je nach Hardware:

| NetBox-Port | RouterOS                                    | Board (bekannt)       |
|-------------|---------------------------------------------|-----------------------|
| `wlan<N>`   | `/interface/wireless` (5 GHz, Nv2)          | `RBDynaDishG-5HacDr3` |
| `wlan60-<N>`| `/interface/w60g` (60 GHz)                  | `RBwAPG-60ad-A`       |

Weitere Boards brauchen einen Eintrag in `hw_facts.rs` (Ethernet-Ports und Funk-Ports), sonst
findet der Provisioner keine Ports.

## Was generiert wird

Jeder Funk-Port, den das L2 benutzt, wird umbenannt (`set [find default-name=wlan1] name=w01`). Hat
er in NetBox eine **RF Role**, kommen die Funk-Einstellungen dazu. Alle anderen Felder des Funk-
Interfaces sind im Modell keep-if-none (`k`): Band, Land, Scan-Liste, Region, Sektor usw. bleiben
wie auf dem Gerät. Der Provisioner kennt die Standardwerte der Funk-Hardware nicht und überschreibt
sie deshalb nicht.

### 5 GHz (`/interface/wireless`)

| Feld                | AP (`RF Role = AP`)            | Station (`RF Role = Station`)        |
|---------------------|--------------------------------|--------------------------------------|
| `mode`              | `bridge`                       | `station-bridge`                     |
| `wireless-protocol` | `nv2`                          | `nv2-nstreme-802.11`                 |
| `frequency`         | RF Channel Frequency, sonst `auto` | unverändert (folgt dem AP)       |
| `channel-width`     | `20/40/80mhz-Ceee` usw.        | `20/40/80mhz-XXXX` usw.              |
| `ssid`              | SSID des Wireless Links        | SSID des Wireless Links              |
| `nv2-security`, `nv2-preshared-key` | `enabled`, PSK des Links | `enabled`, PSK des Links |

Die Protokolle entsprechen den produktiven Strecken (`hegglin-bachmann`/`bachmann-hegglin-2`). Der
AP spricht nur Nv2, deshalb verschlüsselt Nv2 die Strecke (`nv2-security`), nicht das
WPA-Sicherheitsprofil. Das Profil wird nicht verwaltet: das Standardprofil `default` lässt sich nicht
löschen, und für eine Nv2-Strecke spielt es keine Rolle.

Die Kanalbreite wird nur gesetzt, wenn NetBox eine hat (5, 10, 20, 40, 80 oder 160 MHz; sonst
`UnsupportedChannelWidth`). Der AP setzt den Steuerkanal an den Anfang (`C`), die Station folgt mit
`X` dem Layout des APs.

### 60 GHz (`/interface/w60g`)

| Feld                     | AP                       | Station                  |
|--------------------------|--------------------------|--------------------------|
| `mode`                   | `bridge`                 | `station-bridge`         |
| `frequency`              | RF Channel Frequency, sonst `auto` | unverändert    |
| `ssid`, `password`       | SSID und PSK des Links   | SSID und PSK des Links   |
| `put-stations-in-bridge` | die Bridge des Funk-Ports | die Bridge des Funk-Ports |

Auf der AP-Seite erscheint jede Gegenstelle als eigenes, dynamisches Interface
(`/interface/w60g/station`, z.B. `wlan60-station-2`). Diese werden nicht modelliert: über
`put-stations-in-bridge` kommen sie selbst in die Bridge.

### Ohne PSK

Hat der Wireless Link keinen PSK (oder liefert NetBox keinen, wie im gespeicherten Export ohne
Geheimnisse), bleiben Schlüssel, Passwort und `nv2-security` unverändert.

## Erkennung beim Abruf

`DeviceCfg::fetch_with_board_model` liest `/interface/wireless` nur, wenn das Paket `wireless`
installiert ist, und `/interface/w60g` nur auf einem Board mit 60-GHz-Ports (`build_w60g_ports`).
Ein Funk-Port, den NetBox nennt, das Board aber nicht hat, ergibt `PortNotFound`.

## Prüfung

CHR hat weder `/interface/wireless` noch `/interface/w60g`. Die Modelle sind deshalb mit
statischen Decode-Tests aus echten Geräte-Ausgaben geprüft (`mikrotik-model`
`src/model/wireless_tests.rs`), die Generierung mit `device/ros/radio_test.rs` und dem
Produktions-Export (`netbox_example_test.rs`).

## Ausrollen

Eine geänderte SSID oder ein neuer Schlüssel trennt die Strecke, bis beide Seiten ihn haben. Wird
eine Seite nur über die Strecke selbst erreicht, zuerst diese ferne Seite ändern, dann die nahe.
Sonst ist die ferne Seite nach der ersten Änderung nicht mehr erreichbar.
