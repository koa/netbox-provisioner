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
    seinen WLANs (SSID, Passwort bei WPA, Zahl der APs der Gruppe). Ein WLAN einer Gruppe ohne
    `controller` fehlt, wie in der Konfiguration.

### Geräte im Netz

`networkDevices(resident, vlan)` (`graphql::resident::live`, die Leases und WLAN-Clients des
Konzepts in einer Abfrage) liest live, was in einem eigenen Netz hängt:

- die gebundenen Leases (`/ip/dhcp-server/lease`) der Router mit einem DHCP-Server im VLAN
  (ein Interface, das das VLAN ist, mit `dhcp`), nur Adressen in dessen IPv4-Netzen; der Name
  ist der des DHCP-Clients bis zum ersten Steuerzeichen;
- die Clients der WLANs auf dem VLAN, beim Controller ihrer Gruppe (sein CAPsMAN führt auch die
  Clients der CAPs), mit Access Point und Signal; das Band fehlt noch;
- beide über die MAC zusammengeführt, nach Adresse sortiert, Clients ohne Lease zuletzt.

Jedes Gerät wird einmal gelesen, Tabellen bis 10 s alt, alle Geräte zugleich. Wer nicht
antwortet, steht in `unreachable`: Seine Geräte fehlen, und das UI sagt das. Im Anwohner-UI
liest „Geräte im Netz“ in der Karte des Netzes auf Klick. Im Lab geprüft (2026-11-20) als
`meier` und `huber`: Das Lab hat weder einen DHCP-Server noch WLAN-Clients. Gelesen wurde
lab-np2 als Controller (ein Login), die Liste war zu Recht leer; das Zusammenführen ist mit
Unit-Tests geprüft.

### Ändern

Jede Änderung ist der Plan eines Helfers (`graphql::resident::change`), erlaubt nur an
eigenen Objekten: ein Anwohner für seine Owner, ein Admin für jeden (Support-Ansicht). Sonst
lehnt der Server mit `NOT_OWNED` ab.

Die Änderung (`change`, ein `@oneOf`) ist eine von:

- `wlan: { wlan, ssid, psk, enabled }`: Name, Passwort, ein/aus eines eigenen WLANs;
- `portVlan: { port, kind, vlan }`: eines seiner VLANs untagged an einem eigenen Port
  (`kind: INTERFACE`) oder einer eigenen Dose (`SOCKET`: der Switch-Port hinter der Wand, den
  die Dose zeigt), oder `vlan: null` für keines.

Die Abfragen:

- `changePlan(resident, change)`: was gespeichert würde, für die Vorschau: die Änderung (ohne
  die Passwörter, nur dass es ein neues gibt) und die RouterOS-Geräte, deren Konfiguration sich
  ändert: das Gerät des Plans, bei einem WLAN die APs der Gruppe, bei einem Port mit einem VLAN
  über VXLAN die anderen Geräte mit diesem VLAN, wenn das Gerät dazukommt oder wegfällt (ihre
  VTEPs). Ein Switch ohne RouterOS ändert nur NetBox.
- `saveChange(resident, change, fingerprint)`: schreibt den Plan, wenn er noch der gezeigte ist,
  als `NetboxWrite`-Job des Geräts des Plans (wie die Helfer der Admins, mit Rückgängig dort).
  Im NetBox-Changelog steht „WLAN lab-wlan (meier for Familie Meier)“.
- `savedChange(job)`: wie das Speichern steht (`SAVING`, `SAVED`, `FAILED` mit dem Fehler), nur
  für den Anwohner des Jobs oder einen Admin.

Die Regeln des Helfers „WLAN ändern“ (`helper::wlan`, auch im Admin-Schema als `Helper.wlan`):

- SSID: 1 bis 32 Bytes, ohne Leerzeichen am Rand, in der WLAN-Gruppe eindeutig (gegen alle
  WLANs der Gruppe, auch fremde);
- Passwort: 12 bis 63 druckbare ASCII-Zeichen, nur für ein WLAN mit WPA-Passwort (die
  Sicherheit eines WLANs ändert ein Admin);
- ein- oder ausschalten: der Status `active` oder `disabled` am Wireless LAN; ausgeschaltet
  bleibt es in der Ansicht des Anwohners und zählt für eindeutige SSIDs mit;
- nichts geändert ist ein Fehler (`NOTHING_TO_CHANGE`).

Die Regeln für ein Netz am Port (der Helfer „VLAN am Port“, `helper::port_vlan`, dazu):

- der Port ist ein Ethernet-Port ohne Bridge, Adresse, Uplink und Kabel zu einem RouterOS-Gerät:
  den Port, an dem sein eigenes Gerät angebunden ist, stellt `connectDevice` um;
- an seinem Kabel hängen nur Geräte des Anwohners (auch durch die Dose), sonst `NOT_OWNED` mit
  dem fremden Gerät: ein anderes Netz schnitte ab, was dort hängt;
- eine Dose, deren Rear-Port zu keinem Switch-Port führt, ist `SOCKET_WITHOUT_PORT`;
- ohne VLAN ist es kein Mode und kein VLAN; ein Port ohne VLAN bekommt keines
  (`NOTHING_TO_CHANGE`).

Die Passwörter stehen weder im Fingerprint noch im Job-Log (`•••`); das alte bleibt für
Rückgängig nur im Speicher, NetBox-Jobs werden nicht auf die Platte geschrieben. Auf die Geräte
kommt die Änderung über das automatische Rollout (C20); ein Gerät, das nicht selbst ausrollt,
wartet auf die Freigabe eines Admins.

Im Lab geprüft (2026-11-18, Keycloak und Lab-NetBox mit `local/netbox/seed-residents.py`):

- `meier` und `huber` sehen nur ihr Eigenes.
- `admin` sieht beide.
- `reader` bekommt die Rolle `NOBODY` und keinen Anwohner.
- `meier` bekommt an `/graphql` ebenfalls `403`.

## Anwohner-UI

Nach dem Login fragt das UI zuerst `me { role }` am Endpunkt der Anwohner (`app::RoleGate`):

- Ein **Admin** bekommt die Seiten der Admins und dazu im Breadcrumb „Anwohner“ (`/residents`):
  die Support-Ansicht mit der Wahl des Anwohners und der Ansicht, wie er sie sieht.
- Ein **Anwohner** bekommt nur „Mein Netz“ (`pages::residents`), ohne Breadcrumb und ohne
  Routen der Admins. Hat er mehrere Owner, steht jeder mit seinem Namen da.
- **Niemand** bekommt den Hinweis, dass ein Admin ihn bei seinem Haushalt eintragen muss.

Die Ansicht (`components::resident`) hat zwei Tabs nach dem Prototyp:

- **Dosen und Geräte:** nach Site gruppiert, darin nach der Location des Geräts samt ihren
  übergeordneten Locations („1. OG › Wohnung Meier“). Jede Zeile zeigt links die Dose oder den Port:
  das Label, sonst Wand und Buchse („Büro Ecke links“) oder den Port-Namen, darunter „Dose …“,
  „Port … von …“ oder „Port … an …“. Dann folgt das Kabel, rechts das Gerät am anderen Ende mit
  seinem Port (bei einer Dose: was eingesteckt ist). Ohne Kabel steht rechts das Netz des
  Ports, „Netz des Betreibers“ oder gestrichelt „Kein Netz“; eine Dose mit eingestecktem Gerät
  nennt ihr Netz in der eigenen Karte. Ein Port eines eigenen Geräts, der in einer eigenen Dose
  steckt, erscheint nur bei der Dose. Auf dem Telefon stapeln sich die Karten, das Kabel
  steht senkrecht.
- **Netze:** eine Karte pro VLAN mit VID, Adressen und WLAN (SSID, Zahl der APs). Am WLAN
  „WLAN ändern“ (`wlan_edit`): Name und Passwort (zeigen, zufällig erzeugen: 16 Zeichen aus
  der Zufallsquelle des Browsers, ohne 0, O, 1, l und I) und der Schalter „Eingeschaltet“,
  „Vorschau“ erst nach einer Änderung. Ein ausgeschaltetes WLAN steht als „ausgeschaltet“ da.
  Die Vorschau sagt in Worten, was sich ändert, welche Geräte eine neue Konfiguration bekommen
  und dass verbundene Geräte kurz das WLAN verlieren (ausgeschaltet: bis es wieder an ist);
  Vorschau und Speichern teilen sich die WLANs mit „Netz ändern“ (`change_flow`); „Speichern“ wartet auf den Job und sagt
  dann „In NetBox gespeichert“, ohne zu behaupten, dass es schon ausgerollt ist.

Unter jeder Zeile von „Dosen und Geräte“, an deren Kabel nur eigene Geräte hängen, steht „Netz
ändern“ (`port_vlan_edit`): eines seiner Netze oder „Kein Netz“, „Vorschau“ erst bei einem
anderen als dem jetzigen; die Vorschau sagt „Kunde: lab-wlan statt Kein Netz“ (ein fremdes
VLAN als „Netz des Betreibers“), die Geräte und was Geräte daran merken.

Noch nicht gebaut sind Geräte pro Port aus den Leases, Status-Labels, Geräte anschliessen und Entwürfe über mehrere Elemente (C21).
Status-Labels kommen erst mit einer verlässlichen Quelle: Ein Label, das „ausgerollt“ behauptet, ohne es zu wissen, wäre falsch. Geprüft mit
Playwright im Lab (2026-11-18) als `meier` (auch in Telefonbreite), `huber`, `reader` und
`admin`; die Dosen (`wohnung-meier`, `wohnung-huber` hinter `lab-keller-switch`, Geräte ohne
RouterOS, in den Locations „1. OG › Wohnung …“) am 2026-11-19 als `meier` und `huber`.

„WLAN ändern“ im Lab geprüft (2026-11-19) als `meier`, auch in Telefonbreite: `lab-wlan` mit
`lab-np2` als Controller (das Custom Field `controller` der Gruppe zeigte seit 2026-10-31 auf ein
gelöschtes Gerät). Ein zufälliges Passwort und die SSID hin und zurück gespeichert; nach der
Freigabe des Admins rollte das automatische Rollout lab-np2 aus, danach war sein Diff leer.
lab-wap ist nicht in der Gruppe: ihm fehlt das Paket `wireless` für einen CAP. Ebenso
`lab-wlan` aus- und wieder eingeschaltet: ausgeschaltet nahm das Rollout die Konfiguration
`lab-wlan` und VLAN 104 von der Bridge von lab-np2, `lab-gast` blieb allein mit seiner VLAN im
Datapath der CAPs; eingeschaltet kam alles zurück, beide Male danach ein leerer Diff.

„Netz ändern“ im Lab geprüft (2026-11-20) als `meier` in Telefonbreite: lab-wap `ether2`
(„Kunde“) von keinem Netz auf `lab-wlan` und zurück. Die Vorschau nannte lab-wap und lab-np2
(sein VTEP zu lab-wap); lab-np2 hielt das automatische Rollout an, bis der Admin freigab, dann
rollte es beide zusammen aus (lab-wap bekam Bridge und VXLAN zu lab-np2), danach waren beide
Diffs leer. Für Meiers Dose (hinter `lab-keller-switch` ohne RouterOS) sagt die Vorschau, dass
sich nur NetBox ändert. `ether3` von lab-crs125 blieb ungetestet: Dort baute `lab-wlan` mit
seinem VXLAN die Switch-Tabellen des CRS125 auf VLAN-Filtering der Bridge um.
