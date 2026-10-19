# Konzept: Self-Service für Anwohner

Status: Konzept, nicht umgesetzt. User Journey „Self-Service“ und Personas aus
[pending.md](../pending.md#self-service).

## Ausgangslage

- **Zugang:** OIDC (Pocket-ID produktiv, Keycloak im Lab). Hinein darf, wer mit seinem `sub` in
  `auth_users` steht; alle, die hinein dürfen, dürfen alles. `allow_apply` und
  `allow_netbox_write` gelten für den ganzen Server ([Login](../login.md), `WritesAllowed`).
- **Besitz in NetBox:** Tenants gibt es schon, gebraucht für die Zugangsdaten
  (`mikrotik_credentials`, gesucht am Gerät, an der Location, an der Site). VLANs, Wireless LANs
  und Geräte haben in NetBox ein Feld Tenant; Interfaces nicht.
- **WLAN:** SSID und PSK stehen am Wireless LAN in NetBox; der Generator verteilt sie über CAPsMAN
  auf die APs der Gruppe ([WLAN](../netbox-modeling.md#wlan)).

## Personas

| Persona | darf |
|---|---|
| Admin | alles wie heute |
| Anwohner | seine WLANs (SSID, PSK), seine Ports (VLAN wählen, AP anschliessen), zusätzliche APs in seine WLAN-Gruppe aufnehmen, seine eigenen APs ändern und entfernen, die Leases seiner Netze sehen |

Ein Anwohner ist **ein Tenant** in NetBox (eine Usergruppe: Haushalt, Firma). Ihm gehört, was den
Tenant trägt: VLANs, Wireless LANs, Geräte. APs sind RouterOS-Geräte wie die Router; ein AP mit
seinem Tenant gehört ihm, und er darf ihn ändern und entfernen. Ein AP ohne Tenant oder mit einem
anderen (der des Netzbetreibers) bleibt den Admins. Ports haben keinen Tenant: Ein Port gehört ihm, wenn
sein Gerät ihm gehört, oder über ein Custom Field `owner` (→ Tenant) am Interface für einzelne
Ports eines fremden Switches.

## Wer ist wer

- **Rolle aus dem Token:** Der OIDC-Provider liefert Gruppen (`groups`-Claim; Pocket-ID kann
  das). Gruppe `admin` → Admin. Für Anwohner nennt ein Custom Field am Tenant (`oidc_group`) die
  Gruppe. Damit pflegt man die Zuordnung an einer Stelle (Provider) und den Besitz in NetBox.
- `auth_users` bleibt als Notschalter für Admins, bis die Gruppen verlässlich sind.

## Was der Server prüft

Die Grenze bleibt wie in `CLAUDE.md`: Entschieden wird einmal, an der Schnittstelle. Statt eines
Guards „darf schreiben“ bekommt jede Mutation die Frage „darf dieser Benutzer **dieses Objekt**
ändern“. Anwohner bekommen eigene, enge Mutationen statt Zugriff auf die Admin-Helfer:

- `setWlan(wlan, ssid, psk)`: nur ein Wireless LAN seines Tenants;
- `setPortVlan(port, vlan)`: Port und VLAN seines Tenants (VLAN untagged am Port);
- `adoptAp(device, group)`: Gerät und WLAN-Gruppe seines Tenants;
- `updateAp(device, …)`: an seinem eigenen AP Name/Label, WLAN-Gruppe (nur eigene) und die
  VLANs seiner Ports (nur eigene), dann Anwenden auf AP und Controller;
- `removeAp(device)`: sein eigener AP aus NetBox entfernen (Gerät, Kabel, Adressen, wie
  „Verbindung abräumen“ mit Rückgängig) und die WLAN-Gruppe ohne ihn auf den Controller anwenden.
  Das Gerät selbst läuft weiter, bis es abgesteckt wird; die Seite sagt das, statt es aus der
  Ferne zurückzusetzen;
- `leases(vlan)`: lesen, nur seine Netze.

Jede schreibt nach NetBox (Plan, Vorschau, Rückgängig wie bei den [Helfern](config-helper.md)) und
wendet danach **nur die betroffenen Geräte** an (für ein WLAN: der Controller der Gruppe). Ein
Anwohner sieht die Vorschau als Liste in Worten („SSID auf allen 3 APs ändern“), nicht als Skript.

## Leases

`/ip/dhcp-server/lease` auf dem Router des Netzes, live gelesen (Modell in `mikrotik-model`
nötig). Nur Leases von Servern auf Interfaces in den VLANs/Netzen des Tenants. IPv6 hat keine
Leases (SLAAC); dafür ginge `/ipv6/neighbor`, als Ergänzung.

## Risiken

- Ein Anwohner, der einen Port umstellt, kann das Netz dahinter abschneiden (Uplink eines
  Switches). Deshalb nur Ports ohne Kabel zu einem RouterOS-Gerät und ohne `ospf`; bei seinem
  eigenen AP gilt das auch, ausser für die Ports zu seinen eigenen Geräten.
- Ein eigener AP, über den ein fremdes Gerät erreicht wird (er speist es per PoE, oder es hängt
  an ihm), darf nicht entfernt werden, und seine Ports dorthin nicht geändert. Der Plan prüft das
  über den Management-Baum und die PoE-Beziehungen.
- **Zugangsdaten:** Der Provisioner sucht sie am Tenant des Geräts zuerst. Ein eigener AP mit dem
  Tenant des Anwohners braucht dort `mikrotik_credentials` des Provisioners, nicht welche, die
  der Anwohner kennt, sonst könnte er den AP an NetBox vorbei ändern. Oder die Suche überspringt
  Anwohner-Tenants und nimmt die der Location/Site.
- Die Vorschau muss vor dem Anwenden zeigen, welche fremden Geräte betroffen wären; trifft es
  fremde, lehnt der Server ab.
- Rechte im Frontend nur ausblenden; geprüft wird im Backend.

## Reihenfolge

1. Rollen aus dem Token (Admin/Anwohner), Tenant-Zuordnung, Anwohner sieht seine Objekte.
2. Leases lesen (nur lesen, kein Risiko).
3. SSID/PSK ändern.
4. Ports einem VLAN zuordnen.
5. APs aufnehmen, eigene APs ändern und entfernen.

## Zu entscheiden

- Tenant als Anwohner-Gruppe, oder ein eigenes Objekt (Contact Group)? Vorschlag: Tenant.
- `owner` am Interface als Custom Field, oder Ports nur über das Gerät?
- Wird ein Anwohner-Apply sofort ausgeführt oder von einem Admin freigegeben?
