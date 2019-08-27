{ rev    ? "70c1c856d4c96fb37b6e507db4acb125656f992d"
, sha256 ? "0w155rcknc3cfmliqjaq280d09rx4i0wshcrh9xrsiwpdn90i52d"
, pkgs   ?
  import (builtins.fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/${rev}.tar.gz";
    inherit sha256; }) {
    config.allowUnfree = true;
    config.allowBroken = true;
    config.packageOverrides = pkgs: rec {
      rls = pkgs.rls.overrideDerivation (attrs: {
        buildInputs = attrs.buildInputs ++
          (pkgs.stdenv.lib.optional pkgs.stdenv.isDarwin
             pkgs.darwin.apple_sdk.frameworks.Security);
      });
    };
  }

, mkDerivation ? null
}:

with pkgs;

rustPlatform.buildRustPackage rec {
  pname = "procman";
  version = "0.1.0";

  src = ./.;

  cargoSha256 = "0an61s57ps1dqc16iy0n8dd1bnkzlsaa3jhcy5pqm3w5jhgx5bq6";
  cargoSha256Version = 2;
  cargoBuildFlags = [];

  nativeBuildInputs = [ asciidoc asciidoctor plantuml docbook_xsl libxslt ];
  buildInputs = [ cargo rustfmt rls rustPackages.clippy ]
    ++ (stdenv.lib.optional stdenv.isDarwin darwin.apple_sdk.frameworks.Security);

  preFixup = ''
  '';

  RUSTC_BOOTSTRAP = 1;

  meta = with stdenv.lib; {
    description = "Hello, world!";
    homepage = https://github.com/jwiegley/hello-rust;
    license = with licenses; [ mit ];
    maintainers = [ maintainers.jwiegley ];
    platforms = platforms.all;
  };
}
