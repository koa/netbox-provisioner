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
`/graphql` und lässt Admins und Anwohner herein; „Niemand“ bekommt `403` (`no role`).

- `me { role }`: `ADMIN` oder `RESIDENT`.
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
- `reader` bekommt `403`.
- `meier` bekommt an `/graphql` ebenfalls `403`.
