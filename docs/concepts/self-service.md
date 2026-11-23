# Konzept: Self-Service für Anwohner

Status: zum grossen Teil gebaut. Was gebaut ist, beschreibt [self-service.md](../self-service.md):
Besitz, Rollen, die Schnittstelle der Anwohner mit ihren Prüfungen und das Anwohner-UI mit
Entwürfen. Hier steht, was offen ist, dazu die Entscheidungen vom 2026-10-23 und 2026-11-17 und
die Ergebnisse des [Prototyps](#anwohner-ui) (2026-11-17). Aufgaben C7 und C21 in
[todo.md](../todo.md).

## Personas

| Persona | darf |
|---|---|
| Admin | alles; dazu die Ansicht eines gewählten Anwohners, mit dessen Regeln |
| Anwohner | seine WLANs: einschalten, ausschalten, SSID und Passwort (PSK); seine Ports: eines seiner VLANs aufschalten oder eines seiner Geräte anschliessen; seine Geräte: wo sie angeschlossen sind, an welchem eigenen Port (Anschluss), in welchem Raum sie stehen und wie ihre Ports belegt sind; die Clients seiner WLANs und die Leases seiner Netze sehen |

Ein Anwohner (Haushalt, Firma) ist **ein Owner** in NetBox, seine Benutzer sind NetBox-Benutzer
in dessen `users`. Alle Owner der Anwohner liegen in einer Owner Group „Anwohner“ (NetBox
verlangt eine; der Provisioner beachtet sie nicht). Alle APs einer WLAN-Gruppe strahlen alle
WLANs der Gruppe aus, auch die anderer Anwohner im selben Haus, und tragen deren VLANs über das
VXLAN der Gruppe. Das ist gewollt: Der Anwohner konfiguriert sein Gerät nur über den
Provisioner, fremde VLANs müssen nicht von ihm ferngehalten werden.

## Anmelden über Pocket-ID

Mit Keycloak gebaut und geprüft ([login.md](../login.md#netbox-über-oidc)). Offen ist dasselbe
mit Pocket-ID:

- NetBox und Provisioner melden beide über denselben OIDC-Provider an. NetBox legt einen
  Benutzer beim ersten Login an und gibt ihm die Gruppen aus `REMOTE_AUTH_DEFAULT_GROUPS`
  (`user_default_groups_handler` in der Login-Pipeline von NetBox 4.7.1); die Gruppe „Anwohner“
  bekommt so jeder neue Benutzer.
- **Neuer Anwohner:** Er meldet sich einmal bei NetBox an; ein Admin trägt ihn danach bei seinem
  Owner unter `users` ein. Die Mitglieder stehen damit nur in NetBox, der Provider meldet nur an.
  Ein Benutzer, den es in NetBox schon gibt, wird beim Login nicht übernommen, NetBox legt einen
  zweiten an: Benutzer nicht von Hand anlegen.
- Der Provisioner findet die Owner über `preferred_username`. Ob Pocket-ID darin den
  Benutzernamen gibt, den NetBox anlegt, ist zu prüfen.
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
Mit Keycloak auch das Login von NetBox selbst (2026-11-20): Der neue Benutzer kommt in
„Anwohner“ und sieht mit seinem Owner nur dessen WLAN. Offen: die Berechtigungen in der
produktiven NetBox, dasselbe mit Pocket-ID, und ob für Ports und WLAN-Gruppen eine ähnliche
Bedingung nötig wird.

## Anwohner-UI

Ein klickbarer Prototyp (PatternFly 6, Design-Canvas „Anwohner-UI“,
https://claude.ai/artifact/LbpkF1eYc9r3pV92eh4RCa) hat die Abläufe durchgespielt (2026-11-17).
Eine Kopie liegt in [self-service-ui.dc.html](self-service-ui.dc.html): eine Design Component
(Vorlage mit `{{…}}` und der Logik in `class Component`); dargestellt wird sie mit der Runtime
der Design-Canvas (`support.js`), die nicht im Repository liegt. Die Tweaks `rolle`, `stoerung`
und `rolloutFails` spielen Admin, Störungen und ein gescheitertes Ausrollen durch.

Gebaut sind die Tabs „Dosen und Geräte“ und „Netze“ mit Entwürfen (Badges mit ihrer Zahl an
den Tabs), Vorschau, Anschluss und Raum.
Offen nach dem Prototyp:

- **Dosen und Geräte:** rechts neben einem Port das Netz mit den Geräten, die dahinter eine
  Adresse bezogen haben (Name aus dem Lease, leer ohne; MAC als Tooltip auf der IP), siehe
  [Leases](#leases).
- **Netze:** QR-Code zum Verbinden (`WIFI:T:WPA;S:…;P:…;;`), die Ports per Kabel („Per Kabel
  an“) und bei den verbundenen Geräten das Band, siehe [Clients](#clients).
- **Änderungen:** ein dritter Tab, jede gespeicherte Änderung mit Fortschritt und dem Stand pro
  Gerät (aus dem automatischen Rollout, C20).
- **Zurücknehmen** einer ausgerollten Änderung macht aus dem Stand davor neue Entwürfe (auch
  SSID und Passwort), die wie jede Änderung über die Vorschau gespeichert werden. Ein
  gescheitertes Ausrollen wird nicht zurückgenommen: Es bleibt als Abweichung sichtbar, bis das
  Gerät die Konfiguration hat.
- **Zustand, wahrheitsgemäss:** Jedes Element (Port, Gerät, Netz) hat genau ein Label:
  „ungespeichert“ (gebaut), sonst der Stand des Ausrollens („wird ausgerollt“, „wartet“ bei
  einem nicht erreichbaren Gerät, „nicht ausgerollt“), sonst der Live-Zustand („Ok“, „aus“,
  „gestört“, „Problem“, etwa ein WLAN nur auf 4 von 5 APs). Ein Text steht nur, wenn etwas nicht
  Ok ist. Ein Gerät zeigt seinen eigenen Stand auch, wenn die Änderung ein Netz betraf. Oben
  fasst eine Meldung zusammen („Alles ok“ oder was nicht stimmt). Live-Zustände kommen aus den
  Live-Reads des Provisioners und pflanzen sich fort (Gerät aus → sein Anschluss gestört, die
  Geräte dahinter nicht erreichbar, das WLAN „Problem“, seine Clients weg).

## Leases

Gebaut ist das Lesen der Leases eines Netzes (`networkDevices`). Offen: hinter welchem Port ein
Lease steckt (Geräte pro Dose im UI). Das sagt die Host-Tabelle der Bridge
(`/interface/bridge/host`) des Switches oder Geräts: welche MAC an welchem Port gelernt wurde
(E29). IPv6 hat keine Leases (SLAAC); dafür ginge `/ipv6/neighbor`, als Ergänzung.

## Clients

Die Clients der WLANs liest `networkDevices` beim Controller. Das Band (2.4/5 GHz, Standard)
fehlt `WlanClient` noch (E28).

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
  fremde Geräte sind erlaubt. Die Geräte erreicht es über ein **automatisches Rollout** (C20,
  [auto-rollout.md](../auto-rollout.md)).
- Ein Anwohner **legt keine WLANs an**; er ändert Ports und das Passwort bestehender WLANs.
  Ergänzt nach dem Prototyp: auch SSID (eindeutig in der WLAN-Gruppe) und ein/aus.

## Entschieden (2026-11-17, Anwohner-UI)

- **Ersteinrichtung** macht vorerst der Admin.
- **Netz = VLAN mit höchstens einem WLAN**, das der Anwohner ein- und ausschaltet; er ändert SSID
  und Passwort.
- **Anschluss** wird erkannt und mit einem Klick übernommen, nicht von selbst.
- **Kein Rückgängig nach gescheitertem Ausrollen:** Es bleibt als Abweichung sichtbar. Eine
  ausgerollte Änderung lässt sich zurücknehmen, als neue Änderung.
- **Admins** sehen das UI eines gewählten Anwohners und dürfen dort ändern.
- **Raum** eines Geräts: wählbar aus allen Räumen der Gebäude, in denen der Anwohner Ports oder
  Geräte hat.
