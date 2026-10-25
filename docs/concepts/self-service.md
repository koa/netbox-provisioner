# Konzept: Self-Service für Anwohner

Status: Konzept, nicht umgesetzt; Entscheidungen vom 2026-10-23 eingearbeitet. Aufgabe C7 in
[todo.md](../todo.md).

## Ausgangslage

- **Zugang:** OIDC (Pocket-ID produktiv, Keycloak im Lab). Hinein darf, wer mit seinem `sub` in
  `auth_users` steht; alle, die hinein dürfen, dürfen alles. `allow_apply` und
  `allow_netbox_write` gelten für den ganzen Server ([Login](../login.md), `WritesAllowed`).
- **Besitz in NetBox** (4.7): ein eigenes Feld `owner` an Device, Interface, VLAN, Wireless LAN,
  Wireless LAN Group, Prefix und den meisten anderen Objekten. Ein Owner führt NetBox-Benutzer
  (`users`) und -Gruppen (`user_groups`) und muss zu einer Owner Group gehören. GraphQL gibt
  `users`/`user_groups` eines Owners nicht aus, filtert aber danach
  (`owner_list(filters: {users: {username: …}})`). Tenants gibt es auch, gebraucht für die Zugangsdaten (`mikrotik_credentials`,
  gesucht am Gerät, an der Location, an der Site); sie bleiben dafür.
- **WLAN:** SSID und PSK stehen am Wireless LAN in NetBox; der Generator verteilt sie über CAPsMAN
  auf die APs der Gruppe ([WLAN](../netbox-modeling.md#wlan)).

## Personas

| Persona | darf |
|---|---|
| Admin | alles wie heute |
| Anwohner | seine WLANs (SSID, PSK), seine Ports (VLAN wählen, AP anschliessen), zusätzliche APs in seine WLAN-Gruppe aufnehmen, seine eigenen APs ändern und entfernen, die Leases seiner Netze sehen |

Ein Anwohner (Haushalt, Firma) ist **ein Owner** in NetBox, seine Benutzer sind NetBox-Benutzer
in dessen `users`. Alle Owner der Anwohner liegen in einer Owner Group „Anwohner“ (NetBox
verlangt eine; der Provisioner beachtet sie nicht). Einem Benutzer gehört jedes Objekt, dessen
`owner` ihn führt: VLANs, Wireless LANs, Geräte und einzelne Ports (`owner` am Interface, auch an
einem fremden Switch); ein Port seines eigenen Geräts gehört ihm auch ohne eigenen `owner`. APs
sind RouterOS-Geräte wie die Router; ein eigener AP darf geändert und entfernt werden. Ein
Objekt ohne `owner` oder mit einem anderen (der des Netzbetreibers) bleibt den Admins. Custom
Fields braucht es dafür nicht.

## Wer ist wer

- **Anmelden:** NetBox und Provisioner melden beide über denselben OIDC-Provider an
  (Pocket-ID). NetBox legt einen Benutzer beim ersten Login an und gibt ihm die Gruppen aus
  `REMOTE_AUTH_DEFAULT_GROUPS` (`user_default_groups_handler` in der Login-Pipeline von 4.7.1);
  die Gruppe „Anwohner“ bekommt so jeder neue Benutzer.
- **Neuer Anwohner:** Er meldet sich einmal bei NetBox an; ein Admin trägt ihn danach bei seinem
  Owner unter `users` ein. Die Mitglieder stehen damit nur in NetBox, der Provider meldet nur an.
- **Rolle im Provisioner:** Gruppe `admin` im Token (`groups`-Claim) → Admin. Sonst sucht der
  Provisioner die Owner des Benutzers (`owner_list(filters: {users: {username: …}})`) mit dem
  Benutzernamen, den NetBox beim Login angelegt hat (bei OIDC `preferred_username`; zu prüfen,
  welchen Claim NetBox mit Pocket-ID nimmt). Ohne Owner und ohne `admin` darf er nichts.
- `auth_users` bleibt als Notschalter für Admins, bis die Gruppen verlässlich sind.

## Leserechte in NetBox

Anwohner dürfen NetBox lesen, aber nicht die Geheimnisse anderer. NetBox-Berechtigungen wirken auf
ganze Objekte, nicht auf Felder: Wer ein Wireless LAN sieht, sieht seinen PSK. Deshalb zwei
Berechtigungen an der Gruppe „Anwohner“:

- **„Netz lesen“**, ohne Bedingung: Geräte, Interfaces, Kabel, VLANs, Prefixe, Adressen, Sites.
- **„eigene WLANs lesen“**: Wireless LAN mit der Bedingung `{"owner__users": "$user"}`.
- **Keine Freigabe** für Wireless Links (PSKs der Funkbrücken) und Tenants
  (`mikrotik_credentials`).

Geprüft in der Lab-NetBox 4.7.1 (2026-10-23) mit zwei Benutzern und zwei Owners: Jeder sieht nur
sein Wireless LAN samt PSK, in REST und GraphQL; Tenants antworten mit 403 bzw. einer leeren
Liste; auch verschachtelt (`device.tenant`, `interface.wireless_link`, `vlan.tenant`) ist nichts
zu sehen, obwohl es im Lab einen Tenant mit Zugangsdaten und zwei Funkbrücken mit PSK gibt. Nicht
geprüft: das Login von NetBox über Pocket-ID selbst, und ob für Ports und WLAN-Gruppen eine
ähnliche Bedingung nötig wird.

## Was der Server prüft

Die Grenze bleibt wie in `CLAUDE.md`: Entschieden wird einmal, an der Schnittstelle. Statt eines
Guards „darf schreiben“ bekommt jede Mutation die Frage „darf dieser Benutzer **dieses Objekt**
ändern“. Anwohner bekommen eigene, enge Mutationen statt Zugriff auf die Admin-Helfer:

- `setWlan(wlan, ssid, psk)`: nur ein eigenes Wireless LAN;
- `setPortVlan(port, vlan)`: eigener Port und eigenes VLAN (VLAN untagged am Port);
- `adoptAp(device, group)`: eigenes Gerät und eigene WLAN-Gruppe;
- `updateAp(device, …)`: an seinem eigenen AP Name/Label, WLAN-Gruppe (nur eigene) und die
  VLANs seiner Ports (nur eigene), dann Anwenden auf AP und Controller;
- `removeAp(device)`: sein eigener AP aus NetBox entfernen (Gerät, Kabel, Adressen, wie
  „Verbindung abräumen“ mit Rückgängig) und die WLAN-Gruppe ohne ihn auf den Controller anwenden.
  Das Gerät selbst läuft weiter, bis es abgesteckt wird; die Seite sagt das, statt es aus der
  Ferne zurückzusetzen;
- `leases(vlan)`: lesen, nur seine Netze.

Jede schreibt nach NetBox (Plan, Vorschau, Rückgängig wie bei den [Helfern](../config-helper.md)) und
wendet danach **nur die betroffenen Geräte** an (für ein WLAN: der Controller der Gruppe). Ein
Anwohner sieht die Vorschau als Liste in Worten („SSID auf allen 3 APs ändern“), nicht als Skript.

## Leases

`/ip/dhcp-server/lease` auf dem Router des Netzes, live gelesen (Modell in `mikrotik-model`
nötig). Nur Leases von Servern auf Interfaces in seinen eigenen VLANs/Netzen. IPv6 hat keine
Leases (SLAAC); dafür ginge `/ipv6/neighbor`, als Ergänzung.

## Risiken

- Ein Anwohner, der einen Port umstellt, kann das Netz dahinter abschneiden (Uplink eines
  Switches). Deshalb nur Ports ohne Kabel zu einem RouterOS-Gerät und ohne `ospf`; bei seinem
  eigenen AP gilt das auch, ausser für die Ports zu seinen eigenen Geräten.
- Ein eigener AP, über den ein fremdes Gerät erreicht wird (er speist es per PoE, oder es hängt
  an ihm), darf nicht entfernt werden, und seine Ports dorthin nicht geändert. Der Plan prüft das
  über den Management-Baum und die PoE-Beziehungen.
- **Zugangsdaten:** Sie bleiben am Tenant (Gerät, Location, Site), getrennt vom Besitz. Der
  Anwohner kennt sie nicht und ändert seinen AP nur über den Provisioner; ein Tenant am AP ist
  Sache der Admins.
- Die Vorschau muss vor dem Anwenden zeigen, welche fremden Geräte betroffen wären; trifft es
  fremde, lehnt der Server ab.
- Rechte im Frontend nur ausblenden; geprüft wird im Backend.

## Reihenfolge

1. NetBox über Pocket-ID mit der Standardgruppe „Anwohner“ und ihren zwei Berechtigungen;
   Rollen im Provisioner (Admin aus dem Token, Anwohner über seine Owner), Anwohner sieht seine
   Objekte.
2. Leases lesen (nur lesen, kein Risiko).
3. SSID/PSK ändern.
4. Ports einem VLAN zuordnen.
5. APs aufnehmen, eigene APs ändern und entfernen.

## Entschieden (2026-10-23)

- Ein Anwohner ist ein **NetBox-Owner** mit seinen Benutzern in `users`, nicht ein Tenant und
  nicht eine Contact Group; die Owner liegen in der Pflicht-Owner-Group „Anwohner“. Die Benutzer
  sind NetBox-Benutzer (Login über Pocket-ID), die Mitglieder werden in NetBox gepflegt, nicht im
  Provider: so wirken dieselben Zuordnungen für die Leserechte in NetBox.
- Ports eines fremden Switches gehören einem Anwohner über das NetBox-Feld `owner` am Interface,
  kein Custom Field.
- Ein Apply, das ein Anwohner auslöst, läuft **sofort**, ohne Freigabe durch einen Admin. Die
  Prüfungen der engen Mutationen (nur eigene Objekte, keine fremden Geräte betroffen) bleiben.
