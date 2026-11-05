# Release guide

This guide describes the repository's current build and distribution paths.
User installation commands are in the [README](../README.md).

## Local checks

Use the toolchain selected by [`rust-toolchain.toml`](../rust-toolchain.toml).
Cargo is included with that toolchain.

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all --locked
cargo build --release --locked
```

All checks must succeed before releasing. The declared minimum Rust version in
`Cargo.toml` is separate from the development pin.

## Create a release

Start on `main` with a clean working tree. The Bash helper requires Python 3.
After local checks, run one helper:

```sh
bash scripts/release.sh 0.1.7
```

Or in PowerShell:

```powershell
.\scripts\release.ps1 0.1.7
```

The helpers update `Cargo.toml`, `Cargo.lock`, and the version in `nix/shed.nix`;
run `cargo check --locked`; commit the version change; create an annotated
`vX.Y.Z` tag; and request an atomic push of the branch and tag. They do not run
the full local checks above.

This command publishes Git refs and triggers the release workflow. Check the
command's result and the remote branch/tag before retrying after failure; a
failed push can leave a local commit or tag. Nix hashes are refreshed later,
after the new binaries exist.

## GitHub Actions

| Configuration | Trigger | Responsibility |
| --- | --- | --- |
| `ci.yml` | Push to `main`, `master`, or `codex/**`; PR to `main`/`master`; manual | Full shell suite on Linux/Darwin, PowerShell on Windows, formatting/Clippy, CLI/examples, release builds and a separate Rust 1.85 compile check. |
| `release.yml` | Push of a `v*` tag | Build five target binaries and publish a GitHub Release with checksums. |
| `scoop-update.yml` | Successful Release workflow | Download the Windows binary and commit an updated `shed.json`. |
| `nix-update.yml` | Successful Release workflow | Refresh binary hashes in `nix/shed.nix`, evaluate the flake, and commit the result. |

[`dependabot.yml`](../.github/dependabot.yml) is dependency-update configuration,
not a workflow. It checks Cargo monthly and GitHub Actions weekly.

CI and release builds pin the same toolchain as local development. CI requires
PowerShell 7.5+ for real generated-script, scalar-codec, and native-argument tests;
it fails when the interpreter is absent rather than silently skipping them.
The checkout configures Ubuntu, Windows and Darwin jobs; local execution has verified
Windows PowerShell and Linux Bash, not those remote job results. Ubuntu installs Zsh/Fish and runs
the required Unix suite; it fails on missing shells. These configured jobs have
not run for the Stage 6 revision yet. Zsh/Fish have official-documentation review;
execution and minimum-Rust results are recorded in [Stage 6](../plans/dsl-development.md).

The two post-release update workflows push independently. Verify that both
commits reach `main`; simultaneous updates can require resolving a failed push.

## Release artifacts

| Rust target | Runner | Binary |
| --- | --- | --- |
| `x86_64-unknown-linux-musl` | Ubuntu | `shed-linux-x86_64` |
| `aarch64-unknown-linux-musl` | Ubuntu | `shed-linux-aarch64` |
| `x86_64-apple-darwin` | macOS | `shed-macos-x86_64` |
| `aarch64-apple-darwin` | macOS | `shed-macos-aarch64` |
| `x86_64-pc-windows-msvc` | Windows | `shed-windows-x86_64.exe` |

The workflow uses musl for Linux and installs the required linkers. Every binary
has a companion `.sha256` file. The installer verifies that checksum before
placing the binary in the user's install directory.

Scoop is updated by its post-release workflow. The current manifest's
`autoupdate.url` contains a fixed version rather than a version placeholder;
do not rely on that template alone to advance the download URL.

## Nix packaging

[`nix/shed.nix`](../nix/shed.nix) packages downloaded release binaries with
per-platform SRI hashes. [`flake.nix`](../flake.nix) exposes that derivation for
Linux and Darwin on x86_64/aarch64 as `shed` and the default package.

The version and binary hashes must describe the same release. The release
helper changes the version first; the post-release workflow later refreshes
hashes. A checkout between those operations may not be installable with Nix.

Manual hash refresh requires Nix, curl, sed, and a Bash with associative-array
support:

```sh
bash nix/update.sh 0.1.6
nix flake check --no-build --no-write-lock-file
nix build .#shed
```

The first command rewrites the derivation. Flake evaluation alone does not build
or run the package. Confirm the installed binary as well:

```sh
nix run . -- --version
```

## Completion checklist

- [ ] Local checks pass.
- [ ] Remote package version and tag agree.
- [ ] All five binaries and checksums are attached to the GitHub Release.
- [ ] Scoop and Nix update commits reach `main`.
- [ ] Nix version and all hashes match the release.
- [ ] The installer and packaged binary report the expected version.

Other package managers and crates.io publication are not part of the current
automated release path.
