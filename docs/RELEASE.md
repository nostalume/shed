# Release guide

The source checkout and published package catalogs have separate versions until a
new release is available. User installation commands are in the [README](../README.md).

## Checks and release preparation

Use the toolchain in [rust-toolchain.toml](../rust-toolchain.toml):

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all --locked
cargo build --release --locked
python3 -B -m unittest discover -s tests -p '*_test.py' -v
```

Interpreter prerequisites and tested versions are in the
[development plan](../plans/dsl-development.md). The Python packaging tools require
Python 3.9+. PowerShell release preparation requires PowerShell 7.5+.

Start at the repository root, on `main`, with a clean working tree. Run one helper:

```sh
bash scripts/release.sh 0.2.0
```

```powershell
.\scripts\release.ps1 0.2.0
```

These commands update only `Cargo.toml` and `Cargo.lock`, run `cargo check --locked`,
commit, create an annotated `vMAJOR.MINOR.PATCH` tag, and atomically push main and
the tag. They publish Git refs. They stop on native command failure; a `git diff`
error is distinguished from its normal "changes present" result. The full checks
above remain required; tag CI also enforces them before publishing binaries.

Scoop and Nix keep pointing to the preceding published release during preparation.
The helpers do not pair a new Nix version with the old release's hashes. A failed
command can leave modified Cargo files, a commit, or a local tag. Inspect local
and remote state before retrying; do not assume a failed push or API request had
no effect.

## Build once, verify, publish

| Workflow | Trigger | Responsibility |
| --- | --- | --- |
| `ci.yml` | Main/master/codex pushes, main/master PRs, manual, reusable call | Real-shell matrix, Rust minimum, five release builds, catalog tests and a native Nix consumer check. |
| `release.yml` | A `v*` tag | Reuse CI, then publish those exact checked artifacts with a job-scoped write token. |
| `packages-update.yml` | A successful tag-triggered Release run | Verify the source/tag binding and all published assets; update Scoop and Nix in one commit. |

Remote actions are pinned to commits and covered by the existing Dependabot
configuration. CI has read-only repository access; it never creates a release.
The release publisher receives write access only after the checks complete.
Release tags must be stable numeric versions matching the Cargo package version.
The publisher verifies checksums again and uses `gh release create --verify-tag`;
an existing release causes an error rather than silently replacing its assets.

| Rust target | Binary |
| --- | --- |
| `x86_64-unknown-linux-musl` | `shed-linux-x86_64` |
| `aarch64-unknown-linux-musl` | `shed-linux-aarch64` |
| `x86_64-apple-darwin` | `shed-macos-x86_64` |
| `aarch64-apple-darwin` | `shed-macos-aarch64` |
| `x86_64-pc-windows-msvc` | `shed-windows-x86_64.exe` |

Each binary has a matching `.sha256` file containing its digest and exact basename.
CI builds all five variants. Its Nix consumer check installs the just-built Linux
x86_64 binary, checks its version and checks the DSL example. Cross-compilation
does not certify execution on every CPU, and this does not perform a real Scoop
installation or publish a package.

## Scoop and Nix catalogs

[scripts/update-packages.py](../scripts/update-packages.py) owns both catalogs.
It validates the version, downloads all five binaries and sidecars, checks every
digest and basename, then renders both files before writing either. A missing,
corrupt or mismatched asset stops the update. An older delayed release is ignored;
the same release is safe to verify again.

Scoop stores the Windows SHA-256 in [shed.json](../shed.json). Its autoupdate URL
retains `$version`; the checksum URL names the real `.exe.sha256` asset without
the `#/shed.exe` local rename fragment.

[nix/shed.nix](../nix/shed.nix) stores flat-file SHA-256 digests in SRI form for the
four Unix binaries. Computing these digests needs no local Nix installation.
The [flake](../flake.nix) exposes each platform's downloaded binary as `shed` and
the default package. The version and hashes advance together after assets exist.

The updater workflow runs trusted default-branch code, checks the tag's commit
against the successful Release run and default-branch history, and commits both
catalogs together. Its concurrency queue preserves pending updates; a version
check prevents a delayed older event from rolling catalogs back. An unrelated
main commit can still reject a push; rerun after inspecting the remote state.

Manual verification/update, after the release exists:

```sh
python3 scripts/update-packages.py 0.2.0
```

For already downloaded assets:

```sh
python3 scripts/update-packages.py 0.2.0 --assets-dir dist
```

Inspect the two-file diff before committing it. Where Nix is already available:

```sh
nix flake check --no-build --no-write-lock-file
nix build .#shed
nix run . -- --version
```

Flake evaluation alone is not an installation check. These catalog conventions
follow [Scoop's manifest specification](https://github.com/ScoopInstaller/Scoop/wiki/App-Manifests),
[Scoop autoupdate](https://github.com/ScoopInstaller/Scoop/wiki/App-Manifest-Autoupdate),
and [Nixpkgs fetchurl](https://nixos.org/manual/nixpkgs/stable/#fetchurl).

## Publication completion

- [ ] The intended tag and package version identify the same source.
- [ ] CI and all five target builds pass.
- [ ] The release contains all five binaries and their exact checksums.
- [ ] One Scoop/Nix update commit reaches the default branch.
- [ ] Catalog URLs, versions and digests resolve the intended release.
- [ ] Native consumer checks report the expected version.

Preparing or testing this workflow does not itself authorize a tag, release,
catalog push, or other publication. Other package managers and crates.io are not
part of this release path.
