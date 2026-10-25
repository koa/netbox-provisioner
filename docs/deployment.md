# Container, Helm-Chart und CI

Der Provisioner liefert ein OCI-Image und ein Helm-Chart. Die Konfiguration des Clusters (Argo
CD, Secrets, Gateway) macht der Betreiber von Hand.

## Das Binary

- **Ein Binary** (`binary`) mit dem Frontend als statischen Dateien (`ResourceFiles`). Das
  Frontend wird vorher gebaut (`build.sh`, `local/lab/run-provisioner.sh`, im Flake ein eigener
  Schritt); `binary/build.rs` prüft nur, dass `frontend/dist` da ist. Zum Bauen braucht es Rust
  mit dem Ziel `wasm32-unknown-unknown`, `trunk`, `dart-sass` und `wasm-bindgen-cli`.
- **Ports:** API und UI auf `server_port` (Vorgabe 8080), Management auf `server_mgmt_port`
  (Vorgabe +1000, also 9080) mit `/metrics` (Prometheus) und `/health`.
- **Konfiguration:** `config.yaml` im Arbeitsverzeichnis. Einzelne Werte übersteuern
  Umgebungsvariablen mit `__` als Trenner der Ebenen, etwa `APP_SETTINGS__NETBOX_TOKEN` für
  `settings.netbox_token` (Schlüssel kommen kleingeschrieben an). In der Datei stehen
  Geheimnisse: NetBox-Tokens, `mikrotik_credentials`, das Passwort des Management-Proxys.
- **Zustand:** nur die weiterführbaren Aufträge in `state_dir` (Vorgabe `state` neben
  `config.yaml`, mit Sperrdatei gegen eine zweite Instanz;
  [Jobs](jobs.md#weiterführen-nach-einem-neustart)). Alle anderen Jobs leben im Prozess.
- **TLS:** Das Binary linkt OpenSSL (`openssl-sys`, über `native-tls`), nicht nur rustls.
- Zur Laufzeit startet der Provisioner keine Programme (`Command::new` nur in Tests; SSH über
  `russh`).

## Image mit Nix

`flake.nix`, `packages.image`: `pkgs.dockerTools.buildLayeredImage` packt genau die
Laufzeit-Abhängigkeiten (die Closure) des Binaries ins Image, sonst nichts: keine Shell, kein
Paketmanager, keine coreutils. Arbeitsverzeichnis `/config`, Benutzer 65532. Die Toolchain (Rust
samt wasm-Ziel über `rust-overlay`, `trunk`, `dart-sass`, `wasm-bindgen-cli` passend zu
`Cargo.lock`) ist im Ausdruck festgelegt, so bauen Lab, CI und Image gleich. Heraus kommt ein
normales OCI-Image.

- **Closure** (Versuch 2026-10-19): das Binary (74 MiB, Frontend eingebettet), glibc,
  `gcc-lib`/`libgcc`, OpenSSL, `libidn2`, `libunistring`, dazu `cacert`. Image rund 50 MiB
  gepackt, 133 MiB entpackt; ausführbar ist darin ausser dem Provisioner nur
  `glibc/libexec/getconf/*`.
- **`checks`:** die Tests ohne CHR, die drei Clippy-Läufe mit `-D warnings` und `no-shell`: Taucht
  in der Closure eine Shell oder ein Interpreter auf, schlägt der Check fehl. So bleibt das Image
  ohne Shell, auch wenn eine neue Abhängigkeit eine mitbrächte.
- **Bibliotheken:** `mikrotik-model` und `mikrotik-api` sind Flake-Inputs (GitHub, Zweig
  `claude-edition`). Der Build legt sie neben das Repository, wo die Pfad-Abhängigkeiten in
  `Cargo.toml` sie finden.
- `packages.provisioner` ist das Binary als `netbox-provisioner`, dazu eine `devShell`.

**Fehlersuche ohne Shell:** `kubectl debug -it <pod> --image=busybox --target=<container>`
hängt bei Bedarf einen ephemeren Debug-Container an.

## Helm-Chart

`charts/netbox-provisioner`:

- **Eine Replik** mit Strategie `Recreate`: Jobs, Leases (`device/power.rs`) und das Abbrechen
  leben im Prozess, zwei Repliken würden dasselbe Gerät gleichzeitig anwenden, und beim Update
  laufen nie zwei.
- **`state_dir`** ist das Volume `state` unter `/state`: `persistence.existingClaim`, sonst ein
  `emptyDir`, das einen Neustart des Containers übersteht, aber keinen neuen Pod.
- **Beenden:** Der Server lässt weiterführbare Aufträge liegen und bricht die anderen ab. Das
  dauert bis 30 s für offene Anfragen (actix) und dann bis 20 s für die abgebrochenen Aufträge;
  `terminationGracePeriodSeconds` ist 60 s.
- **Konfiguration:** Jedes Setting von `config.yaml` ist ein eigener Wert unter `settings`; das
  Chart schreibt sie ohne die Geheimnisse in eine ConfigMap. Ein Geheimnis (`netbox_token`,
  `netbox_write_token`, das `password` einer Zugangsangabe oder des Proxys) ist entweder `value`
  (das Chart legt es in ein eigenes Secret) oder `secretKeyRef` auf ein Secret des Betreibers und
  kommt als `APP_SETTINGS__…` an. Der Name einer Zugangsangabe mit geheimem Passwort muss klein
  geschrieben sein (Umgebungsvariablen kommen klein an), sonst bricht das Chart ab. Eine
  geänderte Konfiguration startet den Pod neu (Prüfsummen-Annotation).
- **Log:** `logLevel` (Vorgabe `info`) wird `LOG_LEVEL`, mit Stufen pro Modul nach env_logger
  (`info,backend::device=debug`); ohne sie loggt das Binary alles ab `debug`.
- **Netz nach innen:** Die Container-Ports folgen `server_port` und `server_mgmt_port`; ein
  `Service` für beide, `ServiceMonitor` optional. Statt Ingress eine `HTTPRoute` (Gateway API) an
  ein Gateway des Betreibers; die OIDC-Callback-URL ist ihr Host ([Login](login.md)).
- **Probes** auf `/health` am Management-Port. Das Backend lädt beim Start die OIDC-Discovery und
  startet ohne sie nicht ([Login](login.md)); die Startup-Probe braucht dafür Geduld.
- **Netz nach aussen:** Der Pod muss die Geräte erreichen, direkt oder über den Management-Proxy
  (`management_proxy`, SOCKS5 auf einem Router). Mit Proxy reicht ausgehend TCP zum Proxy, zu
  NetBox und zum OIDC-Provider; eine `NetworkPolicy` kann genau das erlauben.
- Nur lesbares Dateisystem, ohne Rechte.

## GitHub Actions

- **`.github/workflows/ci.yaml`:** bei jedem Push `nix flake check`, das Image und `helm lint`.
  - Ein Push auf `master` veröffentlicht einen **Snapshot**: das Image
    `ghcr.io/koa/netbox-provisioner:sha-<kurz>` und das Chart in `oci://ghcr.io/koa/charts` als
    Vorabversion der nächsten Release (`version` in `Chart.yaml`), nach Zeit geordnet
    (`0.1.0-snapshot.20261024123456.sha-abc1234`), mit dem Snapshot-Image als Vorgabe.
  - Ein Tag `vX.Y.Z` veröffentlicht Image und Chart als **Release** `X.Y.Z`
    (`.github/scripts/publish.sh`, mit `skopeo`, ohne Docker). Nach einer Release gehört
    `version` in `Chart.yaml` auf die nächste. Release-Images werden nie gelöscht.
- **`.github/workflows/cleanup.yaml`:** täglich; behält vom Image und vom Chart nur den neuesten
  Snapshot und löscht alle anderen samt unversionierten Resten (`.github/scripts/cleanup.sh`, von
  Hand mit `dry_run`). GHCR löscht eine Version mit allen Tags, und ein Release-Image teilt den
  Digest mit dem Snapshot seines Commits: gelöscht wird nur, was ausschliesslich Snapshot-Tags
  trägt.
- Die CHR-Tests (`#[ignore]`) laufen nicht in der CI: Sie brauchen KVM-lose QEMU-Läufe von
  mehreren Minuten.
- **Rollout mit Argo CD:** Argo CD deployt das Helm-Chart mit dem Image-Tag; es führt kein Nix aus.
  Prod zieht nur Release-Versionen, ein Snapshot lässt sich gezielt in eine Test-Umgebung
  ausrollen.

## Noch nicht geprüft

Das Image lief noch in keinem Cluster, und beide Workflows sind auf GitHub noch nicht gelaufen
(E3). Ohne OpenSSL (rustls, dann musl und ein Image nur aus Binary und Zertifikaten) ist ein
eigener Schritt (E4).
