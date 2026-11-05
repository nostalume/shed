{ lib, stdenv, fetchurl }:

let
  version = "0.2.0";

  sources = {
    "x86_64-linux"  = { url = "https://github.com/nostalume/shed/releases/download/v${version}/shed-linux-x86_64";  hash = "sha256-5Fctt2gIZJUB2D4M7lhhUDnR7Fj5MMiPmvQ9grSXLzc="; }; # x86_64-linux
    "aarch64-linux" = { url = "https://github.com/nostalume/shed/releases/download/v${version}/shed-linux-aarch64"; hash = "sha256-nW6EsFyjgIboU9AiC9g1F0NYQvQzNBa61u1bv6lakKM="; }; # aarch64-linux
    "x86_64-darwin" = { url = "https://github.com/nostalume/shed/releases/download/v${version}/shed-macos-x86_64";  hash = "sha256-viWrl2+b8tIzR+NCs0JPskmD1EoKiRcVtdxxCSpDZeU="; }; # x86_64-darwin
    "aarch64-darwin"= { url = "https://github.com/nostalume/shed/releases/download/v${version}/shed-macos-aarch64"; hash = "sha256-evLHMyC4m58f0oo2P9gY+XqVkOR/3WrYMDFWt/Z0+TI="; }; # aarch64-darwin
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
    description = "Shell Environment Declaration — compile declarations to Bash, Zsh, Fish, or PowerShell";
    homepage    = "https://github.com/nostalume/shed";
    license     = licenses.mit;
    maintainers = [ ];
    platforms   = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
    mainProgram = "shed";
  };
}
