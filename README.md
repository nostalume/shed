# shed

**Sh**ell **E**nvironment **D**eclaration.

A small compiler for shell environment declarations, with no third-party Rust
dependencies. The development checkout implements the
[next DSL](docs/DSL.md): source validation, conservative target specialization,
and generation for PowerShell, Bash, Zsh, and Fish. CI has verified all nine
declared shell/OS pairs across Linux, Darwin, and Windows. See the
[tested versions and scope](plans/dsl-development.md#stage-6-the-declared-matrix-and-public-interface-agree).

Published 0.1.6 binaries use the previous DSL. The examples below belong to the
development checkout; build from source to use this language.

## Check a configuration

The complete example is [examples/env.shed](examples/env.shed):

```text
set EDITOR nvim
path+ "${home}/.cargo/bin"
alias gs = git status --short

if have starship
    init starship init "${shell}"
end
```

From the checkout, check a file or standard input:

```sh
cargo run --quiet -- check examples/env.shed
cargo run --quiet -- check < examples/env.shed
```

The checked-in file reports `ok (6 top-level nodes)`. A check reads no environment
values, launches no commands, and executes no raw code. It validates source even
inside statically dead branches. Errors include line and column positions.

Empty words, adjacent quoted fragments, explicit environment expansions,
parenthesized conditions, and target-tagged raw blocks follow the
[DSL specification](docs/DSL.md). A check does not certify the runtime behavior
of an initializer or embedded shell program.

## Install

### Prebuilt binaries

Download a binary from [GitHub Releases](https://github.com/nostalume/shed/releases).
Published targets are Linux and macOS on x86_64/aarch64, and Windows on x86_64.
Each binary has a companion `.sha256` checksum.

### Build with Cargo

```sh
git clone https://github.com/nostalume/shed.git
cd shed
cargo install --locked --path .
```

The project uses Rust 2024. CI checks the declared Rust 1.85 minimum;
[`rust-toolchain.toml`](rust-toolchain.toml) pins Rust 1.99.1, including Cargo,
Rustfmt, and Clippy, for development and CI. With rustup, entering the checkout
selects the pinned toolchain. Prebuilt binaries do not require Rust.

### Windows: Scoop

```powershell
scoop bucket add shed https://github.com/nostalume/shed
scoop install shed/shed
```

Upgrade with `scoop update shed`. A post-release workflow updates the bucket
manifest stored in this repository.

### macOS and Linux: Nix

```sh
nix profile install github:nostalume/shed#shed
nix run github:nostalume/shed -- --version
```

The flake exposes both `shed` and a default package. For an existing NixOS or
Home Manager flake, add the repository as an input:

```nix
inputs.shed.url = "github:nostalume/shed";
```

Then use the package for your system in the appropriate configuration:

```nix
# NixOS
environment.systemPackages = [ inputs.shed.packages.${pkgs.stdenv.hostPlatform.system}.shed ];

# Home Manager
home.packages = [ inputs.shed.packages.${pkgs.stdenv.hostPlatform.system}.shed ];
```

For a local checkout without flakes, use the binary derivation directly:

```nix
home.packages = [ (pkgs.callPackage ./nix/shed.nix {}) ];
```

The derivation installs a versioned release binary; it does not build the current
Rust source. See [release packaging](docs/RELEASE.md#nix-packaging).

### macOS and Linux: installer

```sh
curl -fsSL https://raw.githubusercontent.com/nostalume/shed/main/install.sh | sh
```

To select a published version:

```sh
curl -fsSL https://raw.githubusercontent.com/nostalume/shed/main/install.sh | sh -s -- v0.1.6
```

The installer verifies the binary checksum and installs to `~/.local/bin`, or
`~/bin` if the preferred directory cannot be created.

## Compilation status

Canonical target names are `bash`, `zsh`, `fish`, and `powershell`. The
[specification](docs/DSL.md#targets) defines the supported target pairs.

The PowerShell backend implements all DSL statements and requires PowerShell
7.5 or later. From a PowerShell session, compile to a file and load it only after
successful compilation:

```powershell
cargo run --quiet --locked -- powershell examples/env.shed > env.generated.ps1
if ($LASTEXITCODE -ne 0) { throw 'shed compilation failed' }
. ./env.generated.ps1
```

Keep compiler output as a file: capturing it as PowerShell lines and joining them
would change newline data inside generated literals and raw bodies. Runtime
errors stop the load with a catchable error; earlier environment changes remain.
Aliases expand their structured words at invocation, preserve native stdout in
the pipeline and `$LASTEXITCODE`, and throw on nonzero native exit.

PowerShell 7.6.9 passed on Windows, Linux and Darwin. Bash, Zsh and Fish passed on
Linux and Darwin. Each Unix backend has separate 12,720-string scalar-codec and
native-argument checks. See the [development stages](plans/dsl-development.md)
for runtime versions, rejected values and the limits of this evidence.

For Bash, compile to a file and source it after successful compilation:

```bash
cargo run --quiet --locked -- bash examples/env.shed > env.generated.sh &&
    source ./env.generated.sh
```

Use `zsh` as the target when loading from Zsh. For Fish:

```fish
cargo run --quiet --locked -- fish examples/env.shed > env.generated.fish
and source ./env.generated.fish
```

Cross-compilation requires an explicit target OS, for example
`shed --target-os darwin zsh examples/env.shed`. Generation uses the selected
target; environment and filesystem queries run when the generated file is loaded.

## Develop

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all --locked
cargo build --release --locked
```

Tests require `pwsh` 7.5+ and the selected Rust compiler on PATH. On Linux/macOS,
the Unix suite also requires real `bash`, `zsh`, and `fish` executables; a missing
shell fails the test. On Windows those three test groups are explicitly ignored.
Tests launch isolated interpreter sessions and compile a disposable native fixture;
they do not load generated code into your development shell.

The Unix adapters use `printenv`, `iconv`, `mktemp`, `cat`, `tr`,
`cmp`, and `rm`, resolved in `/usr/bin`, then `/bin`. Fish rejects PATH values
containing empty entries before mutation because Fish would rewrite them to `.`;
other shells retain exact empty entries. Init uses owned temporary files,
removed on normal success/failure; interruption can leave a `shed.*` capture in
the target's temporary directory.

Bash requires 5.2+; generated files reject older versions before DSL effects.
Fish requires 4.9+, the validated runtime baseline.
On macOS, select a modern Bash explicitly: the system Bash 3.2 cannot reliably
source process-substitution streams. CI acquires modern Bash only on its remote
Darwin runner.

CI runs the complete suite on Linux and Darwin, and PowerShell on Windows.
It records interpreter versions and checks Rust 1.85 separately with
`cargo check --all-targets --locked`. Shells and the minimum Rust toolchain are
acquired only on disposable hosted runners. Workflow configuration is separate
from the observed results recorded in the development plan.

- [DSL specification](docs/DSL.md): approved language semantics and current status.
- [Development stages](plans/dsl-development.md): dependencies and acceptance evidence.
- [Working agreements](docs/AGENTS.md): collaboration principles.
- [Release guide](docs/RELEASE.md): build and distribution procedures.

## License

MIT. See [LICENSE](LICENSE).
