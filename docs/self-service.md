# Self-Service für Anwohner

Was davon gebaut ist; der Rest steht im [Konzept](concepts/self-service.md).

## Besitz

Ein Anwohner ist ein NetBox-Owner. Er bekommt Geräte, Ports fremder Geräte und VLANs über das
Feld `owner` ([netbox-modeling.md](netbox-modeling.md#besitz-anwohner)). Daraus folgt
(`topology::ownership`, `Topology::owned_by`):

- die Ports seiner Geräte,
- die WLANs auf seinen VLANs,
- die WLAN-Gruppen dieser WLANs.

Die Konfiguration der Geräte hängt nicht davon ab.

## Rollen

Der Provisioner entscheidet bei jeder Abfrage, wer der angemeldete Benutzer ist (`role`):

- **Admin:** `sub` in `auth_users` oder die Gruppe `admin` im Claim `groups`. Nur Admins kommen
  an `/graphql` und den WebSocket `/graphql/ws`, sonst `403` (`user refused, no admin`).
- **Anwohner:** sonst ein Benutzer, den NetBox als Mitglied eines Owners kennt. Gesucht wird
  mit `owner_list(filters: {users: {username: …}})` und dem Claim `preferred_username`, für
  30 s gemerkt. So wirkt eine Änderung der Mitglieder in NetBox spätestens nach 30 s.
- **Niemand:** weder das eine noch das andere, auch wenn NetBox nicht antwortet (mit Warnung
  im Log).

## Schnittstelle der Anwohner

`/graphql/resident` ist ein eigenes, schmales Schema (`graphql::resident`). Die Objekte und
Mutationen der Admins sind darüber nicht erreichbar. Der Endpunkt prüft das Token wie
`/graphql`. „Niemand“ bekommt seine Rolle und sonst nichts, damit das UI sagen kann, woran es
liegt.

- `me { role }`: `ADMIN`, `RESIDENT` oder `NOBODY`.
- `residents`: ein Anwohner bekommt sich selbst, ein Admin jeden Owner, den NetBox an einem
  Gerät, Port oder VLAN nennt (für die Support-Ansicht). Jeder mit:
  - **Ports**, nach Site, Gerät und Portname: die ihm gegebenen Ports fremder Geräte und die
    Ethernet-Ports seiner Geräte, ohne `lo`, Funk und Bridges. Zu jedem Port:
    - sein VLAN, wenn es ihm gehört;
    - ob dort ein fremdes VLAN liegt, ohne dessen Namen;
    - die Enden des Kabels (Gerät, Port, ob eigen).
  - **Netze:** seine VLANs mit VID, den Netzen der Adressen, die NetBox auf dem VLAN hat, und
    seinen WLANs (SSID, Zahl der APs der Gruppe). Ein WLAN einer Gruppe ohne `controller` fehlt,
    wie in der Konfiguration.

Das Passwort eines WLANs gibt das Schema noch nicht heraus.

Im Lab geprüft (2026-11-18, Keycloak und Lab-NetBox mit `local/netbox/seed-residents.py`):

- `meier` und `huber` sehen nur ihr Eigenes.
- `admin` sieht beide.
- `reader` bekommt die Rolle `NOBODY` und keinen Anwohner.
- `meier` bekommt an `/graphql` ebenfalls `403`.

## Anwohner-UI (lesend)

Nach dem Login fragt das UI zuerst `me { role }` am Endpunkt der Anwohner (`app::RoleGate`):

- Ein **Admin** bekommt die Seiten der Admins und dazu im Breadcrumb „Anwohner“ (`/residents`):
  die Support-Ansicht mit der Wahl des Anwohners und der Ansicht, wie er sie sieht.
- Ein **Anwohner** bekommt nur „Mein Netz“ (`pages::residents`), ohne Breadcrumb und ohne
  Routen der Admins. Hat er mehrere Owner, steht jeder mit seinem Namen da.
- **Niemand** bekommt den Hinweis, dass ein Admin ihn bei seinem Haushalt eintragen muss.

Die Ansicht (`components::resident`) hat zwei Tabs nach dem Prototyp:

- **Dosen und Geräte:** nach Site gruppiert. Jede Zeile zeigt links die Dose oder den Port:
  das Label, sonst Wand und Buchse („Büro Ecke links“) oder den Port-Namen, darunter „Dose …“,
  „Port … von …“ oder „Port … an …“. Dann folgt das Kabel, rechts das Gerät am anderen Ende mit
  seinem Port (bei einer Dose: was eingesteckt ist). Ohne Kabel steht rechts das Netz des
  Ports, „Netz des Betreibers“ oder gestrichelt „Kein Netz“; eine Dose mit eingestecktem Gerät
  nennt ihr Netz in der eigenen Karte. Ein Port eines eigenen Geräts, der in einer eigenen Dose
  steckt, erscheint nur bei der Dose. Auf dem Telefon stapeln sich die Karten, das Kabel
  steht senkrecht.
- **Netze:** eine Karte pro VLAN mit VID, Adressen und WLAN (SSID, Zahl der APs).

Noch nicht gebaut sind Räume statt Sites, Geräte pro Port aus den Leases, Status-Labels und alles
Schreibende (C21), als Nächstes das Einstellen, was in einer Dose steckt. Status-Labels kommen erst mit einer verlässlichen
Quelle: Ein Label, das „ausgerollt“ behauptet, ohne es zu wissen, wäre falsch. Geprüft mit
Playwright im Lab (2026-11-18) als `meier` (auch in Telefonbreite), `huber`, `reader` und
`admin`; die Dosen (`wohnung-meier`, `wohnung-huber` hinter `lab-keller-switch`, Geräte ohne
RouterOS) am 2026-11-19 als `meier` und `huber`.
