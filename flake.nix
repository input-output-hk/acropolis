{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-25.05";
    flake-parts.url = "github:hercules-ci/flake-parts";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    treefmt-nix.inputs.nixpkgs.follows = "nixpkgs";
    crane.url = "github:ipetkov/crane";
    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";
    devshell.url = "github:numtide/devshell";
    devshell.inputs.nixpkgs.follows = "nixpkgs";
    cardano-playground.url = "github:input-output-hk/cardano-playground/56ebfef5595c43014029b039ade01b0ef06233e0";
    cardano-playground.flake = false; # otherwise, +9k dependencies in flake.lock…
    sanchonet.url = "github:Hornan7/SanchoNet-Tutorials";
    sanchonet.flake = false;
    advisory-db.url = "github:rustsec/advisory-db";
    advisory-db.flake = false;
  };

  outputs = inputs: let
    inherit (inputs.nixpkgs) lib;
  in
    inputs.flake-parts.lib.mkFlake {inherit inputs;} ({config, ...}: {
      imports = [
        inputs.devshell.flakeModule
        inputs.treefmt-nix.flakeModule
      ];

      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
        "x86_64-darwin"
      ];
      perSystem = {system, ...}: let
        internal = inputs.self.internal.${system};
      in {
        packages =
          {
            default = internal.acropolis-process-omnibus;
            inherit (internal) acropolis-process-omnibus acropolis-process-replayer;
          }
          // (lib.optionalAttrs (system == "x86_64-linux") {
            acropolis-process-omnibus-x86_64-windows = inputs.self.internal.x86_64-windows.acropolis-process-omnibus;
            acropolis-process-replayer-x86_64-windows = inputs.self.internal.x86_64-windows.acropolis-process-replayer;
          });

        devshells.default = import ./nix/devshells.nix {inherit inputs;};

        checks = internal.cargoChecks // internal.nixChecks;

        treefmt =
          /*
          { pkgs, ...}
          */
          _: {
            projectRootFile = "flake.nix";
            programs = {
              alejandra.enable = true; # Nix
              # TODO: enable them one by one (large commits, mostly whitespace):
              #prettier.enable = true;
              #rustfmt.enable = true;
              #rustfmt.package = internal.rustfmt;
              #yamlfmt.enable = pkgs.system != "x86_64-darwin"; # a treefmt-nix+yamlfmt bug on Intel Macs
              #taplo.enable = true; # TOML
              #shfmt.enable = true;
            };
            # settings.formatter.rustfmt.options = [
            #   "--config-path"
            #   (builtins.path {
            #     name = "rustfmt.toml";
            #     path = ./rustfmt.toml;
            #   })
            # ];
          };
      };

      flake = {
        internal =
          lib.genAttrs config.systems (
            targetSystem: import ./nix/internal/unix.nix {inherit inputs targetSystem;}
          )
          // lib.genAttrs ["x86_64-windows"] (
            targetSystem: import ./nix/internal/windows.nix {inherit inputs targetSystem;}
          );

        hydraJobs = let
          crossSystems = ["x86_64-windows"];
          allJobs = {
            acropolis-process-omnibus = lib.genAttrs (config.systems ++ crossSystems) (
              targetSystem: inputs.self.internal.${targetSystem}.acropolis-process-omnibus
            );
            acropolis-process-replayer = lib.genAttrs (config.systems ++ crossSystems) (
              targetSystem: inputs.self.internal.${targetSystem}.acropolis-process-replayer
            );
            devshell = lib.genAttrs config.systems (
              targetSystem: inputs.self.devShells.${targetSystem}.default
            );
            inherit (inputs.self) checks;
          };
        in
          allJobs
          // {
            required = inputs.nixpkgs.legacyPackages.x86_64-linux.releaseTools.aggregate {
              name = "github-required";
              meta.description = "All jobs required to pass CI";
              constituents = lib.collect lib.isDerivation allJobs;
            };
          };

        nixConfig = {
          extra-substituters = ["https://cache.iog.io"];
          extra-trusted-public-keys = ["hydra.iohk.io:f/Ea+s+dFdN+3Y/G+FDgSq+a5NEWhJGzdjvKNGv0/EQ="];
          allow-import-from-derivation = "true";
        };
      };
    });
}
