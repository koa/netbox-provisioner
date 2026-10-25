# Konzept: Container, Helm-Chart und CI

Status: umgesetzt bis auf rustls (2026-10-24, siehe
[Umsetzung](#umsetzung)). Offener Punkt „Provisionierung“ aus
[pending.md](../pending.md#provisionierung).

## Ausgangslage

- **Ein Binary** (`binary`): Der Build ruft `trunk build --release` (`binary/build.rs`), das
  Frontend steckt danach als statische Dateien im Binary (`ResourceFiles`). Zum Bauen braucht es
  Rust, `trunk`, `dart-sass` und das Ziel `wasm32-unknown-unknown`.
- **Ports:** API und UI auf `server_port` (Vorgabe 8080), Management auf `server_mgmt_port`
  (Vorgabe +1000, also 9080) mit `/metrics` (Prometheus) und `health`.
- **Konfiguration:** `config.yaml` im Arbeitsverzeichnis. Einzelne Werte übersteuern
  Umgebungsvariablen mit `__` als Trenner der Ebenen, etwa `APP_SETTINGS__NETBOX_TOKEN` für
  `settings.netbox_token` (seit 2026-10-23; Schlüssel kommen kleingeschrieben an). In der Datei
  stehen Geheimnisse: NetBox-Tokens, `mikrotik_credentials`, das Passwort des
  Management-Proxys.
- **Zustand:** nur die weiterführbaren Aufträge in `state_dir` (Vorgabe `state` neben
  `config.yaml`, mit Sperrdatei gegen eine zweite Instanz; [Gerätezustand](device-state.md),
  „Speicher“). Alle anderen Jobs leben im Prozess.
- **TLS:** Das Binary linkt OpenSSL (`openssl-sys`, über `native-tls`), nicht nur rustls.
- `build.sh` endet mit `podman build .`, aber im Repository gibt es kein Containerfile.

## Container

Ziel aus `pending.md`: „basierend auf bare (kein volles Linux)“.

- **Mehrstufig:** Build-Stufe mit Rust, `trunk`, `dart-sass`, wasm-Ziel; Laufzeit-Stufe nur mit
  dem Binary.
- **Laufzeit:** `scratch` geht nur mit statischem Binary (musl, OpenSSL `vendored`). Einfacher ist
  `gcr.io/distroless/cc` (glibc, CA-Zertifikate, sonst nichts) plus `libssl`; noch besser die
  Abhängigkeit auf rustls umstellen, dann reicht `distroless/cc` allein bzw. `scratch` mit musl.
  Vorschlag: zuerst `distroless/cc` mit OpenSSL, die Umstellung auf rustls als eigener Schritt.
- **Benutzer:** nicht root (`USER 65532`), Arbeitsverzeichnis mit `config.yaml` als Volume.
- **Gebaut mit Nix statt mit einem Containerfile** (Variante, siehe unten): Nix ersetzt nicht
  den Container, sondern den Weg, das Image zu bauen. Heraus kommt ein normales OCI-Image; der
  Cluster sieht keinen Unterschied.

### Variante: Image mit Nix

`pkgs.dockerTools.buildLayeredImage` packt genau die Laufzeit-Abhängigkeiten (die Closure) des
Binaries ins Image, sonst nichts: keine Shell, kein Paketmanager, keine coreutils, solange niemand
sie dazunimmt. Die Toolchain (Rust samt wasm-Ziel über `rust-overlay`, `trunk`, `dart-sass`,
`wasm-bindgen-cli` passend zu `Cargo.lock`) ist im Ausdruck festgelegt; Lab, CI und Image bauen
gleich.

**Versuch** (2026-10-19, nixpkgs `26.11pre`, Rust 1.99.1, `buildRustPackage` mit `cargoLock`,
`fetchNpmDeps` für PatternFly, das Frontend mit `trunk build --release --offline` vor dem
Binary): Der Build ging auf Anhieb durch, nachdem die beiden festen Hashes (npm, `graphql_client`
aus Git) eingetragen waren.

- **Closure:** das Binary (74 MiB, Frontend eingebettet), glibc (34 MiB, davon 21 MiB `share`),
  `gcc-lib`/`libgcc` (10 MiB), OpenSSL (9 MiB), `libidn2`, `libunistring`, dazu `cacert`. Image
  50 MiB gepackt, 133 MiB entpackt; im Dateisystem nur `/nix`, `/etc` (Zertifikate),
  `/nix-support`, kein `/bin`.
- **Ausführbar** ist darin ausser dem Provisioner nur `glibc/libexec/getconf/*` (drei kleine
  Hilfsdateien). Keine Shell, kein Interpreter. Der Provisioner selbst startet zur Laufzeit keine
  Programme (`Command::new` nur in Tests; SSH über `russh`).
- Das Binary startet und bricht ohne `config.yaml` ab, wie erwartet. Im Container laufen liess
  es sich hier nicht (kein podman); das bleibt zu prüfen.

**Was das Repository dafür braucht** (im Versuch per Patch gelöst):

- `binary/build.rs` startet `trunk` mit `spawn()` und wartet nicht. Das Binary kann so ein
  halbfertiges `frontend/dist` einbetten. Besser: das Frontend vorher bauen und `build.rs` nur
  prüfen lassen, dass `dist` existiert (oder auf das Ende warten).
- Die Hooks in `frontend/Trunk.toml` (`clear`, `npm install`) brauchen ein Terminal und Netz; im
  Nix-Build kommen die Module aus `fetchNpmDeps`. Die Hooks gehören in ein Skript für die lokale
  Entwicklung, nicht in den Build.
- Die Pfad-Abhängigkeiten `../../mikrotik-model` und `../../mikrotik-api` liegen ausserhalb des
  Repositories. Ein Flake im Repository sieht sie nicht; für CI braucht es Git-Abhängigkeiten (die
  auskommentierten Zeilen in `backend/Cargo.toml`) oder Flake-Inputs für die beiden Repos.

**In der CI prüfen:** `nix path-info -r` über das Binary; taucht `bash`, `coreutils` oder ein
Interpreter auf, schlägt der Build fehl. So bleibt das Image ohne Shell, auch wenn eine neue
Abhängigkeit eine mitbringen würde.

**Noch kleiner:** mit musl und statischem OpenSSL (oder rustls) fällt glibc weg; das Image ist
dann das Binary und die Zertifikate.

**Fehlersuche ohne Shell:** `kubectl debug -it <pod> --image=busybox --target=<container>`
hängt bei Bedarf einen ephemeren Debug-Container an.

## Helm-Chart

- `Deployment` mit **einer** Replik: Jobs, Leases (`device/power.rs`) und das Abbrechen leben im
  Prozess. Zwei Repliken würden dasselbe Gerät gleichzeitig anwenden. Strategie `Recreate`, damit
  beim Update nie zwei laufen. `state_dir` ist das Volume `state` unter `/state`:
  `persistence.existingClaim`, sonst ein `emptyDir`, das einen Neustart des Containers
  übersteht, aber keinen neuen Pod. Beim Beenden lässt der Server weiterführbare Aufträge
  liegen und bricht die anderen ab (bis 20 s innerhalb der 60 s Gnadenfrist).
- `config.yaml` aus einzelnen Werten, die Geheimnisse als Umgebungsvariable `APP_SETTINGS__…`
  aus Secrets (siehe [Umsetzung](#umsetzung)).
- `Service` für 8080, ein zweiter für 9080 (Metriken), `ServiceMonitor` optional.
- `HTTPRoute` (Gateway API) vor 8080; OIDC-Callback-URL = ihr Host ([Login](../login.md)).
- Probes auf `health` am Management-Port. Achtung: Das Backend lädt beim Start die
  OIDC-Discovery und startet ohne sie nicht ([Login](../login.md)); die Startup-Probe braucht
  dafür Geduld.
- **Netz:** Der Pod muss die Geräte erreichen, direkt oder über den Management-Proxy
  (`management_proxy`, SOCKS5 auf einem Router). Mit Proxy reicht ausgehend TCP zum Proxy, zu
  NetBox und zum OIDC-Provider; eine `NetworkPolicy` kann genau das erlauben.
- `terminationGracePeriodSeconds` (60 s) länger als das geordnete Beenden: bis 30 s für offene
  Anfragen (actix), dann bis 20 s für die abgebrochenen Aufträge ([Jobs](job-polling.md)).

## GitHub Actions

- **Bei jedem Push:** Tests (`cargo test`, ohne die `#[ignore]`-CHR-Tests), die drei Clippy-Läufe,
  dann das Image bauen.
- **Snapshot:** Tag aus dem Commit (`sha-<kurz>`), Push nach `ghcr.io/koa/netbox-provisioner`.
  Alte Snapshots löscht ein geplanter Workflow (z.B. `actions/delete-package-versions`, alle
  `sha-*` älter als 30 Tage, die nicht zugleich ein Release-Tag tragen).
- **Release:** Ein Git-Tag `vX.Y.Z` baut dasselbe Image mit dem Versions-Tag und dem Chart
  (`helm package`, als OCI-Artefakt nach ghcr). Release-Images werden nie gelöscht.
- **Rollout mit Argo CD:** Argo CD deployt das Helm-Chart aus Git mit dem Image-Tag; es führt
  kein Nix aus (Nix baut nur das Image in der CI). Prod zieht nur Release-Versionen, ein Snapshot
  lässt sich gezielt in eine Test-Umgebung ausrollen. Manifeste in Nix (nixidy, kubenix) mit
  gerenderten YAML in einem Git-Repo gingen auch, lohnen sich für ein einzelnes Deployment aber
  nicht.
- Die CHR-Tests brauchen KVM-lose QEMU-Läufe von mehreren Minuten; als eigener, manuell oder
  nächtlich gestarteter Workflow.
- Nach `CLAUDE.md` veröffentlicht ein Push auf `master` dann; Pushes bleiben auf Anfrage.

## Reihenfolge

1. Image mit Nix (Variante oben): `build.rs` und die Trunk-Hooks bereinigen, Git- oder
   Flake-Abhängigkeiten für die beiden Bibliotheken, Flake ins Repository, Image mit podman
   starten. Alternativ ein Containerfile auf `distroless/cc`.
2. Workflow: Tests, Clippy, Image mit Snapshot-Tag.
3. Helm-Chart, im Lab-Cluster (falls vorhanden) ausrollen.
4. Release-Workflow und Aufräumen der Snapshots.
5. Optional: rustls statt OpenSSL, dann `scratch`/musl.

## Entschieden (2026-10-23)

- Das Image baut **Nix**.
- Der Provisioner liefert **Image und Helm-Chart**; die Konfiguration des Clusters (Argo CD,
  Secrets, Ingress) macht der Betreiber von Hand.

## Umsetzung

- **`flake.nix`:** `packages.image` (Nix, `buildLayeredImage`, Arbeitsverzeichnis `/config`,
  Benutzer 65532), `packages.provisioner` (das Binary als `netbox-provisioner`), `checks` (Tests
  ohne CHR, die drei Clippy-Läufe mit `-D warnings`, `no-shell` über die Closure), eine
  `devShell`. `mikrotik-model` und `mikrotik-api` sind Flake-Inputs (GitHub, Zweig
  `claude-edition`), der Build legt sie neben das Repository, wo die Pfad-Abhängigkeiten sie
  finden; `Cargo.toml` bleibt dabei, wie es ist.
- **`.github/workflows/ci.yaml`:** bei jedem Push `nix flake check`, das Image und `helm lint`.
  Ein Push auf `master` veröffentlicht einen Snapshot: das Image
  `ghcr.io/koa/netbox-provisioner:sha-<kurz>` und das Chart in `oci://ghcr.io/koa/charts` als
  Vorabversion der nächsten Release (`version` in `Chart.yaml`), nach Zeit geordnet
  (`0.1.0-snapshot.20261024123456.sha-abc1234`), mit dem Snapshot-Image als Vorgabe. Ein Tag
  `vX.Y.Z` veröffentlicht Image und Chart als `X.Y.Z` (`.github/scripts/publish.sh`, mit
  `skopeo`, ohne Docker). Nach einer Release gehört `version` in `Chart.yaml` auf die nächste.
- **`.github/workflows/cleanup.yaml`:** täglich; behält vom Image und vom Chart nur den neuesten
  Snapshot und löscht alle anderen samt unversionierten Resten, lieber zu viele als zu wenige:
  Snapshots sind zum kurzen Ausprobieren da (`.github/scripts/cleanup.sh`, von Hand mit
  `dry_run`). GHCR löscht eine Version mit allen Tags, und ein Release-Image teilt den Digest
  mit dem Snapshot seines Commits: gelöscht wird nur, was ausschliesslich Snapshot-Tags trägt.
- **`charts/netbox-provisioner`:** eine Replik mit `Recreate`. Jedes Setting von `config.yaml`
  ist ein eigener Wert unter `settings`; das Chart schreibt sie ohne die Geheimnisse in eine
  ConfigMap. Ein Geheimnis (`netbox_token`, `netbox_write_token`, das `password` einer
  Zugangsangabe oder des Proxys) ist entweder `value` (das Chart legt es in ein eigenes Secret)
  oder `secretKeyRef` auf ein Secret des Betreibers und kommt als `APP_SETTINGS__…` an. Der Name
  einer Zugangsangabe mit geheimem Passwort muss klein geschrieben sein (Umgebungsvariablen
  kommen klein an), sonst bricht das Chart ab. Die Container-Ports folgen `server_port` und
  `server_mgmt_port`. `logLevel` (Vorgabe `info`) wird `LOG_LEVEL`, mit Stufen pro Modul nach
  env_logger (`info,backend::device=debug`); ohne sie loggt das Binary alles ab `debug`. Statt Ingress eine `HTTPRoute` (Gateway API) an ein Gateway des
  Betreibers, `ServiceMonitor` optional, Probes auf `/health`, nur lesbares Dateisystem ohne
  Rechte; eine geänderte Konfiguration startet den Pod neu (Prüfsummen-Annotation).
- **Offen:** rustls statt OpenSSL, das Image in einem Cluster laufen lassen; beide Workflows
  sind auf GitHub noch nicht gelaufen.
