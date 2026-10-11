# Login (OIDC)

Das UI meldet sich über OpenID Connect an (Authorization Code mit PKCE, im Browser, ohne Client
Secret) und schickt das Access-Token als `Bearer` an `/graphql`. Das Backend prüft das Token gegen
den Issuer und lässt nur die Benutzer aus `auth_users` herein. Jeder OIDC-Provider, der das kann,
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
  auth_scopes: "openid profile email"
  # wer hinein darf: der Claim "sub" der Benutzer
  auth_users:
    - 8b1d6e2a-....-....-....-............
```

- **`auth_issuer`**: Das Backend lädt beim Start die Discovery dieser Adresse (der Provider muss
  dann erreichbar sein, sonst startet es nicht) und verlangt, dass `iss` im Token genau so lautet;
  schon ein `/` am Ende zu viel lässt jede Prüfung scheitern.
- **`auth_client_id`**: Das Frontend meldet sich mit dieser Client ID an, und das Backend
  verlangt sie in `aud` des Access-Tokens. So kommen Tokens, die der Provider für andere Apps
  ausstellt, nicht durch.
- **`auth_users`**: Verglichen wird `sub`, nicht der Benutzername. Wer angemeldet ist, aber nicht in
  der Liste steht, bekommt bei jeder Abfrage „User not allowed“.
- **`auth_scopes`**: `offline_access` (Refresh-Token) kennt nicht jeder Provider; dann die Scopes
  ohne ihn angeben.

## Pocket-ID

Unter *OIDC Clients* einen Client anlegen:

- **Public Client** und **PKCE** einschalten.
- **Callback URLs**: die Adresse des Provisioners, am besten mit Wildcard
  (`https://provisioner.example.ch/*`): Nach dem Login kehrt das UI auf die Seite zurück, auf der
  der Login begann.
- Die angezeigte **Client ID** (UUID) ist `auth_client_id`, die Adresse von Pocket-ID (`APP_URL`,
  ohne `/` am Ende) `auth_issuer`.
- `sub` ist die UUID des Benutzers (in der Admin-Oberfläche unter *Users*, oder im Payload eines
  Tokens); diese kommt in `auth_users`.

Nicht gegen eine Pocket-ID-Instanz getestet sind: dass deren Access-Token die Client ID in `aud`
trägt, und ob sie `offline_access` annimmt (darum oben `auth_scopes` ohne ihn). Scheitert die
Token-Prüfung im Backend, ist `aud` das Erste, was zu prüfen ist.

## Keycloak (Lab)

`local/keycloak/realm.json` zeigt, was ein Provider liefern muss: ein Public Client
`netbox-provisioner` mit PKCE (`S256`), Redirect-URIs auf den Provisioner, und ein
Audience-Mapper, da Keycloak die Client ID sonst nicht in `aud` des Access-Tokens setzt. Die
Benutzer haben feste IDs (`lab-admin`, `lab-reader`), damit `auth_users` in
`local/lab/make-config.sh` sie nennen kann; nur `lab-admin` steht dort.
