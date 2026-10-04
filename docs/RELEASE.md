# Release & Publish Workflow

This document covers how to test, build, and distribute `shed` binaries across
platforms. Everything here runs with tools that are standard in the Rust and
open-source ecosystem.

---

## 1. Local quality gate (run before every release)

```sh
cargo fmt --all -- --check       # formatting
cargo clippy --all-targets --locked -- -D warnings
cargo test --all --locked        # all unit + integration tests
```

All three must pass with exit code 0. The CI workflow (`.github/workflows/ci.yml`)
runs the same three commands on every push and pull request to `main`.

---

## 2. Synchronous versioned release

Do not edit `Cargo.toml`, commit, and tag separately. The release helpers update
`Cargo.toml`, `Cargo.lock`, and the version field in `nix/shed.nix`, commit the
version bump, create the matching annotated tag, and push both refs atomically:

```sh
bash scripts/release.sh 0.1.7
# Windows PowerShell:
.\scripts\release.ps1 0.1.7
```

The single version argument is applied to Cargo and reused for the Git tag;
there is no second version to reconcile. Pushing the tag triggers the GitHub
Actions release workflow. The Nix hashes are refreshed afterward by
`nix-update.yml`, once the release binaries exist.

---

## 3. GitHub Actions workflows

Four workflows live in `.github/workflows/`:

### `ci.yml` — Lint & Test

Runs on every push to `main` and every pull request. Steps:

1. `cargo fmt --all -- --check`
2. `cargo clippy --all-targets --locked -- -D warnings`
3. `cargo test --all --locked`
4. Release-helper and generated Bash syntax validation (plus Fish/PowerShell
   when installed)

Uses `dtolnay/rust-toolchain@1.98.1` and caches the cargo registry and build
artefacts via `actions/cache@v6`.

### `release.yml` — Cross-platform binary builds

Triggered by a push to any `v*` tag created by the release helper. It builds a
static binary for each matrix target and publishes a GitHub Release with all
binaries attached.

| Target | Runner | Binary |
|--------|--------|--------|
| `x86_64-unknown-linux-musl` | ubuntu-latest | `shed-linux-x86_64` |
| `aarch64-unknown-linux-musl` | ubuntu-latest | `shed-linux-aarch64` |
| `x86_64-apple-darwin` | macos-latest | `shed-macos-x86_64` |
| `aarch64-apple-darwin` | macos-latest | `shed-macos-aarch64` |
| `x86_64-pc-windows-msvc` | windows-latest | `shed-windows-x86_64.exe` |

Linux targets use musl for fully static binaries (no glibc dependency).
The aarch64 Linux target requires `gcc-aarch64-linux-gnu` for the linker.

Each build also publishes a `${artifact}.sha256` checksum beside its binary.
The `release` job runs after all `build` jobs complete, downloads all artefacts,
and creates the GitHub Release via `softprops/action-gh-release@v2` with
`generate_release_notes: true`.

### `scoop-update.yml` — Scoop manifest update

Runs automatically after `release.yml` completes successfully. It:

1. Downloads the Windows binary from the new release.
2. Computes its SHA-256 hash.
3. Patches `shed.json` (version, URL, hash) in-place with `sed`.
4. Commits and pushes the updated manifest back to `main`.

This keeps `shed.json` in the repo always in sync with the latest release
without manual intervention.

### `dependabot.yml`

Dependabot is configured to check for GitHub Actions version updates weekly
and open PRs with a `ci:` commit prefix.

### `nix-update.yml` — Nix source refresh

This scheduled workflow refreshes the pinned release version and hashes in
`nix/shed.nix` after a new GitHub release.

---

## 4. Windows — Scoop

[Scoop](https://scoop.sh) installs CLI tools from JSON manifests without
administrator rights. The manifest `shed.json` lives in the repo root and is
updated automatically by the `scoop-update.yml` workflow after each release.

### User installation

```powershell
scoop bucket add shed https://github.com/nostalume/shed
scoop install shed/shed
```

The manifest uses `checkver` pointing at the GitHub releases API and
`autoupdate` to derive the URL and hash for future versions automatically.

---

## 5. macOS & Linux — curl installer

`install.sh` in the repo root detects the OS and architecture, downloads the
correct binary plus its SHA-256 checksum, verifies the download, and places it
in `~/.local/bin` (or `~/bin` as a fallback).

```sh
curl -fsSL https://raw.githubusercontent.com/nostalume/shed/main/install.sh | sh
```

To install a specific version:

```sh
curl -fsSL https://raw.githubusercontent.com/nostalume/shed/main/install.sh | sh -s v0.2.0
```

---

## 6. Nix — binary derivation and flake

The zero-dependency release binaries are packaged by the canonical
[`nix/shed.nix`](../nix/shed.nix) derivation. The repository
[`flake.nix`](../flake.nix) exposes it as both `shed` and the default package:

```sh
nix profile install github:nostalume/shed#shed
nix run github:nostalume/shed -- --version
```

The derivation supports Linux and Darwin on x86_64 and aarch64. Nix verifies
each downloaded release binary with its SRI hash; `nix/update.sh` refreshes the
version and hashes after each GitHub release.

For a non-flake installation, call the canonical derivation directly:

```nix
# home.nix
home.packages = [ (pkgs.callPackage ./nix/shed.nix {}) ];
```

---

## 7. Other package managers (future)

| Manager | Path |
|---------|------|
| **Homebrew** | Submit a formula to `homebrew-core` or maintain a tap at `nostalume/homebrew-shed` |
| **AUR** (Arch Linux) | Publish a `PKGBUILD` that downloads the musl binary or builds via `cargo` |
| **nixpkgs** | Submit the derivation above to `nixpkgs` for inclusion in the official channel |
| **Debian / Ubuntu .deb** | Use `cargo deb` to produce a `.deb` in CI and attach to the release |

---

## 8. crates.io publish (optional)

If the parser or AST are ever exposed as a library:

```sh
cargo publish --dry-run   # verify the package before upload
cargo publish             # upload to crates.io
```

Requires a `CARGO_REGISTRY_TOKEN` secret in GitHub Actions.

---

## 9. Release checklist

```
[ ] cargo fmt --all -- --check passes
[ ] cargo clippy --all-targets --locked -- -D warnings passes
[ ] cargo test --all --locked passes
[ ] `scripts/release.sh` or `scripts/release.ps1` updates Cargo + lockfile
[ ] The release helper creates and atomically pushes `vX.Y.Z`
[ ] CHANGELOG updated (optional)
[ ] GitHub Actions release workflow completes (all 5 binaries attached)
[ ] scoop-update.yml auto-commits updated shed.json
[ ] Nix derivation version + sha256 hashes updated in nix/shed.nix
[ ] install.sh tested with new version
```
