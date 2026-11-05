# Next DSL development plan

Status: Stages 1–6 implemented and verified on 2026-11-05 under the runtime
baselines and Fish representability boundary recorded below.
Canonical contract: [DSL specification](../docs/DSL.md).
Baseline: HEAD `424ff73`, package 0.1.6, inspected 2026-11-04, with existing
uncommitted toolchain/documentation changes. This is the user-requested,
repository-visible development plan, separate from the language contract.
It is the single owner of implementation progress for this goal.

This plan authorizes no implementation, host installation, publication, tag,
push, or release by itself. Stage completion is not a release or approval gate.

## Goal and settled decisions

Implement the approved DSL as one compiler pipeline: source admission and parsing,
typed semantic values, target specialization, and target emission. Preserve the
specification's distinctions instead of compensating for lost information.

The user approved a breaking language replacement. Do not build a legacy parser,
compatibility mode, dialect auto-detection, or permanent parallel compiler.
The semantic contract is settled; target-specific private implementation remains
replaceable when it meets that contract.

Carry these requirements through every stage:

- Words contain literal/expansion parts; argument lists remain lists.
- Raw code retains its exact body and separate code-unit boundary.
- Runtime conditions preserve failures and reached-query order.
- OS/shell facts come only from the selected target.
- Environment, path, and executable operations belong to the runtime target.
- Errors never produce partial generated output or execute partial initializer
  stdout after process failure. Runtime changes are not transactional.
- No stage may claim support for a backend, statement, or target pair it has not
  implemented and checked.

Keep the existing small-project structure unless actual responsibilities require
a change. No extra IR, plugin framework, workspace split, persistent runtime
service, or independent proof framework is selected by this plan.

## Baseline evidence and implementation delta

| Baseline owner | Relevant observed behavior at inspection | Needed change |
| --- | --- | --- |
| `src/ast.rs` | Owned string values/tails; recursive Boolean conditions. | Semantic word parts, argument vectors, approved statements, constants, and opaque raw payloads. |
| `src/parser.rs` | Token-start quote recognition, tail joining, operator scans, source-directory anchoring. | Complete word/line grammar, context-directed operand consumption, raw boundaries, and no compile-host path resolution. |
| `src/prune.rs` | Right-absorbing Boolean folds and incomplete later-elif simplification. | Conservative residual evaluation and complete ordered branch normalization. |
| `src/emit.rs` | Shared double-quote escaping and textual placeholder replacement. | Value-preserving target encoding without re-parsing or implicit shell expansion. |
| `src/emit/pwsh.rs` | Windows path assumptions, Set-Alias command strings, current call wrapping. | PowerShell implementation of structured state, calls, failure, and code-unit execution. |
| `src/emit/bash.rs`, `src/emit/fish.rs` | Existing backend dispatch and string generation. | The same new semantic operations with explicit grouping and target behavior. |
| `src/main.rs` | `pwsh` routing, current help, base-directory lookup, parse/prune/emit orchestration. | Canonical targets and an honest frontend/compilation boundary. |
| `src/tests.rs`, inline tests | 166 current tests; many assert old emitted text. | Meaningful new-contract cases; remove assertions that certify obsolete/wrong behavior with their owning change. |
| `.github/workflows/ci.yml` | Ubuntu-only Rust checks; optional shell checks; PowerShell parse errors not enforced. | Required conformance checks for the declared shell/OS matrix. |

Rust/Cargo 1.99.1 is already selected locally, in CI, and for release builds.
Rustfmt, Clippy, all 166 tests, and a release build passed in the preceding
toolchain work. These establish the old implementation baseline, not next-DSL
conformance. The declared Rust 1.85 minimum was not independently verified.

Stage 1 has replaced the frontend, added `src/lexer.rs` and native CLI tests in
`tests/cli.rs`, and removed the obsolete `src/prune.rs`, `src/emit.rs`, and old
`src/emit/` backends. They are not retained as a parallel compiler. Later-stage
module paths below identify proposed implementation locations, not existing
modules or a mandatory physical layout. `docs/AGENT.md` was renamed to
`docs/AGENTS.md` and its references updated.

The earlier finite model covered error-aware specialization, ordered branch
chains, DSL scalar quoting, raw LF/CRLF boundaries, and a PowerShell scalar codec.
It did not certify the Rust parser, native argument passing, session scope,
other shell adapters, or arbitrary embedded code. Translate relevant examples
and model properties into project-owned checks with the changes they validate;
do not require the earlier workstation's files or runtime to execute this plan.

## Dependency order

```text
1. Frontend and check
        |
2. Specialization
        |
3. Executable PowerShell core
        |
4. PowerShell structured commands and code units
        |
5. Bash, Zsh, and Fish conformance
        |
6. Matrix verification and user-interface cutover
```

Stages are coherent development outcomes, not a requirement for six commits or
six PRs. Review and rollback follow actual changes and dependencies.

## Stage 1: The frontend admits the new language

Status: implemented in the working tree on 2026-11-04. No release or commit made.

Primary owners: `src/ast.rs`, `src/parser.rs`, `src/main.rs`, and their checks.
Adapt affected emitter interfaces/matches and CI smoke inputs in this same change
where the representation replacement invalidates them; these are dependencies
of a buildable frontend, not completed backend semantics.
The first executable task is to replace lossy word/tail representation and wire
the complete next-language parser into the frontend check operation.

- [x] Represent semantic word parts, argument vectors, Boolean constants/tree,
  branch bodies, and target-tagged raw payloads; preserve useful source locations.
- [x] Implement adjacent fragments, boundary comments, exact escapes, expansion
  admission, empty words, and complete-line consumption.
- [x] Parse precedence and parentheses by consuming operands before operators.
  Add all approved statements, including unset, init, structured alias, and raw.
- [x] Scan raw markers without interpreting or normalizing payload contents.
  Validate ordinary source even inside statically dead branches.
- [x] Resolve and validate canonical target names/pairs. Keep CLI/input conventions
  focused on that change; check remains a frontend operation, not a shell executor.
- [x] Remove source-directory path anchoring and obsolete implicit-expansion
  construction. No runtime environment/filesystem query occurs in the frontend.

Completion evidence: new syntax parses into inspectable semantic values; examples
with `--config="hello # users"`, literal backslashes, empty/missing-env arguments,
operator-word operands, nested/empty branches, and LF/CRLF raw bodies are covered.
Malformed escapes, extra delimiter tokens, unknown names/expansions, and missing
raw markers fail with source locations. Failed compilation writes no script.

During the staged replacement, compilation routes that cannot consume the new
representation must return an explicit not-implemented diagnostic. Keep a
buildable checked-in state, but do not preserve the old compiler as a second
semantic authority or translate new values back into old string tails.
Retire or revise directly invalidated old-language tests and emitter arms with
their replacement. Change obsolete CLI smoke inputs to frontend checks while
emission is unavailable, explicitly recording that runtime conformance is pending.
Restore required runtime checks with each executable backend; do not advertise
their temporary absence as successful shell validation.

Observed completion: Rustfmt, Clippy with warnings denied, 10 frontend tests,
5 native CLI tests, and a release build passed on Windows with Rust/Cargo 1.99.1.
The scalar test exhaustively checks 14,425 strings over a 24-character alphabet
up to length 3. CLI tests cover file/stdin, strict UTF-8, no evaluation, empty
stdout on failure, canonical targets, and the committed example.

Depth probes exposed actual main-stack overflow in recursive block parsing.
Blocks now use an explicit frame stack; bounded condition parsing and AST depth
return source-located capacity errors instead of crashing. Both the accepted
capacity boundary and oversized if/not/parenthesis/and inputs are tested.
`cargo run -- check examples/env.shed` reports `ok (6 top-level nodes)`.
Generated-shell execution remains unimplemented and is not certified by these
checks. CI now advertises frontend coverage only, alongside release-helper syntax.

## Stage 2: Specialization preserves evaluation obligations

Status: implemented in the working tree on 2026-11-04. No release or commit made.
Primary owner: `src/prune.rs` and condition/branch checks.

- [x] Specialize only selected target facts and Boolean constants.
- [x] Implement exactly the approved constant/neutral rules and double negation.
  Keep residual `x and false` and `x or true`.
- [x] Normalize the whole ordered branch chain; a known-true later branch becomes
  the fallback after preceding residual conditions.
- [x] Recursively specialize bodies, including target-mismatched raw no-ops.
  Retain residual condition evaluation even when all bodies are empty.
- [x] Replace old tests that expect right-absorbing folds or incomplete branch
  simplification. Keep evaluation/reference machinery test-scoped.

Completion evidence: a small independent evaluator compares value/failure and
runtime query order before/after specialization over bounded generated trees and
branch chains. Include injected failures, not only truth tables. Check idempotence,
removal of all static target leaves, first-match behavior, and empty-body cases.
An explicit `error and false` counterexample must defeat the old folding rule.

Observed completion: the new `src/prune.rs` consumes the AST, reuses condition
boxes, and specializes only immutable targets/Boolean constants. It normalizes
ordered branch chains, inlines unconditional bodies, removes unselected raw
statements, and leaves runtime-dependent words untouched. Compilation invokes
this pass only after full parsing; `check` still reports the original source.
No new dependency or parallel AST/IR was introduced.

Rustfmt, Clippy with warnings denied, 15 unit tests, 6 native CLI tests, and a
release build passed on Windows with Rust/Cargo 1.99.1. The existing source-check
example still reports its original 6 top-level nodes from both file and stdin.

The independent control-flow interpreter, query oracle, and generators are confined
to `src/prune/tests.rs`; ordinary statements are observed as opaque effects.
Its query records include the complete leaf operation, not just its name; program
observations use a combined ordered query/effect trace and retain earlier effects
on failure. The bounded checks cover:

- All 90,690 condition trees of up to 7 nodes over two constants, two runtime
  leaves, and shell/OS leaves. Four targets cover all shell/OS predicate truth
  combinations; true/false/error assignments yield 3,264,840 comparisons.
- Branch chains of up to 3 branches drawn from 10 conditions, with empty/nonempty
  bodies and an optional fallback: 159,984 observation comparisons.
- Idempotence, removal of static leaves, first-match selection, nested bodies,
  raw LF/CRLF payloads, source locations, and opaque argument/expansion values.
- Explicit error witnesses for env/have/exists with absorbing right operands;
  residual empty-body conditions still run and propagate the same failed query.
- Native CLI compilation at the frontend's combined nesting/condition capacity,
  and validation of malformed dead branches before specialization.

These are finite semantic-model checks, not real-shell adapter certification.
The existing frontend/CLI checks remain in place. Stage 3 owns the executable
backend; compilation still reports generation as not implemented.

## Stage 3: A PowerShell core executes end to end

Status: implemented in the working tree on 2026-11-04. No release or commit made.
Primary owners: `src/emit.rs`, `src/powershell.ps1`, `src/main.rs`, runtime tests.

- [x] Implement scalar/expansion encoding and runtime diagnostics, then set/unset,
  runtime predicates, grouped short-circuit branches, and PATH mutations.
- [x] Derive OS-specific PATH behavior from the selected target; keep runtime
  working-directory path resolution and home lookup out of the parser.
- [x] Preserve empty versus absent environment entries and PATH entries. Use
  literal filesystem/name queries and the specified external resolver.
- [x] Make successful empty programs/no-op branches return success. A failure
  stops later DSL effects and returns to the loading caller, retaining prior work.
- [x] Validate target capabilities and target-specific names before emission.
  All PowerShell statements are implemented, so none need a temporary rejection.
  Other unfinished backends remain explicitly unavailable.

Completion evidence: compile and execute the core in an isolated PowerShell
process; inspect environment, ordered PATH values, diagnostics, and completion.
Exercise empty strings, ASCII/Unicode quotes, spaces, dollar signs, backticks,
wildcards, and newline data. A branch truth table alone is insufficient: verify
skipped operands and failure propagation. Verify current-session effects survive
loading and that no query launches its named program or autoloads a module.

Windows execution is the first local vertical slice, not evidence of PowerShell
Unix conformance. Linux/Darwin runs remain required in Stage 6.

Observed completion: `src/emit.rs` validates PowerShell alias names throughout the
source before specialization, then emits semantic words and explicitly grouped
conditions. `src/powershell.ps1` owns the runtime adapters. Ordinary strings use
compact single quoting; strings containing Unicode single quotes use numeric
UTF-16 construction. No Rust dependency, new IR, or runtime service was added.

Real PowerShell tests verify present-empty versus unset environment values,
literal queries, runtime cwd/home, exact PATH order/spelling/empties, duplicate
movement and idempotence. Mixed `or`/`and` cases cover all eight assignments.
Skipped invalid-home operands do not fail; a reached invalid-home operand in
`x and false` does fail. Earlier effects survive and later effects are skipped.
Empty specializations return success. Error preferences are restored on failure.
The native CLI accepts the frontend's combined nesting/condition capacity through
PowerShell generation, and rejects invalid target names even in dead branches
without emitting a partial script.

The separate real-interpreter scalar codec test exhaustively roundtrips 12,720
strings of length 0–3 over 23 characters, including all four Unicode single quotes,
ASCII quotes/metacharacters, CR/LF/tab, and a supplementary Unicode character.
This checks the emitted value expression, independently of native argv transfer.

## Stage 4: PowerShell command and code boundaries are complete

Status: implemented in the working tree on 2026-11-04. No release or commit made.
Primary owners: the PowerShell emitter, shared semantic encoding, runtime fixtures.

- [x] Build one native executable test fixture that reports argument boundaries,
  stdout/stderr, exit status, and an execution marker. Reuse it across backends.
- [x] Implement structured aliases with invocation-time expansion, external
  resolution, caller-argument forwarding, and lifetime in the loading session.
- [x] Implement init's launch/capture/decode/evaluate sequence. Nonzero exit,
  invalid text, or launch failure must prevent code evaluation.
- [x] Implement one target code-unit boundary for matching raw bodies and successful
  init output. Preserve exact code contents and current-session scope.
- [x] Keep external resolution fresh, including after a preceding successful have
  followed by executable disappearance or PATH changes.

Completion evidence: native argument capture preserves zero/empty/multiword
arguments, Unicode, quotes, metacharacters, and argument order. Alias tests
distinguish definition-time from invocation-time expansion. Failed initializers
containing apparently valid script never execute it. Successful code retains
trailing newlines and environment/wrapper effects. Invalid raw payload syntax
fails inside its code unit without corrupting the outer generated grammar.

If a chosen capture strategy uses temporary resources, its implementation must
own their cleanup on normal and failed completion and document interruption
behavior. Do not add a persistent service or a runtime timeout policy without
a separate requirement. Arbitrary raw control transfer remains the specification's
explicit external-code boundary.

Observed completion: `tests/fixtures/native.rs` is compiled into a disposable
native program by the integration tests; it records length-prefixed UTF-8 argv,
emits exact byte streams, returns chosen statuses, and writes an execution marker.
`tests/powershell.rs` reuses it for commands and initializers. Test-owned temporary
directories are removed; production initialization uses in-memory capture and
`finally`-owned process/stream disposal, with no temporary files.

Aliases bind global/AllScope public names to private functions containing their
unevaluated structured words. Tests load them inside a function and invoke them
after that function returns, including replacing `echo` and wrapping an executable
with its own name. PowerShell `Standard` native argument passing preserves empty,
quoted, Unicode, multiword, wildcard, and common-parameter-looking arguments.
Native stdout remains pipeline output; `$LASTEXITCODE` retains the native status.
Nonzero alias exits throw a catchable error, avoiding ordinary PowerShell functions'
otherwise misleading successful `$?`. Invocation-local native preferences do not
change the caller's preference. Invalid Unicode arguments fail before launch.

Discovery uses application-only lookup. A local module-autoload preference proved
ineffective in the real interpreter; the adapter temporarily disables the global
preference and restores its original value or absence in `finally`. A module with
an observable load marker proves that have does not autoload or execute it.
Tests change PATH after discovery and remove a discovered executable; calls resolve
fresh, and have becomes false after disappearance.

Init drains both streams concurrently, checks exit status before decoding or
evaluating stdout, rejects invalid UTF-8/NUL, and dot-sources an independently
parsed script block. Both raw and init preserve exact code text and loading scope.
Tests cover a 256 KiB stdout payload concurrent with 128 KiB stderr, CRLF and
trailing blank lines, empty stdout, persistent function definitions, syntax/runtime
errors, native nonzero exit, and nonzero output containing valid-looking code.

Rustfmt, Clippy with warnings denied, all 26 tests (16 unit, 6 CLI, 4 real-session),
and a release build passed locally with Rust/Cargo 1.99.1 and PowerShell 7.6.9 on
Windows. CI now requires PowerShell 7.5+ and configures the same checks on Ubuntu
and Windows without runtime skips. Those remote jobs were not run in this task;
the full matrix remains Stage 6. README, CLI help, DSL implementation status,
and release guidance describe the actual PowerShell subset and pending backends.

## Stage 5: Bash, Zsh, and Fish implement the same contract

Status: implemented in the working tree on 2026-11-04. The user requested official
documentation review instead of installing missing shells in WSL. Stage 5 closes
with existing Bash execution and Zsh/Fish documentation review; actual Zsh/Fish
execution moves to Stage 6. This is not full matrix certification. No commit or
release made.
Primary owners: `src/emit/unix.rs`, `src/emit/unix.sh`, `src/emit/unix.fish`,
`tests/unix_shells.rs`, and the shared native fixture.

- [x] Implement the same scalar, environment, predicate, path, wrapper, initializer,
  code-unit, and error operations for each remaining shell.
- [x] Preserve condition structure through target grouping or structured control
  flow; do not rely on target operator precedence.
- [x] Keep data away from eval/source boundaries except explicitly selected raw
  code or successful initializer output.
- [x] Replace shell-specific old output assertions with shared behavioral cases
  plus only the target syntax checks that add useful coverage.
- [x] Remove obsolete placeholder/tilde rewriting, string-tail assembly, and dead
  emitter helpers as their actual consumers disappear.

Revised completion evidence: run the shared fixtures in existing Bash, review
Zsh/Fish against official syntax, expansion, variable, and source rules, and keep
unexecuted cases visible in Stage 6. A passing Bash backend does not certify Zsh
even though they share implementation. No local shell/tool installation is needed.

Implementation: `src/emit.rs` owns admission, specialization and dispatch;
the existing PowerShell implementation moved to `src/emit/powershell.rs`. Its
execution/codec tests still pass. No new Rust dependency, AST/IR, compatibility
compiler, or runtime service was introduced.

The Unix adapters lower the same AST to explicit true/false/error control flow.
Temporary values use function parameters rather than environment-named scratch
variables. Static command words take a direct argument path; dynamic words retain
evaluation order and scalar/argument boundaries. Queries read exported values and
walk literal PATH entries, bypassing functions, builtins, autoload and hashes.
PATH retains exact entries, including empty strings and nonexistent directories.
Native expression-interpreting variable slots fail before assignment; Bash's
integer-slot witness confirms rejection before arithmetic code can execute. Fish
reports its intrinsic `argv` slot as an adapter error. Fish PATH and Zsh native-slot
roundtrips still need execution, without changing the approved semantics.

Init captures to an owned file, checks process success and UTF-8/NUL validity,
then emits a separate source stream. Bash/Zsh use process substitution; Fish uses
a source pipeline. All three remove captures explicitly on success/failure,
avoiding dependence on the differing function EXIT-trap contexts.
Interruption can leave `shed.*` captures in TMPDIR. Runtime utilities are
printenv/iconv/mktemp/cat/tr/cmp/rm; Bash/Zsh use the system utility path, while
Fish probes `/usr/bin` and `/bin`. Availability on other targets needs execution.

`tests/unix_shells.rs` reuses the native witness, adding environment snapshots that
distinguish absent/empty. The shared DSL cases cover values/argv, PATH list laws,
literal queries, mixed precedence, late aliases, code-unit scope, failed/invalid
initializer output, short circuit, retained prior effects and capture cleanup.
The 12,720-string alphabet covers length 0–3, ASCII/Unicode quotes, metacharacters,
CR/LF/tab, backslash and emoji through emitted scalars into native argv. Tests also
cover raw CRLF/trailing-newline bytes, zero arguments, late expansion, and fresh
resolution after a PATH change.
On Unix a missing shell fails the suite. On Windows three groups are explicitly
ignored. Ubuntu CI now installs Zsh/Fish and requires these tests; this acquisition
is confined to the disposable runner. No remote workflow ran, and no local tools
were installed.

Observed Windows evidence: Rustfmt, Clippy with warnings denied, 26 existing tests,
compilation of the new test harness, and a release build passed on Rust/Cargo 1.99.1
and PowerShell 7.6.9. CLI checks generate all four targets at the frontend's combined
capacity, including dead-branch rejection of reserved Zsh alias names. Three Unix
runtime groups were ignored, not passed, on Windows.

Observed Linux evidence: escalated WSL execution works after the app restart.
Existing tumbleweed Bash 5.3.15 passed `cargo test --locked --test unix_shells
bash_conformance`; all six Linux CLI tests also passed. This used existing
Rust/Cargo 1.97.1 with a process-only `RUSTUP_TOOLCHAIN` override and an isolated
`/tmp/shed-stage5-target` build directory. Windows uses the pinned 1.99.1 toolchain.
An attempted 1.99.1 Linux toolchain acquisition failed; no toolchain was installed.
Missing Zsh/Fish were not installed, in accordance with the user's preference.

Official review (2026-11-04; Fish current docs identify 4.9.3):

| Boundary | Official basis and implementation consequence |
| --- | --- |
| Zsh command admission | [Grammar](https://zsh.sourceforge.io/Doc/Release/Shell-Grammar.html): reserve native keywords/modifiers such as `nocorrect`, `noglob`, and `typeset` before specialization; force builtins/external command dispatch explicitly. |
| Zsh data and argv | [Expansion](https://zsh.sourceforge.io/Doc/Release/Expansion.html): quoted scalar/positional expansions, fixed positional slices, and parameter type flags preserve data boundaries; status suffixes prevent newline loss. |
| Fish capture and argv | [Bash comparison](https://fishshell.com/docs/current/fish_for_bash_users.html) and [split](https://fishshell.com/docs/current/cmds/string-split.html): variable lists preserve arguments; final `string split`/`split0` carries explicit boundaries through substitution. |
| Fish environment/PATH | [set](https://fishshell.com/docs/current/cmds/set.html): use exported globals, `--path`/`--unpath`, and explicit colon splitting; assignment preserves preceding status, so establish success before testing assignment failure. Read exported values back to detect lossy/native-slot updates. |
| Code scope | [Zsh builtins](https://zsh.sourceforge.io/Doc/Release/Shell-Builtin-Commands.html) and [Fish source](https://fishshell.com/docs/current/cmds/source.html): source executes a separate unit in the loading shell; Fish supports pipeline input with a new local scope. |

These rules support the construction and identified concrete fixes; they do not
prove all interpreter/runtime combinations. Stage 6 owns execution of Zsh/Fish,
PowerShell Unix, and Darwin, including scope, PATH, native slots and cleanup.

## Stage 6: The declared matrix and public interface agree

Status: implemented and verified on 2026-11-05, including Stage 5's deferred
Zsh/Fish execution. No package release or merge to main was made.
Primary owners: `.github/workflows/ci.yml`, `src/main.rs`, user/contributor docs,
and package metadata only where the actual interface change requires it.

- [x] Execute the shared Zsh/Fish suites on isolated Linux CI, then conformance
  cases on every remaining declared shell/OS pair. Required target
  jobs must not silently skip unavailable shells or discard parser diagnostics.
- [x] Test real CLI file/stdin/error paths and canonical target admission. Keep
  script stdout separate from compiler diagnostics; verify no partial output.
- [x] Verify scalar codecs and native argument transfer separately; record the
  tested shell versions and launch protocol without claiming arbitrary program
  argument parsers are identical.
- [x] Update README, CLI help, description/examples, and affected release guidance
  to the implemented language; remove the pending-language notice only when true.
- [x] Preserve the current Rust minimum declaration only with suitable evidence;
  any proposed minimum change is a separate explicit compatibility decision.
- [x] Run the project checks below and inspect the final diff for obsolete
  source/word/path assumptions, new unsupported promises, and unrelated changes.

Completion evidence: a coverage table identifies each target pair, runtime
version, actual checks, and result. All normative example cases and remaining
behavioral counterexamples pass; unavailable jobs or failed adapter guarantees
remain incomplete work. Syntax-only or finite-model success does not substitute
for target execution.

Implementation: one three-OS CI matrix runs the complete suite,
records native versions, checks examples and builds release binaries. Linux owns
formatting/Clippy; a separate isolated Rust 1.85 job checks all targets without
changing the minimum declaration. Existing Ubuntu/Windows check names are kept.
The workflow uses a read-only token, does not persist checkout credentials, and
bounds/cancels stale runs. Small dependency-free builds no longer use a remote
cache action. Checkout v7.0.1 and the Rust toolchain action are pinned to exact
commits resolved from their official repositories on 2026-11-05:
[checkout](https://github.com/actions/checkout/tree/3d3c42e5aac5ba805825da76410c181273ba90b1)
and [rust-toolchain](https://github.com/dtolnay/rust-toolchain/tree/b725eda525362e7cf34af9a7243cac1e3f01a7ec).
Dependabot's existing Actions entry remains in place. No release workflow ran.

Unix scalar codecs now have independent real-interpreter checks over the same
12,720 strings used for native argv. The native-slot witness avoids `typeset -g`,
which is unnecessary at source scope and unavailable in Darwin's system Bash.
README includes safe compile-then-load examples for all shells, explicit target
OS selection, test prerequisites and the distinction between configuration and
observed coverage. Package metadata uses the canonical PowerShell name.

Local Windows checks passed: 26 tests, six explicitly ignored Unix tests,
Rustfmt, Clippy with warnings denied, and release build on Rust/Cargo 1.99.1.
Existing WSL Bash 5.3.15 also passed its codec/behavioral suites and six CLI tests
using the existing Rust 1.97.1 process override. No local tools were installed.

The [successful CI run](https://github.com/nostalume/shed/actions/runs/41952120266)
executed a `push` of source snapshot `10a12409a3b439cdeb2d5cb549919b79253a25fa`
on `codex/dsl-stage6-20261105`. All four jobs passed:

| OS / CPU | Shell | Observed version | Runtime checks | Result |
| --- | --- | --- | --- | --- |
| Linux / x86_64 | Bash | 5.2.21 | Independent scalar codec and shared Unix suite | Pass |
| Linux / x86_64 | Zsh | 5.9 | Independent scalar codec and shared Unix suite | Pass |
| Linux / x86_64 | Fish | 4.9.3 | Independent scalar codec, shared Unix suite and PATH rejection | Pass |
| Linux / x86_64 | PowerShell | 7.6.9 | Scalar codec and native state/command/code-unit suites | Pass |
| Darwin / aarch64 | Bash | 5.3.20 | Independent scalar codec, shared Unix suite and system-Bash rejection | Pass |
| Darwin / aarch64 | Zsh | 5.9 | Independent scalar codec and shared Unix suite | Pass |
| Darwin / aarch64 | Fish | 4.9.3 | Independent scalar codec, shared Unix suite and PATH rejection | Pass |
| Darwin / aarch64 | PowerShell | 7.6.9 | Scalar codec and native state/command/code-unit suites | Pass |
| Windows / x86_64 | PowerShell | 7.6.9 | Scalar codec and native state/command/code-unit suites | Pass |

The runtime jobs used Rust/Cargo 1.99.1. Linux passed 32 tests with no ignores;
Darwin passed 33 with no ignores; Windows passed 26, with six Unix-only tests
explicitly ignored. All three passed release builds and file/stdin example checks;
Linux also passed Rustfmt, Clippy and release-helper syntax checks. The separate
Rust 1.85.0 job passed `cargo check --all-targets --locked`; the declared minimum
is retained. YAML was parsed locally with existing tools; actionlint was absent
and was not installed. The hosted run supplies the workflow execution evidence.

Concrete corrections and boundaries established by execution:

- Native Fish normalizes empty PATH entries to `.`. The user approved explicit
  rejection before mutation instead; tests cover empty, leading, trailing and
  middle empty entries, unchanged PATH, retained prior effects and skipped later
  statements. Other backends keep the common exact-entry rules.
- Bash 3.2's source implementation reads the initial file size and cannot consume
  these streams reliably. The runtime baseline is Bash 5.2+; Darwin CI selects
  modern Bash and tests that the system Bash rejects the load before effects.
  Fish 3.7 also exposed capture ordering failures; the supported baseline is
  Fish 4.9+. Linux CI uses the official 4.9.3 standalone archive and verifies its
  published SHA256; Darwin uses Homebrew. Acquisition stays on disposable runners.
- Fish collects output completely before emitting one scalar/status frame.
  UTF-8 validation precedes scalar emission. No legacy adapter or second
  environment store was introduced.
- Successful init captures are opened and unlinked before streaming; failed
  captures are removed before emitting a failure return. This removes the race
  between loader return and producer cleanup. Interruption before unlinking can
  still leave a capture, as documented by the external-code boundary.
- Utilities use `/usr/bin`, then `/bin`, independently of DSL PATH. Test cwd
  expectations use the actual physical/provider directory, and executable
  selection compares canonical file identity, handling Darwin symlinks and
  Windows short paths. `.gitattributes` fixes shell scripts to LF for WSL use.

These results cover the declared shell/OS pairs at the observed versions and CPU
lanes. They do not prove every version, architecture, arbitrary native argument
parser, or embedded program. The validation branch contains isolated snapshots;
the local main ref and original staging area were preserved.

## Final review after Stage 6

Status: complete on 2026-11-05 within the execution boundaries below.

- Reproduced a compiler stack overflow with 20,000 runtime-dependent `elif`
  branches. Unix emission now uses sequential checks in a single-pass block with
  explicit error arms, avoiding recursive native `elif` grammar too. The CLI
  regression covers every backend; runtime cases cover first
  match, skipped later queries and failure before a later known-true branch.
- Unix alias definition was not guarded, so a readonly Bash function collision
  could be followed by later effects and a successful final status. Definition
  is now checked, and a real readonly-function witness verifies failure stopping.
  Alias admission is backend-specific: Fish essential builtins are rejected even
  in dead branches, while ordinary names remain available where the target allows.
- PowerShell rendering returns its actual infallible result directly; admission
  retains the fallible boundary. No Rust dependency or additional IR was added.
- Both release helpers now stop on native command failures and distinguish Git
  diff errors from changed files. Version edits are prepared before writing either
  Cargo manifest; no pre-release Nix version/hash mismatch is created.
- Scoop and Nix updates have one verified-asset generator and one serialized
  two-file commit. The fixed Scoop version template/checksum URL, stale release
  guard and failed-download/no-write cases are covered by dependency-free tests.
- Release reuses CI's tests, exact five-target builds and Nix consumer check;
  publishing receives only checked artifacts and a separate write token. Workflow
  event data enters through environment values and source/tag identity checks.
  No local tools are installed and no release is executed by these checks.

Validation source: `814eba802e3734f17e3f4188a9b8ff8ed4171d0a` on the explicitly
authorized isolated branch `codex/dsl-stage6-20261105`.
[Final review CI](https://github.com/nostalume/shed/actions/runs/41959316083)
passed all ten jobs:

- Windows: 27 tests passed; Ubuntu: 33; Darwin: 34, including the unsupported
  system-Bash guard. Ubuntu used Bash 5.2.21, Zsh 5.9 and Fish 4.9.3; Darwin used
  Bash 5.3.20, Zsh 5.9 and Fish 4.9.3. All native semantic/codec cases passed.
- Formatting, Clippy, release builds and Rust 1.85 all-target checking passed.
  The five Linux musl, Darwin and Windows release artifacts and sidecars were
  built and transferred through the pinned artifact actions.
- All six Python tests passed, including both real Bash and PowerShell helpers
  with fake Cargo/Git failure injection; no publication commands reached real Git.
- Nix 2.35.4 evaluated the flake, installed the exact just-built Linux x86_64
  artifact through the package derivation, checked its version and accepted the
  committed DSL example. Both catalogs were rendered from all five checked assets
  only in the disposable runner's workspace.

Local Windows formatting, 27 tests, Clippy and release build also passed. Existing
WSL Bash passed the branch/alias regressions and packaging tests (the PowerShell
helper was skipped locally and exercised remotely). Both Bash and PowerShell
parsed generated scripts with 20,000 branches. No local tools were installed.

The tag-triggered reusable call, release creation, workflow-run catalog commit and
real Scoop installation were not executed: these require an actual publication.
Their orchestration was reviewed statically; the common test/build/artifact/
catalog/Nix-consumer path above was executed. This is bounded implementation
evidence, not a machine-checked proof or a claim about arbitrary embedded code.
During this review, local main and the original staging area were preserved.
The result was recorded after the validated source snapshot. Release preparation
advances Cargo to 0.2.0 and excludes `.agents/`; application and workflow behavior
remain as validated, with tag CI checking the final publication source again.

## Validation and execution boundaries

Use the repository-selected toolchain. Existing Rust checks are:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all --locked
cargo build --release --locked
```

Add a focused check with the implementation whose semantic claim needs it; do not
create a separate permanent test project merely to copy the earlier experiment.
No new Rust test dependency is selected by this plan.

Local verification uses existing Windows PowerShell and WSL Bash. Stage 6's remote
matrix ran on an explicitly approved validation branch; missing runtimes were
acquired only on disposable hosted runners. This plan does not independently
authorize local installation, additional remote work, or a release.

## Readiness, recovery, and reopening

Stages 1–6 are complete within the recorded runtime and representability bounds.
Capture exact backend failures when they arise; a probe may select a private implementation, but cannot weaken
the approved contract. If none can meet it, reopen only the affected target or
operation before changing the specification.

Review each implementation against its own observable outcome. Stages do not
promise independently releasable binaries. If development stops, keep the actual
supported subset and pending status explicit. Undo dependent changes together
rather than restoring lossy string boundaries or silently mixing dialects.
Local revert does not undo environment effects from a script the user has loaded.

Before each implementation stage, refresh revision/dirty state and preserve the
existing user changes. Update this plan's statuses with observed completion
evidence. Completion of local development grants no package publication, remote
push, or release authority.
