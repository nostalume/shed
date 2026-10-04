# shed

**Sh**ell **E**nvironment **D**eclaration.

Write your environment once. Compile to bash, zsh, fish, or PowerShell.

```sh
shed bash  ~/.config/shed/env.shed   # → bash / POSIX sh
shed zsh   ~/.config/shed/env.shed   # → zsh
shed fish  ~/.config/shed/env.shed   # → fish
shed pwsh  ~/.config/shed/env.shed   # → PowerShell
shed check ~/.config/shed/env.shed   # → parse & validate only
shed --target-os linux bash env.shed # → generate Linux branches explicitly
```

## install

The project uses Rust 2024, requires Rust 1.85 or newer, and pins Rust 1.98.1
for reproducible local development and CI via `rust-toolchain.toml`.

### cargo (any platform)

```sh
git clone https://github.com/nostalume/shed.git
cd shed
cargo install --path .
```

### pre-built binaries

Grab the right binary from the [releases page](https://github.com/nostalume/shed/releases) — no runtime required.

### Windows — Scoop

```powershell
scoop bucket add shed https://github.com/nostalume/shed
scoop install shed/shed
```

The manifest is updated automatically after every release. To upgrade:

```powershell
scoop update shed
```

### macOS & Linux — Nix

**Profile install**

```sh
nix profile install github:nostalume/shed#shed
```

The flake also exposes `shed` as its default package, so
`nix profile install github:nostalume/shed` is equivalent.

**Flake integration**

```nix
# flake.nix
{
  inputs = {
    nixpkgs.url    = "github:NixOS/nixpkgs/nixpkgs-unstable";
    shed-src.url   = "github:nostalume/shed";
    shed-src.flake = false;
  };

  outputs = { self, nixpkgs, shed-src }: let
    forAllSystems = nixpkgs.lib.genAttrs [
      "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin"
    ];
  in {
    packages = forAllSystems (system: {
      shed = nixpkgs.legacyPackages.${system}.callPackage
        "${shed-src}/nix/shed.nix" {};
    });
  };
}
```

Then add it to your NixOS or home-manager config:

```nix
environment.systemPackages = [ inputs.self.packages.${system}.shed ]; # NixOS
home.packages              = [ inputs.self.packages.${system}.shed ]; # home-manager
```

**Without flakes**

```sh
nix-env --file '<nixpkgs>' --install \
  --expr '(import <nixpkgs> {}).callPackage ./nix/shed.nix {}'
```

Run the packaged binary directly with:

```sh
nix run github:nostalume/shed -- --version
```

### macOS & Linux — curl installer

```sh
curl -fsSL https://raw.githubusercontent.com/nostalume/shed/main/install.sh | sh
```

To install a specific version:

```sh
curl -fsSL https://raw.githubusercontent.com/nostalume/shed/main/install.sh | sh -s v0.2.0
```

### cross-compile from source

Uses [`cross`](https://github.com/cross-rs/cross):

```sh
cross build --release --target x86_64-pc-windows-gnu
cross build --release --target aarch64-apple-darwin
```

## releasing

Use the release helper rather than editing the manifest and tag separately:

```sh
bash scripts/release.sh 0.1.7
# Windows PowerShell: .\scripts\release.ps1 0.1.7
```

It updates Cargo's package version, lockfile, and the Nix manifest version,
commits them, creates the matching tag, and pushes both refs atomically. Nix
artifact hashes are refreshed after the release binaries are built.

## shell rc (write once, forget forever)

```sh
# ~/.bashrc or ~/.zshrc
eval "$(shed bash ~/.config/shed/env.shed)"
```

```fish
# ~/.config/fish/config.fish
shed fish ~/.config/shed/env.shed | source
```

```powershell
# $PROFILE
shed pwsh ~/.config/shed/env.shed | Invoke-Expression
```

## dsl

### statements

| Keyword | Syntax             | Effect                                                                       |
| ------- | ------------------ | ---------------------------------------------------------------------------- |
| `set`   | `set KEY value`    | Export an environment variable                                               |
| `path+` | `path+ dir`        | Prepend `dir` to `PATH` (dedup-guarded)                                      |
| `path-` | `path- dir`        | Append `dir` to `PATH` (dedup-guarded)                                       |
| `call`  | `call cmd [args…]` | Run eval-init style initialisers; `{shell}` expands to the target shell name |
| `alias` | `alias name body`  | Define a shell alias                                                         |

`call` and alias bodies are intentional shell syntax and are emitted as trusted
input; do not use them with untrusted configuration files.

### conditions

```sh
if have <cmd>              # command exists on PATH
if exists <path>           # path exists on filesystem at shell startup
if env <VAR>               # env-var is set and non-empty
if os   darwin|linux|windows
if shell bash|zsh|fish|pwsh
elif <cond>
else
end
```

### compound conditions

Conditions can be combined with `not`, `and`, and `or` on a single line.

```sh
if not have cargo                        # negate
if have cargo and os linux               # both must hold
if os darwin or os linux                 # either holds
if not have nvim and shell bash          # not binds tighter than and
if have cargo or os linux and shell bash # and binds tighter than or
```

**Precedence (high → low):** `not` > `and` > `or`  
Use nested `if` blocks when you need explicit grouping.

### compile-time pruning

`if shell <name>` and `if os <name>` branches that cannot match the compile
target are removed from the output entirely. Dead `elif` / `else` chains are
collapsed too. The emitted script contains no unreachable code.

`os` pruning follows the Rust compilation target (`cfg(target_os)`) by default,
not the machine where the generated script is later run. Use `--target-os` when
generating for a different operating system:

```sh
shed --target-os windows pwsh env.shed
```

```sh
# compiled for bash on Linux:
if shell fish
  ...              # entire block dropped
elif shell bash
  set SHELL_OK 1   # inlined directly — no if/fi wrapper emitted
end

if os darwin
  set BROWSER open      # dropped on a Linux build
elif os linux
  set BROWSER xdg-open  # inlined on a Linux build
end
```

`have`, `exists`, and `env` are runtime checks that vary per machine and are
never folded at compile time.

## path resolution

Paths in `path+`, `path-`, and `exists` are processed at parse time:

1. **Shell variables** — tokens containing `$` (e.g. `$HOME`, `$env:USERPROFILE`,
   `$CARGO_HOME`) and the tilde shorthand `~` are **left as-is**. The target shell
   expands them at runtime. `~` is not expanded inside double-quoted strings by
   any shell, so shed never embeds it in a quoted context.
2. **Relative path** — joined onto the directory that contains the `.shed` file,
   so the file is portable regardless of where you run `shed` from.
   When reading from stdin there is no anchor; relative paths are emitted unchanged.
3. **Absolute path** — passed through unchanged.
4. **Delimiter normalisation** — backslashes (`\`) are always converted to forward
   slashes (`/`). PowerShell Core, bash, zsh, and fish all accept `/` as the
   path separator.

No filesystem access is performed; the path does not need to exist at compile time.
Values emitted inside double-quoted shell strings escape delimiters and control
characters while preserving documented shell-variable references.

```sh
# All of the following are valid and passed through correctly:
path+ $HOME/.cargo/bin        # → $HOME/.cargo/bin   (shell expands at runtime)
path+ ~/.local/bin            # → ~/.local/bin        (shell expands at runtime)
path+ $env:USERPROFILE/tools  # → $env:USERPROFILE/tools  (pwsh expands at runtime)
path+ bin                     # → /home/you/dot/bin   (relative → joined to shed file dir)
path+ /usr/local/bin          # → /usr/local/bin      (absolute, unchanged)
```

### aliases

Aliases are emitted with shell-correct syntax:

| Shell    | Output                                           |
| -------- | ------------------------------------------------ |
| bash/zsh | `alias name='body'`                              |
| fish     | `alias name body`                                |
| pwsh     | `Set-Alias -Scope Global -Name name -Value body` |

PowerShell requires `-Scope Global` so the alias survives past the dot-sourced
script frame into the interactive session. `-Name` and `-Value` are always
explicit to avoid positional ambiguity with multi-word bodies.

### path deduplication

`path+` and `path-` wrap every directory mutation in a guard that checks
whether the directory is already in `PATH`. Re-sourcing your shell config
never duplicates entries.

| Shell    | Guard                                                   |
| -------- | ------------------------------------------------------- |
| bash/zsh | `[[ "${PATH}" != *"dir"* ]] && export PATH="dir:$PATH"` |
| fish     | `fish_add_path` deduplicates automatically              |
| pwsh     | `if (($env:PATH -split ';') -notcontains "dir") { ... }` |

### comments

```sh
# full-line comment
set KEY value  # inline comment
```

Comments begin with `#` outside quoted values. Quoted values may contain
spaces and `#`, for example `set GREETING "hello world # users"`.

## example

```sh
set EDITOR nvim

# shell aliases
alias ll ls -la
alias g git

# per-OS browser
if os darwin
  set BROWSER open
elif os linux
  set BROWSER xdg-open
end

# cargo bootstrap
if exists $HOME/.cargo/bin
  path+ $HOME/.cargo/bin
  set CARGO_HOME $HOME/.cargo
  set RUSTUP_HOME $HOME/.rustup
end

# alternative: check for the env var set by rustup
if env CARGO_HOME
  path+ $CARGO_HOME/bin
end

# compound: zoxide only on Linux when it is installed
if have zoxide and os linux
  call zoxide init {shell}
end

# starship for every shell
if have starship
  call starship init {shell}
  set STARSHIP_CONFIG ~/.config/starship.toml
end
```

<details>
<summary>Compiles to bash</summary>

```sh
export EDITOR="nvim"
alias ll='ls -la'
alias g='git'
if [ "$(uname -s)" = "Darwin" ]; then
  export BROWSER="open"
elif [ "$(uname -s)" = "Linux" ]; then
  export BROWSER="xdg-open"
fi
if [ -e "$HOME/.cargo/bin" ]; then
  [[ "${PATH}" != *"$HOME/.cargo/bin"* ]] && export PATH="$HOME/.cargo/bin:$PATH"
  export CARGO_HOME="$HOME/.cargo"
  export RUSTUP_HOME="$HOME/.rustup"
fi
if [ -n "${CARGO_HOME:-}" ]; then
  [[ "${PATH}" != *"$CARGO_HOME/bin"* ]] && export PATH="$CARGO_HOME/bin:$PATH"
fi
if command -v zoxide >/dev/null 2>&1 && [ "$(uname -s)" = "Linux" ]; then
  eval "$(zoxide init bash)"
fi
if command -v starship >/dev/null 2>&1; then
  eval "$(starship init bash)"
  export STARSHIP_CONFIG="~/.config/starship.toml"
fi
```

</details>

<details>
<summary>Compiles to fish</summary>

```fish
set -gx EDITOR "nvim"
alias ll ls -la
alias g git
if test (uname -s) = "Darwin"
  set -gx BROWSER "open"
else if test (uname -s) = "Linux"
  set -gx BROWSER "xdg-open"
end
if test -e "$HOME/.cargo/bin"
  fish_add_path -gP "$HOME/.cargo/bin"
  set -gx CARGO_HOME "$HOME/.cargo"
  set -gx RUSTUP_HOME "$HOME/.rustup"
end
if set -q CARGO_HOME
  fish_add_path -gP "$CARGO_HOME/bin"
end
if type -q zoxide;  and test (uname -s) = "Linux"
  zoxide init fish | source
end
if type -q starship
  starship init fish | source
  set -gx STARSHIP_CONFIG "~/.config/starship.toml"
end
```

</details>

<details>
<summary>Compiles to PowerShell</summary>

```powershell
$env:EDITOR = "nvim"
Set-Alias -Scope Global -Name ll -Value ls -la
Set-Alias -Scope Global -Name g -Value git
if ($IsMacOS) {
  $env:BROWSER = "open"
} elseif ($IsLinux) {
  $env:BROWSER = "xdg-open"
}
if (Test-Path "$HOME/.cargo/bin") {
  if (($env:PATH -split ';') -notcontains "$HOME/.cargo/bin") { $env:PATH = "$HOME/.cargo/bin;$env:PATH" }
  $env:CARGO_HOME = "$HOME/.cargo"
  $env:RUSTUP_HOME = "$HOME/.rustup"
}
if ((Test-Path env:CARGO_HOME)) {
  if (($env:PATH -split ';') -notcontains "$CARGO_HOME/bin") { $env:PATH = "$CARGO_HOME/bin;$env:PATH" }
}
if ((Get-Command zoxide -ErrorAction SilentlyContinue) -and ($IsLinux)) {
  Invoke-Expression (& zoxide init powershell)
}
if (Get-Command starship -ErrorAction SilentlyContinue) {
  Invoke-Expression (& starship init powershell)
  $env:STARSHIP_CONFIG = "~/.config/starship.toml"
}
```

</details>

## chezmoi

chezmoi manages exactly two things — `env.shed` and the `shed` binary:

```
dot_config/shed/env.shed    # your one source of truth
```

No templates needed. No loaders. No per-shell files.

## contributing

**Adding a new shell backend**

1. Create `src/emit/<shell>.rs` and implement the `Emitter` trait.
2. Declare `pub mod <shell>;` in `src/emit.rs`.
3. Add a match arm in `src/main.rs`.

**Adding a new statement keyword**

1. Add a variant to `Node` in `src/ast.rs`.
2. Add a parse arm in `Parser::parse_statement()` in `src/parser.rs`.
3. Add an emit arm in every backend — the compiler enforces exhaustiveness.

**Adding a new condition type**

1. Add a `Cond` variant in `src/ast.rs`.
2. Add a match arm in `Parser::parse_leaf()` in `src/parser.rs`.
3. Add an emit arm in every backend's `cond()` method.

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the full design and
[`docs/AGENT.md`](docs/AGENT.md) for coding conventions.

## license

MIT — see [`LICENSE`](LICENSE).
