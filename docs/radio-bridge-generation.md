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
bekommt einen eigenen Namen (`w01`); ein 60-GHz-Radio bleibt `wlan60-<N>`.

Erreichbar ist eine Brücke auf zwei Arten:

- **Als OSPF-Router** (das Ziel): das NetBox-Interface `bridge` hat eine statische Adresse im Netz
  des Router-Ports und das Tag `ospf`, `lo` die Loopback (Primary IP, Router-ID). OSPF läuft nur
  für das Management über die Bridge, die Nutzdaten bleiben auf L2 (`test_radio_bridge_with_a_static_address_runs_ospf`).
- **Rein L2**: ohne IP-Adresse in NetBox bekommt ihre Bridge automatisch einen DHCP-Client
  (`setup_ip_addresses`).

Eine Loopback ohne `ospf` an irgendeinem Interface hebt das reine L2 auf (kein DHCP-Client mehr),
ohne dass jemand sie ankündigt: das Gerät ist danach nicht erreichbar (schopf-burri). Der Befund
`LoopbackWithoutOspf` meldet das als Fehler. Der Helfer „Funkbrücke“ trägt beides in NetBox ein
([config-helper.md](concepts/config-helper.md#funkbrücke)).

Zwei RouterOS-Funktypen kommen vor, je nach Hardware:

| NetBox-Port | RouterOS                                    | Board (bekannt)       |
|-------------|---------------------------------------------|-----------------------|
| `wlan<N>`   | `/interface/wireless` (5 GHz, Nv2)          | `RBDynaDishG-5HacDr3`, `SXT G-5HPacD` |
| `wlan60-<N>`| `/interface/w60g` (60 GHz)                  | `RBwAPG-60ad`, `RBwAPG-60ad-A`, `RBLHGG-60ad` |

Weitere Boards brauchen einen Eintrag in `hw_facts.rs` (Ethernet-Ports und Funk-Ports), sonst
findet der Provisioner keine Ports.

## Was generiert wird

Jeder Funk-Port, den das L2 benutzt, wird umbenannt (`set [find default-name=wlan1] name=w01`).
Als Bridge-Port bekommt er kein `hw`: Funk hat keinen Hardware-Offload, und RouterOS meldet das Feld
dort nicht (sonst erschiene es bei jedem Abgleich als Änderung).

**Ohne RF Role** wird der Funk-Port nicht als Funk verwaltet: der Generator übernimmt seine
aktuelle Konfiguration unverändert als Soll (z.B. das CAPsMAN-Radio eines `CRS109`).

**Mit RF Role** legt der Generator jedes Feld fest, das der Funk-Port meldet. Grundlage ist ein
Profil pro Board (`hw_facts.rs`: `wireless_bridge_port`, `w60g_port`), darauf kommen Rolle, Kanal,
Wireless Link und Funkregeln. Reste früherer Versuche auf dem Gerät gehen dabei verloren. Nur was
von der Hardware kommt (`mac-address`, `l2mtu`, `radio-name`), bleibt, wie es ist (`k` im Modell).
Ein Board ohne Profil (`CRS109`) ergibt mit RF Role `NoRadioProfile`.

### 5 GHz (`/interface/wireless`)

Das Profil sind die Werkseinstellungen, wie sie `link-5-ap` zeigt, mit diesen Abweichungen für
eine Strecke mit genau einer Gegenstelle: `wps-mode=disabled`, `wds-mode=disabled` (Nv2 mit
`station-bridge` braucht kein WDS), `station-roaming=disabled`, `tdma-period-size=auto`. Dazu pro
Board die eingebaute Antenne (`antenna-gain`: DynaDish 25, SXT 14 dBi) und
`installation=outdoor`.

| Feld                | AP (`RF Role = AP`)            | Station (`RF Role = Station`)        |
|---------------------|--------------------------------|--------------------------------------|
| `mode`              | `bridge`                       | `station-bridge`                     |
| `wireless-protocol` | `nv2`                          | `nv2-nstreme-802.11`                 |
| `frequency`         | RF Channel Frequency, sonst `auto` | `auto` (sucht den AP)            |
| `channel-width`     | `20/40/80mhz-Ceee` usw.        | `20/40/80mhz-XXXX` usw.              |
| `ssid`              | SSID des Wireless Links        | SSID des Wireless Links              |
| `nv2-security`, `nv2-preshared-key` | `enabled`, PSK des Links | `enabled`, PSK des Links |
| `country`, `frequency-mode` | Land der Site, `regulatory-domain` | wie AP                    |
| `tx-power-mode`, `tx-power` | TX Power: `all-rates-fixed`, sonst `default` | wie AP          |

Die Protokolle entsprechen den produktiven Strecken (`hegglin-bachmann`/`bachmann-hegglin-2`). Der
AP spricht nur Nv2, deshalb verschlüsselt Nv2 die Strecke (`nv2-security`), nicht das
WPA-Sicherheitsprofil. Das Profil bleibt `default`: es lässt sich nicht löschen, und für eine
Nv2-Strecke spielt es keine Rolle.

Ohne Kanalbreite in NetBox gilt `20/40/80mhz-XXXX` (beide Seiten wählen selbst). Mit Kanalbreite
(5, 10, 20, 40, 80 oder 160 MHz; sonst `UnsupportedChannelWidth`) setzt der AP den Steuerkanal an
den Anfang (`C`), die Station folgt mit `X` dem Layout des APs.

### 60 GHz (`/interface/w60g`)

| Feld                     | AP                       | Station                  |
|--------------------------|--------------------------|--------------------------|
| `mode`                   | `bridge`                 | `station-bridge`         |
| `frequency`              | RF Channel Frequency, sonst `auto` | `auto`         |
| `ssid`, `password`       | SSID und PSK des Links   | SSID und PSK des Links   |
| `region`                 | aus dem Land der Site (`ch` → `eu`) | wie AP        |
| `put-stations-in-bridge` | die Bridge des Funk-Ports | die Bridge des Funk-Ports |
| `isolate-stations`       | `yes`                    | `no`                     |

Das Radio wird über seinen Namen gefunden: `/interface/w60g` meldet kein `default-name` (RouterOS
7.23, schopf-burri). Es behält deshalb immer `wlan60-<N>`, auch als Bridge-Port; der
Name-Generator benennt es nicht nach der Gegenstelle um.

`tx-sector`, `arp-timeout` sind `auto`, `mtu` 1500. Auf der AP-Seite erscheint jede Gegenstelle als
eigenes, dynamisches Interface (`/interface/w60g/station`, z.B. `wlan60-station-2`). Diese werden
nicht modelliert: über `put-stations-in-bridge` kommen sie selbst in die Bridge.

### Land und Funkregeln

Das Land ist die oberste Region der Site, als ISO-Code (`DeviceAccess::country`). Eine Tabelle
(`country_settings`) übersetzt es in den RouterOS-Wert für 5 GHz (`ch` → `switzerland`) und die
Region für 60 GHz (`eu`). Ein Land, das nicht in der Tabelle steht, ergibt `UnknownCountry`; eine
Site ohne Region `MissingCountry`.

Am Wireless Link übersteuern `rf_country` (RouterOS-Wert direkt) und `rf_frequency_mode` beides.
So läuft z.B. `hegglin-bachmann` mit `country=etsi 5.5-5.7 outdoor` und
`frequency-mode=manual-txpower`.

### Schlüssel

Ein Wireless Link mit Authentication Open ergibt eine unverschlüsselte Strecke
(`nv2-security=disabled`, kein 60-GHz-Passwort). Ist er verschlüsselt, NetBox liefert aber keinen
PSK (z.B. weil dem API-Token die Rechte fehlen), bricht die Generierung mit `MissingPsk` ab, statt
die Strecke unverschlüsselt zu betreiben.

Das 60-GHz-Passwort zeigt der AP mit `print` nicht an. Liefert ihn auch die API nicht, erscheint er
bei jedem Abgleich wieder als Änderung. Das Setzen ist harmlos, weil der Wert gleich bleibt.

## Erkennung beim Abruf

`DeviceCfg::fetch_with_board_model` liest `/interface/wireless` nur, wenn das Paket `wireless`
installiert ist, und `/interface/w60g` nur auf einem Board mit 60-GHz-Ports (`build_w60g_ports`).
Ein Funk-Port, den NetBox nennt, das Board aber nicht hat, ergibt `PortNotFound`. Auf einem
60-GHz-Board gilt das auch für `wlan<N>` und `wifi<N>`: es hat nur `wlan60-<N>`, ein so benannter
Port ist sein falsch benanntes 60-GHz-Radio (ohne die Prüfung käme ein Bridge-Port auf ein
Interface, das es nicht gibt).

Ein Ende eines Wireless Links ohne RF-Rolle meldet der Befund `RadioWithoutRole` (Warnung).

## Prüfung

CHR hat weder `/interface/wireless` noch `/interface/w60g`. Die Modelle sind deshalb mit
statischen Decode-Tests aus echten Geräte-Ausgaben geprüft (`mikrotik-model`
`src/model/test/wireless.rs`, darunter die vollständige `print advanced detail` zweier Strecken),
die Generierung mit `device/ros/test/radio.rs` und dem Produktions-Export
(`device/ros/test/netbox_example.rs`). Ob RouterOS jede generierte Zeile annimmt, zeigt erst ein echtes
Gerät. Die Werte des Profils stammen aus der CLI-Ausgabe; die API kann einzelne Werte anders
schreiben, das zeigt sich dann als wiederkehrende Änderung.

## Ausrollen

Eine geänderte SSID oder ein neuer Schlüssel trennt die Strecke, bis beide Seiten ihn haben. Wird
eine Seite nur über die Strecke selbst erreicht, zuerst diese ferne Seite ändern, dann die nahe.
Sonst ist die ferne Seite nach der ersten Änderung nicht mehr erreichbar.
