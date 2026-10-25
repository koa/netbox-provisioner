# Custom Fields in NetBox

Der Provisioner liest acht Custom Fields und schreibt eines (`provisioner_job`). Alle sind optional: Ein Feld, das in NetBox fehlt oder
leer ist, schaltet nur die jeweilige Funktion ab, die Generierung bricht deswegen nicht ab. Wo ein
leeres Feld trotzdem zu einem Fehler führt, steht es unten beim Feld.

Gelesen werden sie über `custom_field_data` (bei Wireless LAN Groups `custom_fields`) per Namen,
siehe das `JSON`-Struct in `backend/src/netbox/mod.rs`. Wie die Objekte drumherum modelliert
werden, beschreibt [netbox-modeling.md](netbox-modeling.md).

## Übersicht

| Name                   | Objekt-Typ         | NetBox-Typ                      | Wofür                                       |
|------------------------|--------------------|---------------------------------|---------------------------------------------|
| `mikrotik_credentials` | Tenant             | Text                            | Zugangsdaten für die Geräte des Tenants     |
| `wlan_group`           | Device             | Object → Wireless LAN Group     | Gerät ist Access Point dieser WLAN-Gruppe   |
| `propagate_default_route` | Device          | Boolean                         | OSPF gibt die Default-Route des Geräts weiter |
| `controller`           | Wireless LAN Group | Object → Device                 | CAPsMAN-Controller der Gruppe               |
| `wlan_mgmt`            | Wireless LAN Group | Object → VLAN                   | Management-VLAN der Gruppe                  |
| `vxlan_vni`            | VLAN Group         | Integer                         | VLANs der Gruppe über VXLAN verbinden       |
| `rf_country`           | Wireless Link      | Text                            | Land einer Funkbrücke übersteuern           |
| `rf_frequency_mode`    | Wireless Link      | Selection                       | Frequency Mode einer Funkbrücke übersteuern |
| `provisioner_job`      | Journal Entry      | JSON                            | Stand eines Jobs, vom Provisioner geschrieben |

## Anlegen

### Per Import

[`netbox-import/`](netbox-import/) enthält alle Definitionen als YAML zum Importieren. Ein Feld,
das es in NetBox schon gibt, lässt den ganzen Import scheitern; dessen Abschnitt (von `---` bis
zum nächsten `---`) vorher aus der Datei löschen.

1. **Customization → Custom Field Choice Sets → Import**, Format YAML, Inhalt von
   `1-custom-field-choice-sets.yaml`.
2. **Customization → Custom Fields → Import**, Format YAML, Inhalt von `2-custom-fields.yaml`.

### Von Hand

In NetBox unter **Customization → Custom Fields → Add**:

- **Object types:** der Objekt-Typ aus der Tabelle (z.B. `Tenancy > Tenant`).
- **Name:** genau wie in der Tabelle, klein geschrieben, mit Unterstrich. Der Provisioner findet
  das Feld nur über diesen Namen. Das **Label** ist frei.
- **Type:** wie in der Tabelle. Bei *Object* zusätzlich das **Related object type**.
- **Required:** nein. Sonst verlangt NetBox den Wert an jedem Objekt dieses Typs.

Für `rf_frequency_mode` zuerst unter **Customization → Custom Field Choice Sets** eine Choice Set
anlegen (siehe unten) und sie beim Feld auswählen.

## Die Felder im Einzelnen

### `mikrotik_credentials` (Tenant, Text)

Der Name eines Zugangsdaten-Eintrags in der Provisioner-Konfiguration (`config.yaml`, Abschnitt
`settings.mikrotik_credentials`). Das Passwort selbst steht nie in NetBox.

```yaml
settings:
  mikrotik_credentials:
    standort-berg:
      user: provisioner     # optional, Standard: admin
      password: "..."
```

Am Tenant steht dann `standort-berg`. Für ein Gerät sucht der Provisioner den Tenant in dieser
Reihenfolge: Tenant des Geräts, dann der Location, dann der Site. Der erste mit gesetztem Feld
gewinnt. Ohne einen solchen Tenant ist das Gerät nicht erreichbar; im Frontend lassen sich dann
Zugangsdaten von Hand eingeben.

### `wlan_group` (Device, Object → Wireless LAN Group)

Nur bei Access Points: die WLAN-Gruppe, deren WLANs das Gerät ausstrahlt. Ein Gerät mit
`wlan_group` bekommt die CAP-Konfiguration, auch wenn es kein AP ist. Ein Switch, der nur Ports im
WLAN-Netz hat, braucht das Feld deshalb nicht; ihm reicht das VLAN an den Ports.

### `propagate_default_route` (Device, Boolean)

Nur am Router mit dem Uplink ins Internet (in der Regel der mit `nat`): Sein OSPF gibt die
Default-Route weiter, die er selbst hat (vom DHCP-Client des Uplinks oder eine statische), sobald
sie installiert ist (`originate-default=if-installed`). Alle anderen Geräte, auch bei leerem Feld,
bekommen `originate-default=never`: Hat ein Gerät einen Uplink in ein anderes Netz (z.B. ein Lab
mit Uplink ins produktive Netz), ginge dessen Default-Route sonst über OSPF in dieses Netz zurück
und zöge dort den Verkehr an. Andere statische Routen verteilt OSPF weiterhin
(`redistribute=connected,static,bgp`).

Achtung beim Umstellen: Ein Router, der bisher eine Default-Route verteilt hat, hört damit beim
nächsten Anwenden auf, solange das Feld nicht gesetzt ist.

### `controller` (Wireless LAN Group, Object → Device)

Das Gerät, das als CAPsMAN-Controller die WLANs der Gruppe verteilt. **Pflicht** für eine
WLAN-Gruppe: eine Gruppe ohne `controller` ignoriert der Provisioner komplett, samt ihren WLANs.
Pro Controller nur **eine** Gruppe; ist ein Gerät Controller mehrerer Gruppen, gewinnt eine
davon.

### `wlan_mgmt` (Wireless LAN Group, Object → VLAN)

Das VLAN, über das die Access Points der Gruppe ihren Controller erreichen. Optional.

### `vxlan_vni` (VLAN Group, Integer)

Die VNI des VXLAN, über das die VLANs dieser Gruppe zwischen Geräten getunnelt werden, z.B. `1`.
VLANs aus Gruppen ohne `vxlan_vni` bleiben lokal, auch wenn
sie auf mehreren Geräten vorkommen. Details: [VXLAN](netbox-modeling.md#vxlan).

### `rf_country` (Wireless Link, Text)

Übersteuert das Land einer [Funkbrücke](netbox-modeling.md#funkbrücke) für beide Enden. Ohne
dieses Feld nimmt jede Seite das Land aus der obersten Region ihrer Site (Slug = ISO-Code, z.B.
`ch` → `switzerland`). Gesetzt wird der RouterOS-Wert direkt eingetragen:

- 5 GHz: der Wert für `/interface/wireless country`, z.B. `switzerland` oder
  `etsi 5.5-5.7 outdoor`.
- 60 GHz: der Wert für `/interface/w60g region`, z.B. `eu` oder `no-region-set`.

Der Wert wird nicht geprüft; ein Tippfehler fällt erst beim Anwenden auf. Nötig ist das Feld
- für eine Strecke über eine Landesgrenze, weil sonst jede Seite ein anderes Land bekäme;
- für eine Site in einem Land, das der Provisioner nicht kennt (`UnknownCountry`; bekannt sind
  `ch`, `li`, `de`, `at`, `fr`, `it`);
- für eine Strecke, die bewusst außerhalb der Länderregeln läuft.

Hat die Site keine Region und ist `rf_country` leer, bricht die Generierung des Geräts ab
(`MissingCountry`).

### `rf_frequency_mode` (Wireless Link, Selection)

Übersteuert `/interface/wireless frequency-mode` für beide Enden einer 5-GHz-Funkbrücke. Leer
bedeutet `regulatory-domain`: RouterOS hält sich an die Kanäle und Sendeleistungen des Landes. Bei
60 GHz hat das Feld keine Wirkung.

Choice Set (**Customization → Custom Field Choice Sets → Add**), Werte und Labels gleich:

| Wert                | Wirkung                                                         |
|---------------------|-----------------------------------------------------------------|
| `regulatory-domain` | Länderregeln (wie leer)                                         |
| `manual-txpower`    | Kanäle des Landes, Sendeleistung ohne Begrenzung durch das Land |
| `superchannel`      | alle Kanäle der Hardware, keine Länderregeln                    |

Ein anderer Wert bricht die Generierung ab (`InvalidFrequencyMode`). Mit `manual-txpower` oder
`superchannel` sendet die Strecke in der Regel außerhalb des Zulässigen. Das ist für Ausnahmen
gedacht, nicht für den Normalfall.

Beispiel `hegglin-bachmann`: `rf_country` = `etsi 5.5-5.7 outdoor`, `rf_frequency_mode` =
`manual-txpower`.

### `provisioner_job` (Journal Entry, JSON)

Schreibt der Provisioner selbst, in jeden seiner Einträge im Journal eines Geräts
([Protokoll](protocol.md)): die Art des Jobs (`job`), wie er steht oder endete (`outcome`:
`Running`, `Done`, `Incomplete`, `Failed`, `Cancelled`, `Interrupted`), der Zustand, in dem er
ist oder endete (`state`, z.B. `SCRIPT_RUNNING`), was er auf dem Gerät lässt (`left`:
`ApplyFallback`, `UpdateFallback`, `WatchdogSafetyNet`) und wann der Eintrag geschrieben wurde
(`at`, Unix-Sekunden). Von Hand nichts eintragen. Fehlt das Feld in NetBox, lehnt NetBox jeden
Eintrag ab, und jeder Job meldet „Eintrag im NetBox-Journal nicht geschrieben“.
