# Konzept: Self-Service für Anwohner

Status: Konzept, nicht umgesetzt; Entscheidungen vom 2026-10-23 und 2026-11-17 eingearbeitet.
Aufgabe C7 in [todo.md](../todo.md).

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
| Anwohner | das Passwort (PSK) seiner WLANs; seine Ports: eines seiner VLANs aufschalten oder eines seiner Geräte anschliessen; seine Geräte: wo sie angeschlossen sind und wie ihre Ports belegt sind; die Clients seiner WLANs und die Leases seiner Netze sehen |

Ein Anwohner (Haushalt, Firma) ist **ein Owner** in NetBox, seine Benutzer sind NetBox-Benutzer
in dessen `users`. Alle Owner der Anwohner liegen in einer Owner Group „Anwohner“ (NetBox
verlangt eine; der Provisioner beachtet sie nicht). Ein Objekt ohne `owner` oder mit einem
anderen (der des Netzbetreibers) bleibt den Admins. Custom Fields braucht es dafür nicht.

Was einem Anwohner gehört:

- **In NetBox gesetzt** (`owner`, nur von Admins):
  - **Geräte**, die ihm übergeben werden, meist APs mit eingebautem Switch; RouterOS-Geräte wie
    die Router.
  - **Ports** fremder Geräte (`owner` am Interface, z.B. die Wohnungsdose an einem Switch des
    Betreibers).
  - **VLANs**, etwa „Allgemein“, „Gast“, „Server“; ihre VLAN-Gruppe hat eine VNI, sonst
    erreichen sie seine Ports und Geräte nicht ([VXLAN](../netbox-modeling.md#vxlan)).
- **Abgeleitet:**
  - die Ports seiner Geräte;
  - die WLANs auf seinen VLANs (Feld `vlan` des Wireless LAN): Mit dem VLAN gehört ihm das WLAN;
    `owner` am Wireless LAN braucht es nicht. Ein WLAN ohne VLAN gehört niemandem, es ist ohnehin
    ein Fehler (`WlanWithoutVlan`);
  - die Prefixe seiner VLANs und deren Leases;
  - seine WLAN-Gruppe: die Gruppe seiner WLANs. Liegen sie in mehreren, legt der Admin sie bei
    der Übergabe des Geräts fest.

Alle APs einer WLAN-Gruppe strahlen alle WLANs der Gruppe aus (eine Provisioning-Regel am
Controller), auch die anderer Anwohner im selben Haus, und tragen deren VLANs über das VXLAN der
Gruppe. Das ist gewollt: Der Anwohner konfiguriert sein Gerät nur über den Provisioner, fremde
VLANs müssen nicht von ihm ferngehalten werden.

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
- **„eigene WLANs lesen“**: Wireless LAN mit der Bedingung `{"vlan__owner__users": "$user"}`
  (das WLAN gehört ihm über sein VLAN).
- **Keine Freigabe** für Wireless Links (PSKs der Funkbrücken) und Tenants
  (`mikrotik_credentials`).

Geprüft in der Lab-NetBox 4.7.1 (2026-10-23) mit zwei Benutzern und zwei Owners: Jeder sieht nur
sein Wireless LAN samt PSK, in REST und GraphQL; Tenants antworten mit 403 bzw. einer leeren
Liste; auch verschachtelt (`device.tenant`, `interface.wireless_link`, `vlan.tenant`) ist nichts
zu sehen, obwohl es im Lab einen Tenant mit Zugangsdaten und zwei Funkbrücken mit PSK gibt. Die
Bedingung über das VLAN (`vlan__owner__users`) ebenso (2026-11-17, zwei Owner mit je einem VLAN
und einem WLAN darauf): Jeder sieht in REST und GraphQL nur das WLAN auf seinem VLAN, samt PSK.
Nicht geprüft: das Login von NetBox über Pocket-ID selbst, und ob für Ports und WLAN-Gruppen eine
ähnliche Bedingung nötig wird.

## Was der Server prüft

Die Grenze bleibt wie in `CLAUDE.md`: Entschieden wird einmal, an der Schnittstelle. Statt eines
Guards „darf schreiben“ bekommt jede Mutation die Frage „darf dieser Benutzer **dieses Objekt**
ändern“. Anwohner bekommen eigene, enge Mutationen statt Zugriff auf die Admin-Helfer. Sie
legen fest, was ein Anwohner **in NetBox** eintragen darf; was das auf fremden Geräten
bewirkt, ist erlaubt (sein Port bekommt ein Transfernetz und `ospf`, ein Switch wird VTEP seiner
VLANs, der Controller seiner Gruppe bekommt einen CAP):

- `setWlanPsk(wlan, psk)`: das Passwort eines eigenen WLANs. SSID, Sicherheit und VLAN bleiben
  den Admins; neue WLANs legt er nicht an: die Zusammenhänge von VLAN, WLAN und Adressen sind
  für Anwohner zu verwickelt;
- `setPortVlan(port, vlan)`: ein eigenes VLAN untagged an einem eigenen Port, oder keines;
- `connectDevice(port, device)`: ein eigenes Gerät an einen eigenen Port (auch einen Port eines
  anderen eigenen Geräts): Kabel, Transfernetz und `ospf` wie „Neues Gerät anschliessen“ und
  „Verbindung“, Loopback falls keine, die WLAN-Gruppe vorgegeben (seine). Hing es vorher woanders,
  räumt der Plan die alte Verbindung ab („Verbindung abräumen“);
- `disconnectDevice(device)`: ein eigenes Gerät abhängen (Kabel, Transfernetz, wie „Verbindung
  abräumen“). Das Gerät bleibt in NetBox und gehört ihm weiter;
- `leases(vlan)`, `wlanClients(wlan)`: lesen, nur seine Netze und WLANs.

Jede schreibt nach NetBox (Plan, Vorschau, Rückgängig wie bei den [Helfern](../config-helper.md)).
Ein Anwohner sieht die Vorschau als Liste in Worten („Port e05 ins VLAN Gast“), nicht als
Skript. Auf die Geräte kommt die Änderung über ein **automatisches Rollout** der NetBox-Änderungen
(C20), nicht über ein Anwenden, das der Anwohner auslöst: Ohne das ergibt Self-Service keinen
Sinn, denn seine Änderung betrifft oft mehrere Geräte, auch fremde.

## Leases

`/ip/dhcp-server/lease` auf dem Router des Netzes, live gelesen (das Modell gibt es,
`IpDhcpServerLease`). Nur Leases von Servern auf Interfaces in seinen eigenen VLANs/Netzen. IPv6 hat keine
Leases (SLAAC); dafür ginge `/ipv6/neighbor`, als Ergänzung.

## Clients

Die Registrierungstabellen von `/interface/wifi` (am Controller, auch die Clients der CAPs) und
`/caps-man` liest `device::metrics` schon (`WlanClient`, mit SSID, AP, Signal, Name und Adresse
aus den Leases); der Anwohner sieht die Clients mit der SSID eines seiner WLANs.

## Risiken

- **OSPF am Port des Anwohners.** Sein Gerät hat er in der Hand: Nach einem Reset, oder mit
  einem anderen Gerät am Port, spräche er OSPF ins Netz und könnte Routen einspeisen (auch einen
  Default). Deshalb **OSPF-Authentisierung pro Link** (A10): Ein Gerät ohne den Schlüssel wird
  kein Nachbar. Den Schlüssel kennt nur der Provisioner, er steht nicht in NetBox.
- Ein Anwohner, der einen Port umstellt, schneidet ab, was dahinter hängt. An seinen eigenen
  Ports trifft es nur seine Geräte; den Port, über den sein Gerät selbst angebunden ist, stellt
  er über `connectDevice` um, nicht über `setPortVlan`.
- Ein eigenes Gerät, über das ein fremdes erreicht wird (es speist es per PoE, oder es hängt an
  ihm), darf nicht abgehängt werden, und seine Ports dorthin nicht geändert. Der Plan prüft das
  über den Management-Baum und die PoE-Beziehungen.
- **Zugangsdaten:** Sie bleiben am Tenant (Gerät, Location, Site), getrennt vom Besitz. Der
  Anwohner kennt sie nicht und ändert sein Gerät nur über den Provisioner; ein Tenant am Gerät ist
  Sache der Admins.
- Rechte im Frontend nur ausblenden; geprüft wird im Backend.

## Reihenfolge

1. NetBox über Pocket-ID mit der Standardgruppe „Anwohner“ und ihren zwei Berechtigungen;
   Rollen im Provisioner (Admin aus dem Token, Anwohner über seine Owner), Besitz gesetzt und
   abgeleitet, Anwohner sieht seine Objekte.
2. Leases und Clients lesen (nur lesen, kein Risiko).
3. Passwort der WLANs ändern.
4. Ports einem VLAN zuordnen.
5. Geräte anschliessen und abhängen (braucht A10).

Das automatische Rollout (C20) braucht es ab Schritt 3, damit eine Änderung die Geräte erreicht.

## Entschieden (2026-10-23)

- Ein Anwohner ist ein **NetBox-Owner** mit seinen Benutzern in `users`, nicht ein Tenant und
  nicht eine Contact Group; die Owner liegen in der Pflicht-Owner-Group „Anwohner“. Die Benutzer
  sind NetBox-Benutzer (Login über Pocket-ID), die Mitglieder werden in NetBox gepflegt, nicht im
  Provider: so wirken dieselben Zuordnungen für die Leserechte in NetBox.
- Ports eines fremden Switches gehören einem Anwohner über das NetBox-Feld `owner` am Interface,
  kein Custom Field.
- Ein Apply, das ein Anwohner auslöst, läuft **sofort**, ohne Freigabe durch einen Admin. Die
  Prüfungen der engen Mutationen (nur eigene Objekte, keine fremden Geräte betroffen) bleiben.
  Abgelöst am 2026-11-17: Der Anwohner schreibt nur NetBox, die Geräte erreicht es über das
  automatische Rollout, und fremde Geräte dürfen betroffen sein.

## Entschieden (2026-11-17)

- Ein Anwohner bekommt **Geräte** (meist APs mit Switch), **Ports** fremder Geräte und **VLANs**
  (`owner`). Seine WLANs leiten sich aus seinen VLANs ab, seine WLAN-Gruppe aus seinen WLANs.
- **Alle APs einer WLAN-Gruppe strahlen alle WLANs der Gruppe aus**, auch die anderer Anwohner;
  keine Gruppe pro Anwohner.
- **OSPF-Authentisierung pro Link** gegen ein fremdes Gerät am Port eines Anwohners.
- **Fremde VLANs** müssen nicht von seinem Gerät ferngehalten werden: Er konfiguriert es nur über
  den Provisioner.
- Die Mutationen regeln nur, was ein Anwohner **in NetBox** eintragen darf; Auswirkungen auf
  fremde Geräte sind erlaubt. Die Geräte erreicht es über ein **automatisches Rollout** (C20).
- Ein Anwohner **legt keine WLANs an**; er ändert Ports und das Passwort bestehender WLANs.
