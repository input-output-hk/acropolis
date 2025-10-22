{
  inputs,
  targetSystem,
}:
assert builtins.elem targetSystem ["x86_64-windows"]; let
  buildSystem = "x86_64-linux";
  pkgs = inputs.nixpkgs.legacyPackages.${buildSystem};
in rec {
  toolchain = with inputs.fenix.packages.${buildSystem};
    combine [
      minimal.rustc
      minimal.cargo
      targets.x86_64-pc-windows-gnu.latest.rust-std
    ];

  craneLib = (inputs.crane.mkLib pkgs).overrideToolchain toolchain;

  inherit (inputs.self.internal.${buildSystem}) src packageVersions GIT_REVISION ACROPOLIS_OFFLINE_MIRROR;

  pkgsCross = pkgs.pkgsCross.mingwW64;

  commonArgs = {
    pname = "acropolis";
    inherit (packageVersions.omnibus) version;
    inherit src;
    strictDeps = true;

    CARGO_BUILD_TARGET = "x86_64-pc-windows-gnu";
    TARGET_CC = "${pkgsCross.stdenv.cc}/bin/${pkgsCross.stdenv.cc.targetPrefix}cc";

    OPENSSL_DIR = "${pkgs.openssl.dev}";
    OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
    OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include/";

    depsBuildBuild = [
      pkgsCross.stdenv.cc
      pkgsCross.windows.pthreads
    ];
  };

  # For better caching:
  cargoArtifacts = craneLib.buildDepsOnly commonArgs;

  # FIXME: currently the Windows build fails because `caryatid_process` does
  # `use tokio::signal::unix`, which is not available on Windows.
  workspace = craneLib.buildPackage (commonArgs
    // {
      inherit cargoArtifacts GIT_REVISION ACROPOLIS_OFFLINE_MIRROR;
      doCheck = false; # can’t run Windows tests on Linux (at least without Wine)
    });

  acropolis-process-omnibus = pkgs.stdenv.mkDerivation {
    inherit (packageVersions.omnibus) pname version;
    buildCommand = ''mkdir -p $out && cp ${workspace}/bin/acropolis_process_omnibus.exe $out/'';
    meta.description = "A kit of micro-service parts, written in Rust, which allows flexible construction of clients, services and APIs for the Cardano ecosystem";
  };

  acropolis-process-replayer = pkgs.stdenv.mkDerivation {
    inherit (packageVersions.replayer) pname version;
    buildCommand = ''mkdir -p $out && cp ${workspace}/bin/acropolis_process_replayer.exe $out/'';
    meta.description = "Acropolis replayer process, allowing to debug any module";
  };
}
