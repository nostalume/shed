{ lib, stdenv, fetchurl }:

let
  version = "0.1.6";

  sources = {
    "x86_64-linux"  = { url = "https://github.com/nostalume/shed/releases/download/v${version}/shed-linux-x86_64";  hash = "sha256-s6CDyk6NMYWhZXL1zT7vccfSXQIXHjGxd8dUUSfn1+o="; }; # x86_64-linux
    "aarch64-linux" = { url = "https://github.com/nostalume/shed/releases/download/v${version}/shed-linux-aarch64"; hash = "sha256-hoKwYX+2Z1Qod98FGs5DPoziy9JZHhTFQy/6QTWt/1Y="; }; # aarch64-linux
    "x86_64-darwin" = { url = "https://github.com/nostalume/shed/releases/download/v${version}/shed-macos-x86_64";  hash = "sha256-8O6nPkK3GolBGaYF/QsJSYilP3B4cIxygr9jhLzDVaM="; }; # x86_64-darwin
    "aarch64-darwin"= { url = "https://github.com/nostalume/shed/releases/download/v${version}/shed-macos-aarch64"; hash = "sha256-q9p4p1LVRSokX8ZjAS8GKlZZCa7+0UagmQ1+KfuOD2U="; }; # aarch64-darwin
  };

  src = sources.${stdenv.hostPlatform.system}
    or (throw "shed: unsupported platform ${stdenv.hostPlatform.system}");
in
stdenv.mkDerivation {
  pname   = "shed";
  inherit version;

  src = fetchurl {
    inherit (src) url hash;
  };

  # Linux musl binaries are fully static — no interpreter patching needed.
  # Darwin binaries link only against system frameworks already present.
  dontUnpack = true;
  dontBuild  = true;

  installPhase = ''
    install -Dm755 "$src" "$out/bin/shed"
  '';

  meta = with lib; {
    description = "Shell Environment Declaration — compile env.shed to bash, zsh, fish, or pwsh";
    homepage    = "https://github.com/nostalume/shed";
    license     = licenses.mit;
    maintainers = [ ];
    platforms   = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
    mainProgram = "shed";
  };
}
