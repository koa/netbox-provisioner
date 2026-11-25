# Gerätemetriken

Der Provisioner liefert an `/metrics` (Management-Port) die Werte der Geräte selbst: WLAN-Clients
und Funk, Health, PoE, System, Interfaces, Ethernet-Links und DHCP-Leases. Das übernimmt, was
bisher der [mikrotik-metrik-exporter](https://github.com/koa/mikrotik-metrik-exporter), ein
zweiter MikroTik-Exporter (Metriken `mikrotik_system_*`, `mikrotik_monitor_*` mit Label `name`)
und SNMP (`mtxrWl*`) lieferten (C14); sie und der eigene `prometheus`-Benutzer auf den Geräten
sind danach nicht mehr nötig. Der Provisioner liest mit seinen eigenen Zugangsdaten
(`mikrotik_credentials`). Dazu die Strecken
zwischen den OSPF-Routern aus der LSDB und der Ping zum Gateway des Uplinks (C8, siehe
[Überwachung](concepts/monitoring.md)).

## Sammeln

- **Jeder Scrape liest** (`crate::metrics::scrape`, eigener Handler für `/metrics` am
  mgmt-Port) jedes Gerät, das in NetBox RouterOS und Zugangsdaten hat, alle zugleich, über ein
  Login pro Gerät. Der Scrape antwortet, bevor Prometheus aufgibt: 1 s vor dem Timeout, das
  Prometheus mitschickt (`X-Prometheus-Scrape-Timeout-Seconds`, Vorgabe dort 10 s), höchstens
  nach 20 s (`DEVICE_TIMEOUT`, der ServiceMonitor wartet 25 s). Ein Gerät, das bis dahin nicht
  fertig ist, zählt in diesem Scrape als nicht erreicht (`mikrotik_up` 0, sonst keine Serie);
  ein gescheiterter Scrape nähme dagegen allen Geräten ihre Serien.
  `device_metrics: false` liest keine Geräte, `/metrics` hat dann nur die eigenen Metriken.
- Gelesen wird über denselben Weg wie überall (`DeviceAccess::connect`), jede Tabelle als Fakt des
  [Gerätezustands](device-state.md): Health, Ressourcen und PoE liest der Scrape wie die
  Netz-Karte (`network_state::device_data`) und teilt die Tabellen mit ihr, WLAN-Clients, Radios,
  Interfaces, Ethernet-Links und Leases liest `device::metrics`. Ein Scrape nimmt nur Fakten,
  die nach seinem Beginn begonnen haben
  (`Freshness::Since`): Nichts ist älter als der Scrape, und eine Lesung, die seither schon
  läuft (ein zweiter Scrape, die Netz-Karte), wird mitgenommen, so fragt ein Scrape jedes Gerät
  einmal. Dauer und Erfolg der Lesungen stehen deshalb auch in
  `device_query_duration_seconds` (`kind="device_metrics"`).
- Es wird nur gelesen, jede Tabelle mit ihrem Typ aus mikrotik-model; die Monitore
  (`/interface/ethernet/monitor` und `/interface/wireless/monitor` mit `once`, alle Ports in
  einem Befehl) sind Aktionen seines Katalogs ([Zugriff auf die Geräte](device-access.md)). Ein
  Tabelle, die ein Gerät nicht hat (kein `wifi`- oder `wireless`-Paket, kein Legacy-CAPsMAN), hat
  keine Zeilen (`access::collect_if_present`, `is_missing_table`); jeder andere Fehler macht das Gerät für den Scrape unerreicht.

## Metriken

Jede Metrik hat `device` (Name in NetBox) und `site` (Site in NetBox). Die Namen folgen den
Prometheus-Konventionen: Einheit im Namen, `_total` für einen Zähler des Geräts (RouterOS zählt,
`rate()` rechnet). Was ein Gerät nicht meldet, fehlt, statt 0 zu sein.

| Metrik | Typ | weitere Labels | woher |
|---|---|---|---|
| `mikrotik_up` | Gauge | | 1, wenn das Gerät den Scrape beantwortet hat |
| `mikrotik_system_info` | Gauge (1) | `version`, `board` | `/system/resource`, `board-name` |
| `mikrotik_system_uptime_seconds` | Gauge | | |
| `mikrotik_system_cpu_load_ratio` | Gauge | | `cpu-load` / 100 |
| `mikrotik_system_memory_free_bytes`, `…_total_bytes` | Gauge | | |
| `mikrotik_system_disk_free_bytes`, `…_total_bytes` | Gauge | | `free-hdd-space`, `total-hdd-space` |
| `mikrotik_health_temperature_celsius` | Gauge | `name` | `/system/health`, nach `type`: `C` |
| `mikrotik_health_voltage_volts` | Gauge | `name` | `V` |
| `mikrotik_health_current_amperes` | Gauge | `name` | `A` |
| `mikrotik_health_power_watts` | Gauge | `name` | `W` |
| `mikrotik_health_fan_rpm` | Gauge | `name` | `RPM`; Zustände ohne Einheit fehlen |
| `mikrotik_poe_out_status` | Gauge (1) | `interface`, `status` | `/interface/ethernet/poe/monitor`, `poe-out-status` |
| `mikrotik_poe_out_voltage_volts` | Gauge | `interface` | |
| `mikrotik_poe_out_current_amperes` | Gauge | `interface` | RouterOS meldet mA |
| `mikrotik_poe_out_power_watts` | Gauge | `interface` | |
| `mikrotik_interface_running` | Gauge | `interface`, `type` | `/interface` |
| `mikrotik_interface_{rx,tx}_{bytes,packets}_total` | Counter | `interface` | |
| `mikrotik_interface_{rx,tx}_{errors,drops}_total` | Counter | `interface` | nur wo gemeldet |
| `mikrotik_interface_link_downs_total` | Counter | `interface` | |
| `mikrotik_ethernet_link_up` | Gauge | `interface` | `/interface/ethernet/monitor`, `status` `link-ok` |
| `mikrotik_ethernet_rate_bits_per_second` | Gauge | `interface` | `rate`, nur mit Link |
| `mikrotik_ethernet_rx_fcs_errors_total` | Counter | `interface` | `/interface/ethernet`, `rx-fcs-error`: Kabel oder Stecker |
| `mikrotik_ethernet_rx_overflows_total` | Counter | `interface` | `rx-overflow`, nur wo gemeldet |
| `mikrotik_ethernet_{rx,tx}_drops_total` | Counter | `interface` | `rx-drop`; `tx-drop`, sonst `tx-drop-packet`; nur wo gemeldet |
| `mikrotik_ethernet_collisions_total` | Counter | `interface` | `tx-collision`, sonst `tx-total-collision`: Duplex falsch |
| `mikrotik_wireless_radio_info` | Gauge (1) | `interface`, `ssid`, `band`, `wlan_group`, `ap`, `capsman` | laufende Radios von `/interface/wireless` und, am Controller, der CAPs von `/caps-man` |
| `mikrotik_wireless_radio_frequency_hertz` | Gauge | `interface` | `monitor`, `channel`; am Controller `current-channel` |
| `mikrotik_wireless_radio_noise_floor_dbm` | Gauge | `interface` | `monitor`, `noise-floor` |
| `mikrotik_wlan_client_{rx,tx}_{bytes,packets}_total` | Counter | siehe unten | Registrierungstabellen |
| `mikrotik_wlan_client_signal_dbm` | Gauge | siehe unten | |
| `mikrotik_wlan_client_signal_to_noise_db` | Gauge | siehe unten | nur `/interface/wireless` |
| `mikrotik_wlan_client_uptime_seconds` | Gauge | siehe unten | |
| `mikrotik_dhcp_lease` | Gauge (1) | `server`, `address`, `mac_address`, `hostname` | gebundene Leases |
| `mikrotik_link_up` | Gauge | `interface`, `peer_device`, `peer_interface`, `kind` | 1, wenn beide Router eines NetBox-Kabels oder einer Funkstrecke (`kind` `cable`, `radio`) in OSPFv2 auf dem Segment sind; aus der LSDB |
| `mikrotik_link_ipv6_up` | Gauge | wie `mikrotik_link_up` | 1, wenn sie auch in OSPFv3 Nachbarn sind |
| `mikrotik_uplink_ping_loss_ratio` | Gauge | `gateway` | Anteil verlorener Pings zum Gateway des Uplinks, 0 bis 1 |
| `mikrotik_uplink_ping_rtt_seconds` | Gauge | `gateway` | mittlere Laufzeit der beantworteten; fehlt ohne Antwort |

**Strecken** (`mikrotik_link_*`) liest der Scrape einmal aus der LSDB des ersten Routers, der
antwortet, wie die [Netz-Karte](network-map.md) (`network_state::edges`); `device` und `site` sind
die des einen Endes, `peer_*` das andere. Eine Strecke, deren eines Ende kein OSPF-Router mit
Adresse dort ist (ein Switch auf L2, ein Kunden-Port), fehlt; antwortet kein Router, fehlen
alle.

**Uplink** (`mikrotik_uplink_ping_*`): Ein Router mit einem Interface mit Tag `internet` pingt
pro Scrape 5-mal im Abstand von 200 ms den nächsten Hop seiner aktiven IPv4-Default-Route der
Tabelle `main` (`immediate-gw`; `device::uplink`), nach seinen übrigen Lesungen. Gemessen wird
der Uplink, nicht das Internet dahinter (Z14). Ohne aktive Default-Route fehlen beide.

**WLAN-Clients** stehen beim Gerät, dessen Registrierungstabelle sie hat: beim Controller für
seine CAPs, beim AP für seine eigenen Radios, bei einer Funkstrecke beim AP-Ende. Labels:
`interface` (wo er registriert ist), `wlan_group` (siehe unten), `ap` (Identity des CAPs;
leer für ein eigenes Radio),
`ssid`, `capsman` (`wifi` für `/interface/wifi`, `legacy` für `/caps-man`, `none` für ein
Radio von `/interface/wireless` ohne CAPsMAN: Funkstrecke, Standalone-AP),
`mac_address`, `client_hostname` und `client_ip` aus einem gebundenen Lease irgendeines Geräts
mit derselben MAC (der Lease liegt meist auf dem Router, nicht auf dem AP). Ein Client an einem
virtuellen Interface gehört zum Radio seines Master-Interfaces.

**Radios eines Legacy-CAPsMAN:** Auf dem CAP hat ein Radio, das der CAPsMAN verwaltet, keine
eigene SSID (ap-klr-solar); es hat dort Frequenz und Rauschpegel, aber kein `info`, und seine
Clients fehlen dort. Beides kommt vom Controller: `/caps-man/interface` (laufend,
`current-channel`), SSID und Band aus `/caps-man/actual-interface-configuration` (die Zeilen
haben auch die Passphrase; sie bleibt im Speicher und wird nirgends ausgegeben), der CAP aus `/caps-man/radio`; ein
virtuelles Interface hat CAP und Band seines Masters. Die Radios von `/interface/wifi` fehlen
noch in `mikrotik_wireless_radio_*`.

**WLAN-Gruppe** (`wlan_group`, an WLAN-Clients und `mikrotik_wireless_radio_info`): der Name
der Wireless LAN Group in NetBox, deren Controller oder AP das Gerät ist, mit einem WLAN dieser
SSID und, bei einem Client eines CAPs, diesem CAP unter ihren APs (die Identity des CAPs ist sein
NetBox-Name). Passen keine oder mehrere, bleibt es leer, etwa bei einer Funkstrecke.

## Von den Exportern

| bisher | Provisioner |
|---|---|
| Label `hostname` (Identity), `name`, `instance` | `device` (NetBox), dazu `site` |
| `mikrotik_exporter_wlan_registration_{tx,rx}_{bytes,packets}` | `mikrotik_wlan_client_{tx,rx}_{bytes,packets}_total` |
| `mikrotik_exporter_wlan_registration_rx_signal`, `mtxrWlRtabStrength`, `mtxrWlCMRtabRxStrength` | `mikrotik_wlan_client_signal_dbm` |
| `mtxrWlRtabSignalToNoise` | `mikrotik_wlan_client_signal_to_noise_db` |
| `mikrotik_exporter_wlan_registration_uptime` | `mikrotik_wlan_client_uptime_seconds` |
| Labels `cap_identity`, `caps` | `ap`, `capsman` |
| `mtxrWlRtabEntryCount`, `mtxrWlCMRtabEntryCount` | `count(mikrotik_wlan_client_uptime_seconds)` nach `capsman` |
| `mtxrWlApSsid`, `mtxrWlApBand`, `mtxrWlApNoiseFloor` | `mikrotik_wireless_radio_{info,noise_floor_dbm}`, dazu die Frequenz |
| `mikrotik_exporter_health_{temperature,voltage,current,power,rpm}`, `mikrotik_health_*` | `mikrotik_health_{temperature_celsius,voltage_volts,current_amperes,power_watts,fan_rpm}` |
| `mikrotik_exporter_ethernet_poe_out_{voltage,current,power}`, `mikrotik_poe_wattage` | `mikrotik_poe_out_{voltage_volts,current_amperes,power_watts}`; der Strom in A statt mA |
| `mikrotik_system_{uptime,cpu_load,free_memory,total_memory,free_hdd_space,total_hdd_space}` | `mikrotik_system_{uptime_seconds,cpu_load_ratio,memory_free_bytes,memory_total_bytes,disk_free_bytes,disk_total_bytes}`; die Last 0 bis 1 |
| Labels `version`, `boardname` | `mikrotik_system_info{version,board}` |
| `mikrotik_interface_{rx,tx}_{byte,error}` | `mikrotik_interface_{rx,tx}_{bytes,errors}_total` |
| `mikrotik_monitor_status`, `mikrotik_monitor_rate` | `mikrotik_ethernet_link_up`, `mikrotik_ethernet_rate_bits_per_second` |
| `mikrotik_dhcp_leases_metrics`, `mikrotik_dhcp_leases_active_count` | `mikrotik_dhcp_lease`, `count(mikrotik_dhcp_lease)` |

Neu sind `mikrotik_up` und der PoE-Status. Der Blackbox-Ping (`probe_icmp_*`) gehört zum
Uplink-Ping von C8.

## Dashboards

In `charts/netbox-provisioner/dashboards/`, mit `grafanaDashboard.enabled` ConfigMaps für den
Grafana-Sidecar wie `device-queries.json`, beide mit den Variablen Datenquelle, Site und Gerät:

- **Mikrotik Übersicht** (`mikrotik-overview.json`, aus „Mikrotik Exporter“): System, Netz,
  Ports, Leases, Funk und Signal der Clients. „Used memory“ zeigte den Speicherplatz und
  heisst nun „Used storage“; der Verkehr ist in Bit/s (vorher Byte/s mit der Einheit bps). Weg
  sind der Ping (C8) und ein Zabbix-Panel ohne Daten. „Interface status“ ist eine Tabelle
  (Port, Status, Rate, nach Port sortiert) statt der zwei für Link und Rate. Die Tabellen fassen
  die Serien nach ihren eigenen Labels zusammen (`max by (interface)`, bei den Leases
  `by (address, …)`): Die Ziel-Labels, die Prometheus anhängt (`namespace`, `pod`, `service`
  …), sind keine Spalten.
- **Mikrotik Geräte** (`mikrotik-health.json`): Health, PoE pro Port (ein Panel je Port) und das
  Log aus Loki mit Zeitstempeln (Variable `loki`), dessen Label `hostname` die Identity des Geräts ist, also sein
  NetBox-Name. Die Logins des Provisioners blendet es aus: Variable „Benutzer des
  Provisioners“ (Vorgabe `admin`), einige pro Gerät und Scrape.
- **Mikrotik Anschlüsse** (`mikrotik-interfaces.json`): Verkehr, Pakete, verworfene Pakete,
  Fehler und Link-Abbrüche jedes Anschlusses, für mehrere Geräte und Anschlüsse auf einmal
  (Variablen Gerät und Anschluss, je mit „All“), empfangen nach unten, gesendet nach oben. Oben
  eine Tabelle mit den Summen über den Zeitraum, eine Zeile pro Anschluss, nach Spalten
  sortierbar. Verworfen zählt bei Ethernet-Ports der Port selbst (`mikrotik_ethernet_*`), sonst
  `/interface`: Auf den Ports eines Switch-Chips meldet `/interface` weder Drops noch Fehler.

## Geprüft

Im Lab (6 Geräte, 7.24.6): `mikrotik_up`, System, Health, PoE, Interfaces und Ethernet-Links
aller Geräte, Radio und Client der Funkstrecke lab-sxt-a → lab-sxt-b (`capsman="none"`, Signal,
Signal/Rauschen); der PoE-Strom passt zu Spannung und Leistung (lab-np2 → lab-wap:
47,6 V × 0,051 A = 2,4 W). Die Dashboards mit Grafana 13.3 und Prometheus 3.16 gegen den
Lab-Provisioner: jedes Panel mit Daten, ausser Leases, Lüfter und dem Log (kein Loki dort).
Die Port-Zähler von `/interface/ethernet` (2026-11-16, 7 Geräte): jedes Board liest die Tabelle
getypt; lab-np2 (CRS318), lab-crs125 und lab-crs226 melden `rx-overflow` und `tx-drop-packet`,
lab-hex und die SXT `rx-drop` und `tx-drop`, lab-wap nur FCS-Fehler und Kollisionen. „Mikrotik
Anschlüsse“ zeigt so auch die Ports von lab-np2, wo `/interface` nichts meldet.
Im Lab gibt es weder Clients von `/interface/wifi` oder `/caps-man` noch einen gebundenen Lease:
diese sind nur mit Unit-Tests geprüft (`metrics::test`, `device::metrics::test`).

Auf einem CHR (`chr_network::wifi_country`, 7.24.6, mit dem `wireless`-Paket): Der Legacy-CAPsMAN
schreibt das Land klein (`switzerland`, `WifiCountryLower`) in `/caps-man/configuration`,
`/caps-man/interface` und `/caps-man/actual-interface-configuration`, der Wifi-CAPsMAN gross
(`Switzerland`, `/interface/wifi/configuration`); beide lehnen die andere Schreibweise ab. Ein
CAPsMAN-Interface ohne gebundenen CAP lässt sich lesen, obwohl ihm Zustand, Raten und
`disabled` fehlen.
