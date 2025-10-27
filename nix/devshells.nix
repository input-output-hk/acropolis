{inputs}: {
  config,
  pkgs,
  ...
}: let
  inherit (pkgs) lib;
  internal = inputs.self.internal.${pkgs.system};
in {
  name = "acropolis-devshell";

  imports = [
    "${inputs.devshell}/extra/language/c.nix"
    "${inputs.devshell}/extra/language/rust.nix"
  ];

  commands = [
    {package = inputs.self.formatter.${pkgs.system};}
    {
      name = "cargo";
      package = internal.rustPackages.cargo;
    }
    {package = pkgs.cargo-nextest;}
    {package = internal.rustPackages.rust-analyzer;}
  ];

  devshell.packages =
    [
      pkgs.gnum4
      pkgs.gnumake
      pkgs.unixtools.xxd
      internal.rustPackages.clippy
    ]
    ++ lib.optionals pkgs.stdenv.isLinux [
      pkgs.pkg-config
    ]
    ++ lib.optionals pkgs.stdenv.isDarwin [
      pkgs.libiconv
    ];

  language.c = {
    compiler =
      if pkgs.stdenv.isLinux
      then pkgs.gcc
      else pkgs.clang;
    includes = internal.commonArgs.buildInputs;
  };

  language.rust = {
    packageSet = internal.rustPackages;
    tools = ["cargo" "rustfmt"]; # The rest is provided above.
    enableDefaultToolchain = true;
  };

  env =
    lib.optionals pkgs.stdenv.isDarwin [
      {
        name = "LIBCLANG_PATH";
        value = internal.commonArgs.LIBCLANG_PATH;
      }
    ]
    ++ lib.optionals pkgs.stdenv.isLinux [
      # Embed `openssl` in `RPATH`:
      {
        name = "RUSTFLAGS";
        eval = ''"-C link-arg=-Wl,-rpath,$(pkg-config --variable=libdir openssl)"'';
      }
    ];

  devshell.motd = ''

    {202}🔨 Welcome to ${config.name}{reset}
    $(menu)

    You can now run ‘{bold}cargo run{reset}’.
  '';
}
