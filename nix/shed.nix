{ lib, stdenv, fetchurl }:

let
  version = "0.2.1";

  sources = {
    "x86_64-linux"  = { url = "https://github.com/nostalume/shed/releases/download/v${version}/shed-linux-x86_64";  hash = "sha256-/o9YDC1NchXltfMmS3zlBmP1u/s4ytPJvuq/+Cl8fE8="; }; # x86_64-linux
    "aarch64-linux" = { url = "https://github.com/nostalume/shed/releases/download/v${version}/shed-linux-aarch64"; hash = "sha256-Vl8EtT46Z5C1qWFr+0F6kdUj0Dfw7YAWSffBZAOMDmc="; }; # aarch64-linux
    "x86_64-darwin" = { url = "https://github.com/nostalume/shed/releases/download/v${version}/shed-macos-x86_64";  hash = "sha256-wPKc3pvO457NYfIJ+lwCprybFbq3Z1sOe6H4m9PMFn8="; }; # x86_64-darwin
    "aarch64-darwin"= { url = "https://github.com/nostalume/shed/releases/download/v${version}/shed-macos-aarch64"; hash = "sha256-/2zhiE3nt20XlaAhGRRJeVP3bkeOUchZa8w92sCUxEA="; }; # aarch64-darwin
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
