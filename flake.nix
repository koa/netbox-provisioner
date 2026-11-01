{
  description = "netbox-provisioner: provisions a MikroTik RouterOS network from NetBox";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # builds the dependencies apart from the crates, so they come from the cache
    crane.url = "github:ipetkov/crane";
    # the two libraries are path dependencies (`../../mikrotik-model`); the build puts them next
    # to this repository, as they are in the workspace. Locally:
    #   nix build --override-input mikrotik-model path:../mikrotik-model \
    #     --override-input mikrotik-api path:../mikrotik-api
    mikrotik-model = {
      url = "github:koa/mikrotik-model/claude-edition";
      flake = false;
    };
    mikrotik-api = {
      url = "github:koa/mikrotik-api/claude-edition";
      flake = false;
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      crane,
      mikrotik-model,
      mikrotik-api,
    }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };
      lib = pkgs.lib;
      toolchain = pkgs.rust-bin.stable.latest.default.override {
        targets = [ "wasm32-unknown-unknown" ];
      };
      craneLib = (crane.mkLib pkgs).overrideToolchain (_: toolchain);
      # trunk runs the wasm-bindgen of the version in Cargo.lock
      wasmBindgen = pkgs.wasm-bindgen-cli_0_2_129;

      # the workspace as the path dependencies expect it: this repository and the two libraries
      # side by side; only tracked files, without build output or the lab's secrets
      own = lib.cleanSourceWith {
        src = self;
        filter =
          path: type:
          !(builtins.elem (baseNameOf path) [
            "target"
            "dist"
            "node_modules"
            ".lab-netbox"
            ".chr-test-cache"
            "config.yaml"
          ]);
      };
      sideBySide =
        name: ownSrc: modelSrc: apiSrc:
        pkgs.runCommand name { } ''
          mkdir -p $out
          cp -r ${ownSrc} $out/netbox-provisioner
          cp -r ${modelSrc} $out/mikrotik-model
          cp -r ${apiSrc} $out/mikrotik-api
          chmod -R u+w $out
        '';
      workspace = sideBySide "workspace" own mikrotik-model mikrotik-api;
      # the same with this repository's crates dummied, for building only the dependencies: it
      # changes only with a Cargo.toml or Cargo.lock here or with flake.lock, so the dependencies
      # come from the cache. The two libraries stay whole: they change only with flake.lock, and
      # mikrotik-model is the biggest crate of all. Same name as `workspace`, so both unpack to
      # the same path and cargo takes the dependencies as fresh.
      dummyWorkspace =
        sideBySide "workspace" (craneLib.mkDummySrc { src = own; }) mikrotik-model
          mikrotik-api;

      # what every build shares: the vendored crates, cargo run in this repository's directory
      common = {
        pname = "netbox-provisioner";
        version = "0.1.0";
        src = workspace;
        # cargo runs in this repository, next to the two libraries
        postUnpack = ''
          sourceRoot="$sourceRoot/netbox-provisioner"
        '';
        cargoVendorDir = craneLib.vendorCargoDeps { cargoLock = ./Cargo.lock; };
        strictDeps = true;
        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = [ pkgs.openssl ];
      };
      # the dependencies alone, built from the dummied workspace with the commands of the
      # builds that inherit them, so cargo finds them fresh there
      deps =
        name: attrs:
        craneLib.buildDepsOnly (
          common
          // {
            pname = "netbox-provisioner-${name}";
            src = null;
            dummySrc = dummyWorkspace;
            doCheck = false;
          }
          // attrs
        );

      # the tests and the clippy runs of backend and binary (debug)
      nativeDevDeps = deps "native-dev" {
        CARGO_PROFILE = "dev";
        buildPhaseCargoCommand = ''
          cargo clippy --locked -p backend --all-targets
          cargo clippy --locked -p binary
          cargo test --locked -p backend --no-run
        '';
      };
      # the binary (release)
      nativeReleaseDeps = deps "native-release" {
        buildPhaseCargoCommand = ''
          cargo build --locked --release -p binary
        '';
      };
      # the frontend (wasm, release) and its clippy run, with the build script's host crates
      wasmDeps = deps "wasm" {
        buildPhaseCargoCommand = ''
          cargo build --locked --release -p frontend --target wasm32-unknown-unknown
          cargo clippy --locked --release -p frontend --target wasm32-unknown-unknown
        '';
      };

      # the frontend once, embedded by the binary (`binary/build.rs` only checks that `dist` is
      # there) and needed by the clippy run of the binary
      frontend = craneLib.mkCargoDerivation (
        common
        // {
          pname = "netbox-provisioner-frontend";
          cargoArtifacts = wasmDeps;
          npmDeps = pkgs.fetchNpmDeps {
            src = ./frontend/node;
            hash = "sha256-/eB4Wl1zm8x6I+mvgVX8Np/OPggI+pkKUy+Tei+WEQU=";
          };
          npmRoot = "frontend/node";
          nativeBuildInputs = common.nativeBuildInputs ++ [
            pkgs.trunk
            pkgs.dart-sass
            wasmBindgen
            pkgs.nodejs
            pkgs.npmHooks.npmConfigHook
          ];
          buildPhaseCargoCommand = ''
            (cd frontend && trunk build --release --offline)
          '';
          doInstallCargoArtifacts = false;
          installPhaseCommand = ''
            cp -r frontend/dist $out
          '';
        }
      );
      withFrontend = ''
        cp -r ${frontend} frontend/dist
        chmod -R u+w frontend/dist
      '';

      provisioner = craneLib.buildPackage (
        common
        // {
          cargoArtifacts = nativeReleaseDeps;
          cargoExtraArgs = "--locked -p binary";
          preBuild = withFrontend;
          # the checks run them (`checks.tests`)
          doCheck = false;
          postInstall = ''
            mv $out/bin/binary $out/bin/netbox-provisioner
          '';
        }
      );

      # a check is a build of the workspace that runs cargo and leaves only a stamp
      check =
        name: artifacts: script:
        craneLib.mkCargoDerivation (
          common
          // {
            pname = "netbox-provisioner-${name}";
            cargoArtifacts = artifacts;
            buildPhaseCargoCommand = script;
            doInstallCargoArtifacts = false;
          }
        );
    in
    {
      packages.${system} = {
        default = provisioner;
        inherit provisioner frontend;
        # an OCI image of the binary and the CA certificates, nothing else: no shell, no
        # coreutils (`docs/deployment.md`)
        image = pkgs.dockerTools.buildLayeredImage {
          name = "netbox-provisioner";
          tag = self.shortRev or "dirty";
          contents = [ pkgs.cacert ];
          config = {
            Entrypoint = [ "${provisioner}/bin/netbox-provisioner" ];
            # `config.yaml` is read from the working directory
            WorkingDir = "/config";
            User = "65532:65532";
            ExposedPorts = {
              "8080/tcp" = { };
              "9080/tcp" = { };
            };
            Env = [ "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" ];
          };
        };
      };

      checks.${system} = {
        # the CHR tests are `#[ignore]`: they boot VMs
        tests = check "tests" nativeDevDeps ''
          cargo test --locked -p backend
        '';
        clippy = check "clippy" nativeDevDeps ''
          cargo clippy --locked -p backend --all-targets -- -D warnings
          ${withFrontend}
          cargo clippy --locked -p binary -- -D warnings
        '';
        clippy-frontend = check "clippy-frontend" wasmDeps ''
          cargo clippy --locked --release -p frontend --target wasm32-unknown-unknown -- -D warnings
        '';
        # the image holds no shell or interpreter, also when a new dependency would bring one
        no-shell = pkgs.runCommand "no-shell" { closure = pkgs.closureInfo { rootPaths = [ provisioner ]; }; } ''
          if grep -E -- '-(bash|coreutils|busybox|python3|perl)-[0-9]' $closure/store-paths; then
            echo "the provisioner's closure holds a shell or interpreter" >&2
            exit 1
          fi
          touch $out
        '';
      };

      devShells.${system}.default = pkgs.mkShell {
        packages = [
          toolchain
          pkgs.trunk
          pkgs.dart-sass
          wasmBindgen
          pkgs.nodejs
          pkgs.pkg-config
          pkgs.openssl
        ];
      };

      formatter.${system} = pkgs.nixfmt;
    };
}
