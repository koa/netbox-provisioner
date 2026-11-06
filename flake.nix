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
    # mikrotik-model and mikrotik-api are git dependencies in Cargo.toml, pinned by Cargo.lock
    # and vendored with the other crates
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      crane,
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

      # this repository without build output or the lab's secrets
      src = lib.cleanSourceWith {
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
      # the same with this repository's crates dummied, for building only the dependencies: it
      # changes only with a Cargo.toml or Cargo.lock, so the dependencies come from the cache
      dummySrc = craneLib.mkDummySrc { inherit src; };

      # what every build shares: the vendored crates, the git dependencies among them
      common = {
        pname = "netbox-provisioner";
        version = "0.1.0";
        inherit src;
        cargoVendorDir = craneLib.vendorCargoDeps { cargoLock = ./Cargo.lock; };
        strictDeps = true;
        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = [ pkgs.openssl ];
      };
      # the dependencies alone, built from the dummied sources with the commands of the builds
      # that inherit them, so cargo finds them fresh there
      deps =
        name: attrs:
        craneLib.buildDepsOnly (
          common
          // {
            pname = "netbox-provisioner-${name}";
            inherit dummySrc;
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

      # a check is a build of the repository that runs cargo and leaves only a stamp
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
        # every commit is formatted with `cargo fmt` (Z37)
        fmt = craneLib.cargoFmt {
          inherit src;
          pname = "netbox-provisioner";
          version = "0.1.0";
        };
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
