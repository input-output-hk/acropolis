{
  inputs,
  targetSystem,
}:
assert builtins.elem targetSystem ["x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin"]; let
  buildSystem = targetSystem;
  pkgs = inputs.nixpkgs.legacyPackages.${buildSystem};
  inherit (pkgs) lib;
  extendForTarget = unix:
    (
      if pkgs.stdenv.isLinux
      then import ./linux.nix
      else if pkgs.stdenv.isDarwin
      then import ./darwin.nix
      else throw "can’t happen"
    ) {inherit inputs targetSystem unix;};
in
  extendForTarget rec {
    rustPackages = inputs.fenix.packages.${pkgs.system}.stable;

    craneLib = (inputs.crane.mkLib pkgs).overrideToolchain rustPackages.toolchain;

    src = lib.cleanSourceWith {
      src = lib.cleanSource ../../.;
      filter = path: type:
        craneLib.filterCargoSources path type
        || lib.hasSuffix ".cbor" path
        || lib.hasSuffix ".json" path;
      name = "source";
    };

    packageVersions = {
      omnibus = craneLib.crateNameFromCargoToml {cargoToml = builtins.path {path = src + "/processes/omnibus/Cargo.toml";};};
      replayer = craneLib.crateNameFromCargoToml {cargoToml = builtins.path {path = src + "/processes/replayer/Cargo.toml";};};
    };

    commonArgs =
      {
        pname = "acropolis";
        inherit (packageVersions.omnibus) version;
        inherit src;
        strictDeps = true;
        nativeBuildInputs =
          [pkgs.gnum4]
          ++ lib.optionals pkgs.stdenv.isLinux [
            pkgs.pkg-config
          ];
        buildInputs =
          lib.optionals pkgs.stdenv.isLinux [
            pkgs.openssl
          ]
          ++ lib.optionals pkgs.stdenv.isDarwin [
            pkgs.libiconv
            pkgs.darwin.apple_sdk_12_3.frameworks.SystemConfiguration
            pkgs.darwin.apple_sdk_12_3.frameworks.Security
            pkgs.darwin.apple_sdk_12_3.frameworks.CoreFoundation
          ];
      }
      // lib.optionalAttrs pkgs.stdenv.isLinux {
        # The linker bundled with Fenix has wrong interpreter path, and it fails with ENOENT, so:
        RUSTFLAGS = "-Clink-arg=-fuse-ld=bfd";
      }
      // lib.optionalAttrs pkgs.stdenv.isDarwin {
        # for bindgen, used by libproc, used by metrics_process
        LIBCLANG_PATH = "${lib.getLib pkgs.llvmPackages.libclang}/lib";
      };

    # For better caching:
    cargoArtifacts = craneLib.buildDepsOnly commonArgs;

    packageName = (craneLib.crateNameFromCargoToml {cargoToml = src + "/Cargo.toml";}).pname;

    GIT_REVISION = inputs.self.rev or "dirty";

    ACROPOLIS_OFFLINE_MIRROR = pkgs.writeText "offline-mirror.json" (builtins.toJSON {
      "https://book.world.dev.cardano.org/environments/mainnet/byron-genesis.json" = cardano-node-configs + "/mainnet/byron-genesis.json";
      "https://book.world.dev.cardano.org/environments/mainnet/shelley-genesis.json" = cardano-node-configs + "/mainnet/shelley-genesis.json";
      "https://book.world.dev.cardano.org/environments/mainnet/alonzo-genesis.json" = cardano-node-configs + "/mainnet/alonzo-genesis.json";
      "https://book.world.dev.cardano.org/environments/mainnet/conway-genesis.json" = cardano-node-configs + "/mainnet/conway-genesis.json";
      "https://raw.githubusercontent.com/Hornan7/SanchoNet-Tutorials/refs/heads/main/genesis/byron-genesis.json" = inputs.sanchonet + "/genesis/byron-genesis.json";
      "https://raw.githubusercontent.com/Hornan7/SanchoNet-Tutorials/refs/heads/main/genesis/shelley-genesis.json" = inputs.sanchonet + "/genesis/shelley-genesis.json";
      "https://raw.githubusercontent.com/Hornan7/SanchoNet-Tutorials/refs/heads/main/genesis/alonzo-genesis.json" = inputs.sanchonet + "/genesis/alonzo-genesis.json";
      "https://raw.githubusercontent.com/Hornan7/SanchoNet-Tutorials/refs/heads/main/genesis/conway-genesis.json" = inputs.sanchonet + "/genesis/conway-genesis.json";
      "https://raw.githubusercontent.com/Octalus/cardano/master/p.json" = pkgs.fetchurl {
        url = "https://raw.githubusercontent.com/Octalus/cardano/master/p.json";
        hash = "sha256-fTBfIH3RA3yEEWUGb5zGusKoEjypYelOb7OKdVdYiFg=";
      };
      "https://880w.short.gy/clrsp.json" = pkgs.fetchurl {
        url = "https://880w.short.gy/clrsp.json";
        hash = "sha256-bKOsYxccsOHqHmT+hvw7OxFopTj1PYr10a3ohW6KUKU=";
      };
    });

    cardano-node-configs = builtins.path {
      name = "cardano-playground-configs";
      path = inputs.cardano-playground + "/static/book.play.dev.cardano.org/environments";
    };

    workspace = craneLib.buildPackage (commonArgs
      // {
        inherit cargoArtifacts GIT_REVISION ACROPOLIS_OFFLINE_MIRROR;
        ACROPOLIS_OMNIBUS_DEFAULT_CONFIG = builtins.path {path = src + "/processes/omnibus/omnibus.toml";};
        ACROPOLIS_REPLAYER_DEFAULT_CONFIG = builtins.path {path = src + "/processes/replayer/replayer.toml";};
        doCheck = false; # we run tests with `cargo-nextest` below
      });

    acropolis-process-omnibus = pkgs.stdenv.mkDerivation {
      inherit (packageVersions.omnibus) pname version;
      buildCommand = ''mkdir -p $out/bin && cp ${workspace}/bin/acropolis_process_omnibus $out/bin/'';
      meta.description = "A kit of micro-service parts, written in Rust, which allows flexible construction of clients, services and APIs for the Cardano ecosystem";
    };

    acropolis-process-replayer = pkgs.stdenv.mkDerivation {
      inherit (packageVersions.replayer) pname version;
      buildCommand = ''mkdir -p $out/bin && cp ${workspace}/bin/acropolis_process_replayer $out/bin/'';
      meta.description = "Acropolis replayer process, allowing to debug any module";
    };

    # We use a newer `rustfmt`:
    inherit (inputs.fenix.packages.${pkgs.system}.stable) rustfmt;

    cargoChecks = {
      cargo-clippy = craneLib.cargoClippy (commonArgs
        // {
          inherit cargoArtifacts GIT_REVISION ACROPOLIS_OFFLINE_MIRROR;
          # Maybe also add `--deny clippy::pedantic`?
          cargoClippyExtraArgs = "--all-targets --all-features -- --deny warnings";
        });

      cargo-doc = craneLib.cargoDoc (commonArgs
        // {
          inherit cargoArtifacts GIT_REVISION ACROPOLIS_OFFLINE_MIRROR;
          RUSTDOCFLAGS = "-D warnings";
        });

      cargo-audit = craneLib.cargoAudit {
        inherit src;
        inherit (inputs) advisory-db;
      };

      cargo-deny = craneLib.cargoDeny {
        inherit src;
      };

      cargo-test = craneLib.cargoNextest (commonArgs
        // {
          inherit cargoArtifacts GIT_REVISION ACROPOLIS_OFFLINE_MIRROR;
        });
    };

    nixChecks = {
      nix-statix =
        pkgs.runCommandNoCC "nix-statix" {
          buildInputs = [pkgs.statix];
        } ''
          touch $out
          cd ${inputs.self}
          exec statix check .
        '';

      nix-deadnix =
        pkgs.runCommandNoCC "nix-deadnix" {
          buildInputs = [pkgs.deadnix];
        } ''
          touch $out
          cd ${inputs.self}
          exec deadnix --fail .
        '';

      nix-nil =
        pkgs.runCommandNoCC "nix-nil" {
          buildInputs = [pkgs.nil];
        } ''
          ec=0
          touch $out
          cd ${inputs.self}
          find . -type f -iname '*.nix' | while IFS= read -r file; do
            nil diagnostics "$file" || ec=1
          done
          exit $ec
        '';

      # From `nixd`:
      nix-nixf =
        pkgs.runCommandNoCC "nix-nil" {
          buildInputs = [pkgs.nixf pkgs.jq];
        } ''
          ec=0
          touch $out
          cd ${inputs.self}
          find . -type f -iname '*.nix' | while IFS= read -r file; do
            errors=$(nixf-tidy --variable-lookup --pretty-print <"$file" | jq -c '.[]' | sed -r "s#^#$file: #")
            if [ -n "$errors" ] ; then
              cat <<<"$errors"
              echo
              ec=1
            fi
          done
          exit $ec
        '';
    };
  }
