# Login (OIDC)

Das UI meldet sich über OpenID Connect an (Authorization Code mit PKCE, im Browser, ohne Client
Secret) und schickt das ID-Token als `Bearer` an `/graphql`: Nur darin steht bei Pocket-ID der
Name des Benutzers, das Access-Token trägt nur `sub`, `client_id` und `scope`. Liefert ein Refresh
kein neues ID-Token, geht das Access-Token mit (der Name fehlt dann, in den Metriken steht `sub`).
Das Backend prüft das Token gegen
den Issuer und lässt nur Admins herein: die Benutzer aus `auth_users` und die mit der Gruppe
`admin` im Claim `groups`. Die Anwohner fragen `/graphql/resident`
([self-service.md](self-service.md#rollen)). Bei den Zeiten (`iat`, `nbf`,
`exp`) lässt es 60 s Abweichung zu: ohne lehnte ein Server, dessen Uhr eine Sekunde hinter
Pocket-ID ging, jedes frische Token ab. Lehnt es ein Token mit gültiger Signatur ab (`403`
„Token does not have sufficient rights“), steht der Grund im Log (`token refused: …`, mit den
Claims und der Uhrzeit des Servers). `exp`, `iss`, `aud` und `sub` muss das Token haben: die
Prüfung (biscuit) schaut einen Claim sonst nur an, wo er steht, ein Token ohne `exp` liefe nie ab.
Die WebSocket-Verbindung der Live-Ansichten bekommt das Token nur beim Aufbau
(`connection_init`); jede Operation darauf, auch eine Query oder Mutation, endet beim Ablauf des
Tokens (`exp` plus die 60 s) mit dem Fehler `TOKEN_EXPIRED`, und das UI baut die Verbindung mit
dem aktuellen Token neu auf. Die Schlüssel des Issuers holt das Backend beim Start, danach jede
Stunde und sofort, wenn ein Token einen unbekannten Schlüssel (`kid`) nennt, höchstens einmal pro
Minute (`binary/src/issuer_keys.rs`): Ein neuer Schlüssel beim Provider gilt so ab dem ersten
Login damit, ein zurückgezogener fällt spätestens nach einer Stunde weg; scheitert ein Abruf,
bleiben die bisherigen. Im Lab geprüft: ein zusätzlicher Schlüssel höherer Priorität im Keycloak
direkt nach dem Start, das Login danach ging ohne Fehler durch. Jeder OIDC-Provider, der das kann,
funktioniert; das Lab verwendet Keycloak (`local/keycloak/`, Realm `lab`), produktiv ist Pocket-ID
vorgesehen.

## config.yaml

```yaml
settings:
  # die Client ID beim Provider
  auth_client_id: 3f2c9a1e-....-....-....-............
  # die Adresse des Providers, genau wie "issuer" in <issuer>/.well-known/openid-configuration
  auth_issuer: https://id.example.ch
  # angeforderte Scopes, mit Leerzeichen getrennt (ohne Angabe: openid profile email offline_access)
  # auth_scopes: "openid profile email"
  # wer hinein darf: der Claim "sub" der Benutzer
  auth_users:
    - 8b1d6e2a-....-....-....-............
```

- **`auth_issuer`**: Das Backend lädt beim Start die Discovery dieser Adresse (der Provider muss
  dann erreichbar sein, sonst startet es nicht) und verlangt, dass `iss` im Token genau so lautet;
  schon ein `/` am Ende zu viel lässt jede Prüfung scheitern.
- **`auth_client_id`**: Das Frontend meldet sich mit dieser Client ID an, und das Backend
  verlangt sie in `aud` des Tokens (beim ID-Token immer so). So kommen Tokens, die der Provider für andere Apps
  ausstellt, nicht durch.
- **`auth_users`**: Verglichen wird `sub`, nicht der Benutzername. Wer angemeldet ist, aber nicht in
  der Liste steht, bekommt bei jeder Abfrage `403` („Kein Zugriff“ im UI); das Serverlog nennt
  das `sub` (`user refused, no admin`). Statt der Liste macht auch die Gruppe `admin` im Claim
  `groups` zum Admin; die Liste bleibt als Notschalter.
- **`auth_scopes`**: `offline_access` (Refresh-Token) kennt nicht jeder Provider; dann die Scopes
  ohne ihn angeben. Ohne Refresh-Token erneuert das UI das Token nicht: Ist es abgelaufen,
  antwortet der Server `403` (und schliesst den WebSocket bei `connection_init`), bis man sich
  neu anmeldet. Jede Seite zeigt dann oben „Anmeldung abgelaufen“ mit „Erneut anmelden“
  (`components::frame::login_expired`, Z34): Der Login führt über den Provider zurück auf dieselbe
  Seite, von selbst leitet nichts um, ein offenes Formular bleibt, bis man klickt. Die Job-Ansicht
  fragt
  weiter und nimmt den Job danach wieder auf. Wer angemeldet ist, aber nicht in `auth_users`
  steht, bekommt den Hinweis nicht: neu anmelden änderte nichts. Im Lab mit Keycloak ohne
  Refresh-Token und 20 s Laufzeit geprüft, über HTTP-Abfragen und über den WebSocket allein.

## Pocket-ID

Unter *OIDC Clients* einen Client anlegen:

- **Public Client** und **PKCE** einschalten.
- **Callback URLs**: die Adresse des Provisioners mit `**` als Pfad
  (`https://provisioner.example.ch/**`): Nach dem Login kehrt das UI auf die Seite zurück, auf der
  der Login begann. `*` steht bei Pocket-ID (2.19) für genau ein Pfadsegment, nicht leer: `/*`
  deckt weder `/` noch `/site/1/device/3/show` ab („redirect_uri … is not registered“).
- Die angezeigte **Client ID** (UUID) ist `auth_client_id`, die Adresse von Pocket-ID (`APP_URL`,
  ohne `/` am Ende) `auth_issuer`.
- `sub` ist die UUID des Benutzers (in der Admin-Oberfläche unter *Users*, oder im Payload eines
  Tokens); diese kommt in `auth_users`.

Mit Pocket-ID getestet: Das Access-Token trägt die Client ID in `aud` und kommt durch die Prüfung,
aber ohne `name`. `offline_access` nimmt Pocket-ID an (2.19.1, 2026-11-06, lokal mit einem Public
Client, PKCE und 1 min Laufzeit des Access-Tokens, `accessTokenDurationMinutes`): Es gibt ein
Refresh-Token, jedes Refresh bringt ein neues ID-Token mit `name`, und das UI blieb über 4 min
angemeldet, ohne Hinweis und ohne abgelehntes Token. `auth_scopes` bleibt darum ohne Angabe.

Für die Rollen ([self-service.md](self-service.md#rollen)) braucht das ID-Token den Claim
`groups` (Admins) und `preferred_username` (der Benutzername, den NetBox beim Login über
Pocket-ID anlegt). Bei Pocket-ID kommt `groups` mit dem Scope `groups`, also
`auth_scopes: "openid profile email groups offline_access"`. Mit Pocket-ID ist das noch nicht
geprüft, ebenso wenig, welchen Claim NetBox als Benutzernamen nimmt.

## Keycloak (Lab)

`local/keycloak/realm.json` zeigt, was ein Provider liefern muss: ein Public Client
`netbox-provisioner` mit PKCE (`S256`), Redirect-URIs auf den Provisioner, und ein
Audience-Mapper, da Keycloak die Client ID sonst nicht in `aud` des Access-Tokens setzt. Die
Benutzer haben feste IDs (`lab-admin`, `lab-reader`), damit `auth_users` in
`local/lab/make-config.sh` sie nennen kann; nur `lab-admin` steht dort. `admin` ist zudem in der
Gruppe `admin`, ein Group-Membership-Mapper setzt den Claim `groups`. Die Anwohner
`meier`/`meier` und `huber`/`huber` sind in der Lab-NetBox Mitglieder ihrer Owner
(`local/netbox/seed-residents.py`); `reader` ist weder Admin noch Anwohner.

## NetBox über OIDC

NetBox meldet über denselben Provider an (`python-social-auth`, Backend `oidc`). In der
NetBox-Konfiguration, wie die Lab-NetBox mit Keycloak (`local/netbox/configuration.py`):

```python
REMOTE_AUTH_ENABLED = True
REMOTE_AUTH_BACKEND = "social_core.backends.open_id_connect.OpenIdConnectAuth"
REMOTE_AUTH_DEFAULT_GROUPS = ["Anwohner"]
SOCIAL_AUTH_OIDC_OIDC_ENDPOINT = "<Issuer>"
SOCIAL_AUTH_OIDC_KEY = "<Client ID>"
SOCIAL_AUTH_OIDC_SECRET = "<Client Secret>"
```

Der Client ist ein eigener, vertraulicher (mit Secret), Callback
`<NetBox>/oauth/complete/oidc/`. Der Login beginnt mit einem POST auf `/oauth/begin/oidc/` (der
Knopf auf der Login-Seite); ein GET dorthin ist `405`.

Mit Keycloak geprüft (NetBox 4.7.3, 2026-11-20), mit dem Benutzer `fischer`, den NetBox vorher
nicht kannte:

- Der erste Login legt den Benutzer `fischer` an, also `preferred_username` des Tokens, mit
  E-Mail und Namen, in der Gruppe „Anwohner“.
- Ohne Owner sieht er Geräte, aber kein Wireless LAN; Wireless Links, Tenants (und Locations,
  die „Netz lesen“ nicht nennt) sind `403`, in GraphQL leer.
- Bei „Familie Meier“ eingetragen, sieht er in REST und GraphQL genau `lab-wlan` mit PSK, nicht
  `lab-gast` von Huber.
- Der Provisioner: `preferred_username` ist `fischer`, also der NetBox-Benutzername, und die
  Rolle ist `RESIDENT` mit dem Owner „Familie Meier“; die Ansicht zeigt dessen Ports und
  Geräte. Ohne Owner (nach dem Cache von 30 s) ist die Rolle `NOBODY`, die Seite sagt es. Ohne
  Gruppe fehlt der Claim `groups` im Token ganz; `admin` hat ihn und ist `ADMIN`.

**Benutzer nicht vorher anlegen.** Gibt es den Benutzernamen in NetBox schon (von Hand
angelegt), verknüpft NetBox den Login nicht damit, sondern legt einen zweiten an
(`meier73c3169cbfaf4116`), in „Anwohner“ und ohne Owner: Der Anwohner sieht in NetBox sein
WLAN nicht, und der Provisioner, der den Owner über `preferred_username` sucht, fände den alten
Benutzer. Ein Anwohner meldet sich darum zuerst bei NetBox an, dann trägt ihn ein Admin bei
seinem Owner ein. Im Lab sind `meier` und `huber` so angelegte Benutzer: Für den Login bei
NetBox ist `fischer` da.

Mit Pocket-ID ist das noch nicht geprüft.
