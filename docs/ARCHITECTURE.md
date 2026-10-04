# Architecture -- shed

This document describes the conceptual architecture of shed at the component
and data-flow level. It does not prescribe concrete code. See the source
files for implementation detail and docs/AGENT.md for coding rules.

---

## Purpose Recap

shed is a source-to-source compiler: it reads one .shed file and writes
valid shell script for a chosen target dialect. The input language is a
small, line-oriented DSL; the output is idiomatic shell code meant to be
eval-ed or source-d at shell startup.

Design principle: maximum simplicity at every layer.

  - one input format, many output dialects
  - one binary, no runtime, no installer, no config files
  - one straight pipe: source text -> tokens -> AST -> prune -> emitted text

---
## High-Level Data Flow

    stdin / file
         |
         v
    [ Reader ] --> [ Parser ] --> [ Pruner ] --> [ Emitter ] --> stdout
     raw String      AST          AST             String

Reader  -- trivial file or stdin read. No buffering; .shed files are tiny.
Parser  -- converts raw text into a typed AST. Pure: no I/O, no global state.
Pruner  -- folds shell/OS conditions for the binary target and simplifies
           branches. Pure: no I/O, no global state.
Emitter -- converts the AST into a target-language string. Pure: no I/O.

Four stages. No symbol table, no linker.

---

## Component Map

    src/
      main.rs      CLI: parse args, read source, parse/prune,
                   dispatch to emitter, print result.
      ast.rs       Shared data types: Node, IfNode, Cond.
      parser.rs    Converts text -> Vec.
                   Quote-aware tokenisation, validation, and recursive descent.
      prune.rs     Folds target shell/OS conditions.
      emit.rs      Emitter trait + sub-module declarations.
      emit/
        bash.rs    bash + zsh backend (POSIX-compatible)
        fish.rs    fish backend
        pwsh.rs    PowerShell backend

---
## The AST

The statement AST is deliberately flat and concrete. Conditions use a small
recursive tree so the parser can preserve the documented `not > and > or`
precedence without introducing a generic expression node.

    Node
      Set   { key, val }     -- export an environment variable
      Path  { dir, direction } -- prepend or append to PATH
      Call  { cmd, args }    -- eval-style initialiser
      Alias { name, body }   -- shell alias
      If(IfNode)
        cond  : Cond
        body  : Vec<Node>     -- then-branch
        elifs : Vec<(Cond, Vec<Node>)> -- elif branches
        else_ : Vec<Node>     -- empty = absent

    Cond
      Have(cmd)    -- command must exist on PATH (runtime)
      Exists(path) -- path must exist on filesystem (runtime)
      Env(var)    -- environment variable is set and non-empty (runtime)
      Os(name)     -- darwin | linux | windows (compile-time fold)
      Shell(name)  -- bash | zsh | fish | pwsh (compile-time fold)
      Not(Cond)
      And(Cond, Cond)
      Or(Cond, Cond)

Nesting is supported through IfNode body / elifs / else_.
The recursive block() call handles arbitrary depth naturally.

`Call` and alias bodies deliberately preserve shell syntax for eval-init and
alias use cases. They are trusted-input escape hatches, not data serializers.

---

## The Parser

Two micro-phases:

1. Pre-tokenisation -- split source into lines and run a small quote-aware
   lexer. Whitespace inside quoted tokens is preserved; `#` starts a comment
   only outside quotes. Drop blank lines and reject unterminated quotes.

2. Recursive descent -- block(stops) consumes token-lines until a stop
   keyword or EOF. parse_if handles branching. parse_cond applies the
   `not > and > or` precedence rules before producing a Cond tree.

Error messages carry enough context for the user to self-correct without
reading source code.

---

## The Emitter Trait

    trait Emitter
      name()        -> &str       -- shell name; used for {shell} substitution
      emit_nodes()  -> Vec -- only required implementation
      render()      -> String     -- joins lines with newlines (default)
      indent()      -> String     -- prepends N*2 spaces (default)

Each backend implements emit_nodes as a flat match over Node variants.
Conditional code generation lives in a private emit_if method on the struct.
Shell-specific idioms are encapsulated entirely inside the backend.

No associated types, no generics, no lifetime parameters on the trait.

---
## Extension Points

### New shell dialect

1. Create `src/emit/<shell>.rs`.
2. Declare `pub mod <shell>;` in `src/emit.rs`.
3. Implement `Emitter` for the new struct.
4. Add a match arm in `main.rs`.

No other files need to change.

### New statement keyword

1. Add a variant to `Node` (and `Cond` if it is a condition) in `src/ast.rs`.
2. Add a parse arm in `Parser::block()` (or `parse_cond`) in `src/parser.rs`.
3. Add an emit arm in every emitter's `node()` / `cond()` method.

The compiler enforces exhaustiveness: no variant can be silently skipped.

### New condition type

1. Add a `Cond` variant in `src/ast.rs`.
2. Add a parse arm in `Parser::parse_cond()` in `src/parser.rs`.
3. Add an emit arm in every backend `cond()` method.

---

## Module Layout

```
src/
  main.rs       — CLI entry point only; no business logic
  ast.rs        — data types (Node, IfNode, Cond, ParseError)
  parser.rs     — source → AST
  emit.rs       — Emitter trait + sub-module declarations
  emit/
    bash.rs     — bash / zsh backend
    fish.rs     — fish backend
    pwsh.rs     — PowerShell backend
  prune.rs      — compile-target condition pruning
```

Do not let `main.rs` grow. If logic is needed beyond dispatching to an emitter,
it belongs in a dedicated module.

---

## Compound Conditions (`and`, `or`, `not`) -- Operator Precedence

This section documents the implemented syntax and precedence rules for
compound conditions in the shed DSL.

### Syntax

```sh
# not (prefix) -- negates one leaf; highest precedence
if not have cargo
  set CARGO_ABSENT 1
end

# and (infix) -- medium precedence
if have cargo and os linux
  path+ ~/.cargo/bin
end

# or (infix) -- lowest precedence
if os darwin or os linux
  set POSIX 1
end

# mixed: not binds tighter than and, and tighter than or
if not have cargo and os linux
  # parsed as: (not have cargo) and (os linux)
end

if have cargo or not shell fish
  # parsed as: (have cargo) or (not (shell fish))
end
```

### Precedence table (highest to lowest)

| Level | Operator | Arity | Associates |
|-------|----------|-------|------------|
| 1 | `not` | prefix | right |
| 2 | `and` | infix | left |
| 3 | `or` | infix | left |

There are **no parentheses** in the DSL. Conditions requiring grouping beyond
what precedence provides must use nested `if` blocks.

### Grammar (EBNF)

```
cond     = or_expr
or_expr  = and_expr ( "or"  and_expr )*
and_expr = not_expr ( "and" not_expr )*
not_expr = "not" not_expr | leaf
leaf     = ( "have" | "exists" | "os" | "shell" ) value
```

Because the DSL is line/token-split, each leaf occupies exactly two tokens.
The parser locates `or` / `and` by scanning the flat token slice for those
keywords at positions where a boundary between two leaves can exist (i.e.
positions 2, 5, 8... for a sequence of two-token leaves, but since `not`
consumes a prefix the scan uses a right-to-left search for the lowest-
precedence operator).

### Parser implementation (`src/parser.rs`)

`parse_cond(ln, toks)` is the entry point. It calls three helpers in order
of descending precedence:

```
parse_or(ln, toks)
  scan RIGHT-TO-LEFT for the last "or" where index >= 2 and index+1 < len.
  split at that position:
    left  = parse_or(toks[..pos])    -- recurse left for left-associativity
    right = parse_and(toks[pos+1..]) -- right operand is and-level
  if none found: delegate to parse_and.

parse_and(ln, toks)
  scan RIGHT-TO-LEFT for the last "and" where index >= 2 and index+1 < len.
  split:
    left  = parse_and(toks[..pos])   -- recurse left for left-associativity
    right = parse_not(toks[pos+1..]) -- right operand is not-level
  if none found: delegate to parse_not.

parse_not(ln, toks)
  if toks[0] == "not": Not(Box::new(parse_not(toks[1..])))
  else: parse_leaf(toks)

parse_leaf(ln, toks)
  expects exactly [type, value]; type in {have, exists, os, shell}
```

Right-to-left scanning for the LAST operator, with left-recursive descent,
achieves left-associativity:
  `a and b and c` -- last `and` splits right side off
  → And(parse_and("a and b"), parse_not("c"))
  → And(And(a,b), c)     -- left-associative

### Emitter output

| Shell | `Not(c)` | `And(a, b)` | `Or(a, b)` |
|-------|----------|-------------|------------|
| bash/zsh | `! <c>` | `<a> && <b>` | `<a> \|\| <b>` |
| fish | `not <c>` | `<a>; and <b>` | `<a>; or <b>` |
| pwsh | `(-not (<c>))` | `(<a>) -and (<b>)` | `(<a>) -or (<b>)` |

### What to avoid

- **Parentheses in the DSL** — the parser is line/token-split; a paren grammar
  would need a character scanner.
- **Implicit precedence surprises** -- always document that `not` binds tighter
  than `and`, which binds tighter than `or`. Add a comment in source if unclear.

### Implementation files

`src/parser.rs` — condition parsing and precedence.
`src/ast.rs` — `Cond` variants (`Have`, `Exists`, `Os`, `Shell`, `Not`, `And`, `Or`).
All four emitter backends — each must handle every `Cond` variant.

---

## Semantic Pruning Pass

The pruning pass runs after parsing and before emitting. It is pure: no I/O,
no global state; the input AST, target shell, and optional target OS are the
only inputs.

### Purpose

Conditional blocks guarded by `Cond::Shell` or `Cond::Os` are statically
known to be always-true or always-false for a specific compilation target.
Emitting dead branches wastes output lines and can confuse shell linters.

Examples (target = bash on Linux):

```
if shell fish          →  entire block unreachable; prune to nothing
  ...
end

if shell bash          →  guard always true; inline the body directly
  set EDITOR nvim
end

if os linux            →  always-true on a Linux build; inline body directly
  set BROWSER xdg-open
end

if os darwin           →  always-false on a Linux build; drop block
  set BROWSER open
end

if have cargo          →  runtime check; always kept as-is
  path+ $HOME/.cargo/bin
end

if exists $HOME/.cargo/bin   →  filesystem check; always kept as-is
  path+ $HOME/.cargo/bin
end
```

`Cond::Have` and `Cond::Exists` are runtime checks that may differ per
machine and are never folded.

`Cond::Os` is folded **at compile time** using Rust's `#[cfg(target_os)]`.
For the three known names (`darwin`, `linux`, `windows`) the result is
always-true or always-false depending on the Rust compilation target. Unknown
OS names stay as runtime checks. The CLI can override this default with
`--target-os darwin|linux|windows`, allowing one binary to generate output for
another operating system without losing static pruning.

### Pruning rules for `Cond::Shell(name)`

| Condition | Target shell | Result |
|-----------|-------------|--------|
| `Shell(s)` where `s == target` | any | always-true → inline body |
| `Shell(s)` where `s != target` | any | always-false → drop branch |

### Pruning rules for `Cond::Os(name)`

| Condition | Rust target | Result |
|-----------|-----------|--------|
| `Os("linux")` | linux | always-true → inline body |
| `Os("darwin")` | macos | always-true → inline body |
| `Os("windows")` | windows | always-true → inline body |
| `Os(known)` | different known OS | always-false → drop branch |
| `Os(unknown)` | any | Unknown → runtime check emitted |

### Pruning rules for compound conditions

The pass reduces compound `Cond` nodes before deciding branch fate:

- `Not(always-true)`  → always-false
- `Not(always-false)` → always-true
- `And(always-false, _)` or `And(_, always-false)` → always-false (short-circuit)
- `And(always-true,  c)` → reduce to `c`
- `Or(always-true,  _)` or `Or(_, always-true)`  → always-true (short-circuit)
- `Or(always-false, c)` → reduce to `c`

A `Cond` that cannot be fully resolved stays as a `Cond` node and is emitted
normally.

### Branch outcome

After condition evaluation:

- **Always-true body** — replace `Node::If(inode)` with the body nodes
  inlined into the parent list. `elif` / `else` branches are dropped.
- **Always-false body** — check `elifs` in order; the first elif whose
  condition is also always-true is inlined. If no elif matches, the `else_`
  block (if present) is inlined. If nothing matches the node is dropped.
- **Unknown** — the `IfNode` is kept but its `body`, `elifs`, and `else_`
  are recursively pruned.

### Implementation location

```
src/
  prune.rs    — prune_nodes_for_target(nodes, shell, target_os) -> Vec<Node>
                prune_cond(cond, shell, target_os)              -> CondResult
```

`CondResult` is a local enum:

```rust
enum CondResult {
    AlwaysTrue,
    AlwaysFalse,
    Unknown(Cond),
}
```

The pass is wired into `main.rs` after parsing and before emission:

```
read → parse (including path resolution) → prune_nodes_for_target → emit
```

`prune_nodes_for_target` takes the target shell and optional target OS as
strings, so it remains pure and testable without constructing an `Emitter`.

---

## What This Architecture Deliberately Omits

  Symbol table / var resolution  -- DSL has no user-defined variables
  Type system                    -- all values are strings; no type errors
  General optimisation pass      -- output correctness matters more than brevity
  Plugin / dynamic loading       -- shell set is closed; static dispatch wins
  Runtime configuration file     -- all behaviour driven by the .shed source
  IR between parser and emitter  -- the AST is the IR; no lowering needed

---

## `Cond::Exists` — Filesystem-Presence Check

`if exists <path>` evaluates the path at shell startup time. It solves the
cargo bootstrap problem cleanly: instead of checking whether `cargo` is on
PATH (which it isn't yet), check whether the `.cargo/bin` directory already
exists on disk.

```shed
# chicken-and-egg free:
if exists $HOME/.cargo/bin
  path+ $HOME/.cargo/bin
end
```

Emitter output:

| Shell | `Exists(path)` |
|-------|----------------|
| bash/zsh | `[ -e "path" ]` |
| fish | `test -e "path"` |
| pwsh | `Test-Path "path"` |

Like `Have`, `Exists` is always kept as a runtime check; it is never
statically folded.

---

## `Cond::Env` — Environment-Variable Presence Check

`if env <VAR>` checks whether an environment variable is set and non-empty.
This is a second option for the bootstrap problem: tools like `rustup` set
`CARGO_HOME` on installation, so checking for that variable is an alternative
to checking the directory.

```shed
# alternative using env var:
if env CARGO_HOME
  path+ $CARGO_HOME/bin
end
```

Emitter output:

| Shell | `Env(var)` |
|-------|------------|
| bash/zsh | `[ -n "${VAR:-}" ]` |
| fish | `set -q VAR` |
| pwsh | `(Test-Path env:VAR)` |

Like `Have` and `Exists`, `Env` is always a runtime check; never folded.

---

## PATH Deduplication Guard

`path+` and `path-` emit an unconditional PATH mutation today, which means
re-sourcing the shell config appends the same directory multiple times.

The emitters wrap every `Path` node with a delimiter-aware duplicate guard:

| Shell | Guard |
|-------|-------|
| bash/zsh | `[[ ":$PATH:" != *":dir:"* ]] && export PATH="dir:$PATH"` |
| fish | `fish_add_path` already deduplicates; no change needed |
| pwsh | `if (($env:PATH -split ';') -notcontains "dir") { $env:PATH = "dir;$env:PATH" }` |

---

## Quote Stripping in the Parser

**Implemented.** `strip_quotes(s)` strips a single layer of surrounding
`"…"` or `'…'` from any wholly-wrapped token. Partial quotes and multi-token
values are left alone. Applied via `tail_stripped` / `tok_stripped` helpers
to every value position (`val`, `dir`, `cmd`, `body`, condition value).

Rule: `"foo"` → `foo`, `'bar'` → `bar`, `foo"bar` → `foo"bar` (unchanged).

---

## Shell Variable Injection Guard

**Implemented.** `contains_shell_variable(s)` returns `true` when a path
token starts with `~` or contains `$`. Such tokens are returned from
`resolve_path` as-is (only `\` → `/` normalisation applied).

This prevents `$HOME/go/bin` from being passed through `PathBuf` and
corrupted into `C:\Users\user\$HOME/go/bin` on Windows.

Rule:
- `$HOME/.cargo/bin` → stored as `$HOME/.cargo/bin` (target shell expands)
- `~/bin` → stored as `~/bin` (target shell expands)
- `$env:USERPROFILE/.cargo` → stored unchanged
- `C:\tools` → normalised to `C:/tools`

Applied in `parse_statement` for `path+`/`path-` and in `parse_leaf` for
`exists` conditions.

---

## `alias` Keyword

**Implemented.** `Node::Alias { name, body }` is in `ast.rs`. All four
emitters render it with shell-correct syntax:

| Shell | Output |
|-------|--------|
| bash/zsh | `alias name='body'` |
| fish | `alias name body` |
| pwsh | `Set-Alias -Scope Global -Name name -Value body` |

PowerShell requires `-Scope Global` so the alias survives past the
dot-sourced script frame. `-Name` / `-Value` are always explicit to avoid
positional ambiguity with multi-word bodies.

---

## `format_call` on the Emitter Trait

**Implemented.** The `Emitter` trait provides a default `format_call` method:

```rust
fn format_call(&self, cmd: &str, args: &str, prefix: &str, suffix: &str) -> String
```

It calls `resolve_call_args` for `{shell}` substitution then builds
`prefix + cmd [+ " " + args] + suffix`. All three backends use it, removing
the repeated `if a.is_empty()` branch that previously existed in each emitter.

`PwshEmitter` overrides `resolve_call_args` to substitute `"powershell"` for
`{shell}` (the correct init-command name) while keeping `name()` returning
`"pwsh"` (the DSL identifier used in routing and `Cond::Shell` comparisons).

---

## Structured Error Type

**Implemented.** `ParseError { line: usize, msg: String }` is defined in
`src/ast.rs`. All parser errors carry the 1-based source line number.
Error messages are plain English, lowercase, no trailing period.

```
shed: line 42: unknown keyword "sett"
shed: line 7: usage: set KEY VALUE
```

---
