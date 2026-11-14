# Container, Helm-Chart und CI

Der Provisioner liefert ein OCI-Image und ein Helm-Chart. Die Konfiguration des Clusters (Argo
CD, Secrets, Gateway) macht der Betreiber von Hand.

## Das Binary

- **Ein Binary** (`binary`) mit dem Frontend als statischen Dateien (`ResourceFiles`). Das
  Frontend wird vorher gebaut (`build.sh`, `local/lab/run-provisioner.sh`, im Flake ein eigener
  Schritt); `binary/build.rs` prüft nur, dass `frontend/dist` da ist. Zum Bauen braucht es Rust
  mit dem Ziel `wasm32-unknown-unknown`, `trunk`, `dart-sass` und `wasm-bindgen-cli`.
- **Ports:** API und UI auf `server_port` (Vorgabe 8080), Management auf `server_mgmt_port`
  (Vorgabe +1000, also 9080) mit `/metrics` (Prometheus; dort auch die
  [Gerätemetriken](device-metrics.md)) und `/health`. Das Grafana-Dashboard
  der Geräteabfragen ([Gerätezustand](device-state.md#dashboard)) liegt im Chart
  ([dashboards/device-queries.json](../charts/netbox-provisioner/dashboards/device-queries.json));
  ohne Chart über Dashboards → Import, Datenquelle Prometheus wählen.
- **Konfiguration:** `config.yaml` im Arbeitsverzeichnis. Einzelne Werte übersteuern
  Umgebungsvariablen mit `__` als Trenner der Ebenen, etwa `APP_SETTINGS__NETBOX_TOKEN` für
  `settings.netbox_token` (Schlüssel kommen kleingeschrieben an). In der Datei stehen
  Geheimnisse: NetBox-Tokens, `mikrotik_credentials`, das Passwort des Management-Proxys.
- **Passwortwechsel der Geräte (A9):** ein Eintrag von `mikrotik_credentials` hat neben
  `password` ein `previous_password` (im Chart ebenfalls `value` oder `secretKeyRef`). Für einen
  Wechsel kommt das neue nach `password`, das alte nach `previous_password`. Der Provisioner
  meldet sich zuerst mit dem neuen an, abgelehnt mit dem alten: über die API, beim SSH-Login am
  Nachbarn (Tunnel, Konsole der Ersteinrichtung) und beim mac-telnet-Login der Ersteinrichtung.
  Ein Gerät mit dem alten zeigt auf der Geräteseite eine Abweichung, sein Anwenden setzt das
  neue; es geschieht nichts von selbst. Sind alle umgestellt, fällt `previous_password` weg.
- **WebFig über HTTPS (A7):** mit `www_ca` (`certificate` und `key`, PEM; im Chart beide als
  `value` oder `secretKeyRef`, `www_ca_certificate` und `www_ca_key`, etwa `tls.crt` und
  `tls.key` des Secrets eines cert-manager-Certificate mit `isCA: true` und
  `privateKey.rotationPolicy: Never`: ein neuer Schlüssel hiesse eine neue CA in den
  Browsern) stellt der Provisioner jedem Gerät ein Zertifikat aus: ECDSA P-256, 397 Tage,
  mit allen Adressen des Geräts aus NetBox (ohne Link-Local) und seinem Namen, wo der ein
  DNS-Name ist. Die CA braucht
  einen Subject Key Identifier (`openssl req -x509` setzt ihn). Das Anwenden legt es an, wenn
  keines passt (andere Adressen, andere CA, weniger als 30 Tage übrig), hängt es an `www-ssl`
  und entfernt ältere des Provisioners (`netbox-provisioner-www-<Sekunde>`), die kein anderer
  Dienst nennt; erst dann schaltet die Konfiguration `www-ssl` ein (wie `www` auf
  `api_networks` beschränkt) und `www` aus. Die Admins importieren das CA-Zertifikat einmal in
  den Browser. Ohne `www_ca` bleibt WebFig auf HTTP. Eine CA, die sich nicht lesen lässt, hält
  den Start auf.
- **Zustand:** nur die weiterführbaren Aufträge in `state_dir` (Vorgabe `state` neben
  `config.yaml`, mit Sperrdatei gegen eine zweite Instanz;
  [Jobs](jobs.md#weiterführen-nach-einem-neustart)). Alle anderen Jobs leben im Prozess.
- **TLS:** nur rustls (Provider aws-lc-rs), kein OpenSSL mehr seit reqwest 0.13. Die
  Wurzelzertifikate kommen aus dem System bzw. `SSL_CERT_FILE` (`rustls-native-certs`), auch
  für den Aussteller der Anmeldung (`actix-4-jwt-auth`, native roots).
- Zur Laufzeit startet der Provisioner keine Programme (`Command::new` nur in Tests; SSH über
  `russh`).

## Image mit Nix

`flake.nix`, `packages.image`: `pkgs.dockerTools.buildLayeredImage` packt genau die
Laufzeit-Abhängigkeiten (die Closure) des Binaries ins Image, sonst nichts: keine Shell, kein
Paketmanager, keine coreutils. Arbeitsverzeichnis `/config`, Benutzer 65532. Die Toolchain (Rust
samt wasm-Ziel über `rust-overlay`, `trunk`, `dart-sass`, `wasm-bindgen-cli` passend zu
`Cargo.lock`) ist im Ausdruck festgelegt, so bauen Lab, CI und Image gleich. Heraus kommt ein
normales OCI-Image.

- **Closure:** nur das Binary, statisch gegen musl gelinkt (`x86_64-unknown-linux-musl`, rund
  100 MiB mit dem eingebetteten Frontend); die C-Teile der Crates (aws-lc-rs, ring) baut der
  gcc von `pkgsCross.musl64`. Im Image liegen daneben nur die Zertifikate (`cacert`), keine
  libc, keine Shell. Bis 2026-11-06 war es glibc mit `gcc-lib`, OpenSSL, `libidn2` und
  `libunistring` (Image rund 50 MiB gepackt, 133 MiB entpackt). Lab-Test 2026-11-06: das
  musl-Binary mit Anmeldung, NetBox, Gerät und den RouterOS-Versionen über HTTPS.
- **Gebaut mit [crane](https://crane.dev):** Die Abhängigkeiten der Crates sind eigene
  Ableitungen, gebaut aus dem Workspace mit Attrappen statt der Quellen dieses Repositorys
  (`mkDummySrc`); sie ändern sich nur mit einem `Cargo.toml`, `Cargo.lock` oder `flake.lock`.
  Drei davon: nativ im Debug-Profil für Tests und Clippy, nativ im Release-Profil für das Binary,
  wasm für das Frontend (mit den Host-Crates seines Build-Skripts). Die beiden Bibliotheken sind
  darin ganz enthalten, weil sie sich nur mit `Cargo.lock` ändern. Das Frontend ist eine eigene
  Ableitung (`packages.frontend`), einmal gebaut für das Binary und den Clippy-Lauf des Binaries.
  Tests, Clippy und Binary übernehmen die Abhängigkeiten (`cargoArtifacts`) und bauen nur die
  eigenen Crates.
- **`checks`:** die Tests ohne CHR, die drei Clippy-Läufe mit `-D warnings` (Backend und Binary,
  Frontend für wasm), `fmt` (`cargo fmt --check`) und `only-binary`: Hält
  die Closure mehr als das Binary (eine libc, eine Shell, einen Interpreter), schlägt der Check
  fehl. So bleibt das Image so, auch wenn eine neue Abhängigkeit etwas mitbrächte.
- **Bibliotheken:** `mikrotik-model` und `mikrotik-api` sind Git-Abhängigkeiten in `Cargo.toml`
  (GitHub, Zweig `master`), festgelegt in `Cargo.lock` und mit den anderen Crates
  vendort. Eine Änderung dort kommt erst ins Image, wenn sie gepusht und
  `cargo update -p mikrotik-model` (oder `mikrotik-api`) hier committet ist. Wird eine
  Bibliothek geändert, zeigt ihre Abhängigkeit in `backend/Cargo.toml` vorübergehend auf das
  Nachbarverzeichnis:

  ```toml
  mikrotik-model = { version = "0.1.0", path = "../../mikrotik-model" }
  ```

  Bei mikrotik-api zusätzlich in `mikrotik-model/Cargo.toml` (`path = "../mikrotik-api"`),
  sonst gäbe es zwei mikrotik-api. Die Pfade und das `Cargo.lock` dazu werden nie committet.
  Zum Commit wird zuerst die Bibliothek committet und gepusht, dann die Git-Referenz
  zurückgestellt, `cargo update -p <Bibliothek>` ausgeführt und der Provisioner mit dem neuen
  `Cargo.lock` committet; die Checks laufen mit `--locked` und lehnen ein Lock mit Pfaden ab.
- `packages.provisioner` ist das Binary als `netbox-provisioner`, dazu eine `devShell`.

**Fehlersuche ohne Shell:** `kubectl debug -it <pod> --image=busybox --target=<container>`
hängt bei Bedarf einen ephemeren Debug-Container an.

## Helm-Chart

`charts/netbox-provisioner`:

- **Eine Replik** mit Strategie `Recreate`: Jobs, Leases (`device/dependency.rs`) und das Abbrechen
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
  `Service` für beide, `ServiceMonitor` optional, ebenso das Grafana-Dashboard
  (`grafanaDashboard`: eine ConfigMap mit dem Label, nach dem der Grafana-Sidecar sucht, Vorgabe
  `grafana_dashboard: "1"`; ein Upgrade des Charts bringt es auf den Stand). Statt Ingress eine `HTTPRoute` (Gateway API) an
  ein Gateway des Betreibers; die OIDC-Callback-URL ist ihr Host ([Login](login.md)). Der
  Server spricht nur HTTP/1.1: HTTP/2 zum Browser macht das Gateway (bei Cilium
  `gatewayAPI.enableAlpn`, danach den Operator neu starten), zum Pod bleibt es ohne `appProtocol`
  im Service bei HTTP/1.1. So laufen die WebSockets, und h2 0.3 bleibt unerreichbar (A6 in
  [done.md](done.md)).
- **Probes** auf `/health` am Management-Port. Das Backend lädt beim Start die OIDC-Discovery und
  startet ohne sie nicht ([Login](login.md)); die Startup-Probe braucht dafür Geduld.
- **Netz nach aussen:** Der Pod muss die Geräte erreichen, direkt oder über den Management-Proxy
  (`management_proxy`, SOCKS5 auf einem Router, der nur Quellen aus `api_networks` annimmt,
  [Firewall](firewall.md)). Mit Proxy reicht ausgehend TCP zum Proxy, zu
  NetBox und zum OIDC-Provider; eine `NetworkPolicy` kann genau das erlauben.
- Nur lesbares Dateisystem, ohne Rechte.

## GitHub Actions

- **`.github/workflows/ci.yaml`:** bei jedem Push `nix flake check`, das Image und `helm lint`.
  `magic-nix-cache-action` hält den Nix-Store zwischen den Läufen im Cache von GitHub Actions,
  Pfad für Pfad (bis 10 GB pro Repository, das älteste fällt zuerst heraus): Ein Lauf baut nur,
  was sich geändert hat, ohne geänderte Abhängigkeiten also nur die eigenen Crates und das
  Frontend.
  - Ein Push auf `master` veröffentlicht einen **Snapshot**: das Image
    `ghcr.io/koa/netbox-provisioner:sha-<Commit-ID>` und das Chart in `oci://ghcr.io/koa/charts`
    als Vorabversion der nächsten Release (`version` in `Chart.yaml`), benannt nach der
    vollständigen Commit-ID (`0.1.0-snapshot.sha-<40 Zeichen>`, aus dem lokalen
    `git rev-parse HEAD`), mit dem Snapshot-Image als Vorgabe. Nach Zeit geordnet sind die
    Snapshots so nicht mehr: Helm ohne feste Version nimmt nicht den neuesten, sondern den
    lexikalisch grössten; einen Snapshot zieht man daher immer mit seiner Version.
    `cleanup.sh` wählt den neuesten nach `created_at` und bleibt richtig.
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
(E3).
