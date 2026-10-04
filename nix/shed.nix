{ lib, stdenv, fetchurl }:

let
  version = "0.1.5";

  sources = {
    "x86_64-linux"  = { url = "https://github.com/nostalume/shed/releases/download/v${version}/shed-linux-x86_64";  hash = "sha256-2N901GcwgEHcDLGNkhFGeFHyFeSwWM2JF7Q2ALPSYzk="; }; # x86_64-linux
    "aarch64-linux" = { url = "https://github.com/nostalume/shed/releases/download/v${version}/shed-linux-aarch64"; hash = "sha256-ihRyuFfQikKL+m4GI3UdNSiUoDSHmIcV2UqPFThobRQ="; }; # aarch64-linux
    "x86_64-darwin" = { url = "https://github.com/nostalume/shed/releases/download/v${version}/shed-macos-x86_64";  hash = "sha256-zNhLHkum3Wk3bdOoXmmqVyyHTCwHbBwFzQ7/4aAXkbs="; }; # x86_64-darwin
    "aarch64-darwin"= { url = "https://github.com/nostalume/shed/releases/download/v${version}/shed-macos-aarch64"; hash = "sha256-TMZyLjOLtUYN//yu7XQUIta49puAW40tA4xlUibuV64="; }; # aarch64-darwin
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
