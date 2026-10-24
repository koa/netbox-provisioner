{
  description = "netbox-provisioner: provisions a MikroTik RouterOS network from NetBox";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
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
      rustPlatform = pkgs.makeRustPlatform {
        cargo = toolchain;
        rustc = toolchain;
      };
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
      workspace = pkgs.runCommand "workspace" { } ''
        mkdir -p $out
        cp -r ${own} $out/netbox-provisioner
        cp -r ${mikrotik-model} $out/mikrotik-model
        cp -r ${mikrotik-api} $out/mikrotik-api
        chmod -R u+w $out
      '';

      # what every build of the crates shares: sources, vendored crates and npm modules, tools
      common = {
        pname = "netbox-provisioner";
        version = "0.1.0";
        src = workspace;
        sourceRoot = "workspace/netbox-provisioner";
        cargoLock.lockFile = ./Cargo.lock;
        npmDeps = pkgs.fetchNpmDeps {
          src = ./frontend/node;
          hash = "sha256-/eB4Wl1zm8x6I+mvgVX8Np/OPggI+pkKUy+Tei+WEQU=";
        };
        npmRoot = "frontend/node";
        nativeBuildInputs = [
          pkgs.trunk
          pkgs.dart-sass
          wasmBindgen
          pkgs.nodejs
          pkgs.npmHooks.npmConfigHook
          pkgs.pkg-config
        ];
        buildInputs = [ pkgs.openssl ];
        # the binary embeds the frontend (`binary/build.rs` only checks that `dist` is there)
        preBuild = ''
          (cd frontend && trunk build --release --offline)
        '';
      };

      provisioner = rustPlatform.buildRustPackage (
        common
        // {
          cargoBuildFlags = [
            "-p"
            "binary"
          ];
          # the checks run them (`checks.tests`)
          doCheck = false;
          postInstall = ''
            mv $out/bin/binary $out/bin/netbox-provisioner
          '';
        }
      );

      # a check is a build of the workspace that runs cargo and leaves only a stamp
      check =
        name: script:
        rustPlatform.buildRustPackage (
          common
          // {
            pname = "netbox-provisioner-${name}";
            buildPhase = ''
              runHook preBuild
              ${script}
              runHook postBuild
            '';
            doCheck = false;
            installPhase = "touch $out";
          }
        );
    in
    {
      packages.${system} = {
        default = provisioner;
        inherit provisioner;
        # an OCI image of the binary and the CA certificates, nothing else: no shell, no
        # coreutils (`docs/concepts/deployment.md`)
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
        tests = check "tests" ''
          cargo test --offline -p backend
        '';
        clippy = check "clippy" ''
          cargo clippy --offline -p backend --all-targets -- -D warnings
          (cd frontend && cargo clippy --offline --target wasm32-unknown-unknown -- -D warnings)
          cargo clippy --offline -p binary -- -D warnings
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

      formatter.${system} = pkgs.nixfmt-rfc-style;
    };
}
