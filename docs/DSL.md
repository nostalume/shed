# DSL specification

**Status: approved language; frontend, specialization, and all four emitters implemented.**
This is the canonical language contract for the next shed compiler, approved on
2026-11-04. The development checkout accepts this source grammar through `check`;
all four backends generate the runtime contract below. The nine declared shell/OS
pairs have passed real-interpreter CI under the adapter limits recorded below.
Published 0.1.6
binaries use the previous DSL. Compatibility with
that DSL is not a requirement.
The [development plan](../plans/dsl-development.md) owns implementation
stages and completion evidence.

## Targets

Compilation receives a resolved pair `(shell, os)`. Shell names are `bash`,
`zsh`, `fish`, and `powershell`; OS names are `linux`, `darwin`, and `windows`.
These names are used consistently in conditions, raw tags, and expansions.
There is no implicit `pwsh` to `powershell` name substitution.

The initial target matrix is:

| Shell | OS |
| --- | --- |
| Bash, Zsh, Fish | Linux, Darwin |
| PowerShell | Windows, Linux, Darwin |

Unknown names and unsupported pairs are compile errors. Windows Bash/MSYS is
outside this matrix; its path and process conventions must not be inferred from
the Windows PowerShell target. The CLI resolves defaults before compilation;
compile-time evaluation uses the selected target, never ambient machine queries.

## Source and words

Source is UTF-8 without NUL. LF and CRLF both delimit ordinary source lines.
One ordinary statement occupies one physical line; blank lines are ignored.
There is no backslash line continuation. Raw blocks are the multiline form.

A `WORD` is one scalar string expression built from adjacent fragments:

| Fragment | Meaning |
| --- | --- |
| Bare text | Literal text; no expansion or escape processing. |
| ASCII single quotes `'...'` | Literal content; no expansion or escapes. |
| ASCII double quotes `"..."` | Literal content plus the escapes and expansions below. |

Spaces and tabs outside quotes separate words. Quotes can start anywhere inside
a word, so `--config="hello # users"` is one word. Empty quotes produce a present,
empty word. Curly quotation marks are ordinary data in the DSL.

Outside quotes, `#` begins a comment only at a word boundary: at the start of a
line or after separating whitespace. Within an already-started word it is data.
After the closing quote of a word, an immediately adjacent `#` is also data.

```text
set A abc#def       # value: abc#def
set B "abc # def"   # value: abc # def
set C 'C:\tools\'   # final backslash is literal
```

In bare text, backslash is literal. Dollar sign, backtick, semicolon, ampersand,
pipe, angle brackets, braces, and square brackets must be quoted. Parentheses
are condition punctuation, not unquoted word content. The `<<MARKER` raw header
has its own grammar. An asterisk or question mark is literal data, never a glob.

Double quotes recognize exactly `\\`, `\"`, `\$`, `\n`, `\r`, and `\t`.
Unknown escapes, unfinished quotes, and unfinished expansions are compile errors.
A physical newline cannot occur inside an ordinary quoted word; use an escape
for a newline value. Single quotes do not have an escape for a single quote;
use a double-quoted fragment to represent that character.

Keywords are recognized only as bare words at the grammatical positions where
they are expected. An operand is consumed before testing for the next operator.
A quoted keyword is word data, not a structural token.

## Values and expansion

Only double-quoted fragments recognize these expansions:

| Form | Meaning |
| --- | --- |
| `${env:NAME}` | Environment variable at the point of execution. |
| `${home}` | The current target session's home directory. |
| `${shell}` | The selected canonical shell name. |

Environment names match `[A-Za-z_][A-Za-z0-9_]*`. Lookup and update follow the
target OS's environment-name case rules. Shell-local variables are not environment
variables. An unset variable expands to the empty string. Home lookup must yield
a valid absolute directory; failure is a runtime error at the point of use.

Expansion produces string data exactly once. There is no recursive expansion,
word splitting, globbing, command substitution, or general expression evaluation.
All runtime values must be representable Unicode strings without NUL; an
unrepresentable runtime value is an error, not a lossy conversion.

```text
set A '${env:OTHER}'       # literal text
set B "\${env:OTHER}"      # the same literal text
init tool "${env:FLAGS}"   # exactly one argument, including when empty
```

`"$(date)"`, bare `$HOME`, and unknown expansions are rejected. A value read
from the environment that contains `$(date)` or `${env:OTHER}` stays data.
Tilde has no special meaning; use `"${home}/..."`.

The semantic representation is:

```text
Word = Part*
Part = Literal(text) | Env(name) | Home | TargetShell
```

This representation preserves interpretation, not the spelling of equivalent
quote forms. Preserve source locations separately for diagnostics. No later pass
may recover parameter boundaries or expansion types by re-tokenizing strings.

## Statements

```text
set NAME WORD
unset NAME
path+ WORD
path- WORD
alias NAME = PROGRAM WORD*
init PROGRAM WORD*
```

`PROGRAM` is a word. `NAME` is a static identifier, not an expansion. Environment
names use the form above; alias names use `[A-Za-z_][A-Za-z0-9_-]*`.
Alias names must be diagnosed if the selected target
cannot define and invoke them as command identifiers; names must never be
interpolated as unchecked executable syntax.

Every statement consumes its complete line. `set` takes exactly one word;
`set GREETING hello world` is an error, not implicit concatenation. All operands
are evaluated left to right before the statement's effect, except that alias
definitions retain their words for later invocation.

### Environment

`set` exports the evaluated value to the current session. Setting `""` creates
a present, empty variable; `unset` removes the variable. Failure to update the
target environment is a runtime error. Subsequent statements observe the updated
environment. No compilation pass pre-evaluates environment-dependent words.

### Paths and PATH

Both PATH operations and `exists` interpret relative paths against the working
directory at the time that statement or condition executes. They do not use the
source file's directory or a directory on the compiler's machine. PATH insertion
resolves a relative operand to an absolute spelling before mutation. Path joining
does not require existence, resolve symlinks, or globally rewrite backslashes.
Target path conventions govern recognition of absolute and relative forms.

PATH uses `:` on Linux/Darwin and `;` on Windows. Each mutation reads the current
environment string and decodes its list, retaining existing empty entries.
An unset PATH decodes as `[]`; a present empty string decodes as `[""]`.
An inserted path must be nonempty and contain no target PATH delimiter.

Let `D_p(P)` remove every list entry whose string is exactly `p`:

```text
path+ p: [p] ++ D_p(P)
path- p: D_p(P) ++ [p]
```

The operation moves an existing exact match to the requested end. Comparison is
string equality, not filesystem identity, case folding, or symlink equivalence.
Existing entries otherwise retain their order and spelling. The list is encoded
back into PATH. At a fixed evaluated `p`, both operations are idempotent.

### Structured aliases

`alias gs = git status --short` defines a command wrapper, not a shell-code
string. Defining it does not evaluate its program or argument words. Each
invocation evaluates them against the then-current environment, appends the
caller's already-separated argument values, and calls the external program.

There is no re-parsing, splitting, globbing, or evaluation of the resulting
argument values. Empty arguments survive. External resolution bypasses shell
aliases, functions, builtins, and PowerShell cmdlet argument binding. A wrapper
around the external program with its own name therefore does not recurse through
itself. Definition failure is a runtime error; invocation reports the external
program's result to its caller.

### Initializers

`init PROGRAM WORD*` resolves and launches an external executable with the
structured argument list. An empty program or failed launch is a runtime error.
A preceding `have` does not cache a program or guarantee a later launch succeeds.

Capture stdout while forwarding stderr. On nonzero exit, report failure and do
not evaluate captured output. On success, stdout must be valid UTF-8 text without
NUL; preserve its contents, including line endings and trailing newlines. Execute
it as a separate target-shell code unit in the current loading session.

Successful process execution and successful code execution are separate outcomes.
A code-unit failure stops subsequent DSL statements; prior effects are retained.
The external program's own interpretation of its arguments is outside DSL data
encoding guarantees.

## Conditions

```text
or_expr  = and_expr ("or" and_expr)*
and_expr = unary ("and" unary)*
unary    = "not" unary | "(" or_expr ")" | leaf
leaf     = "true" | "false"
         | "have" WORD | "exists" WORD | "env" NAME
         | "os" OS_NAME | "shell" SHELL_NAME
```

`not` binds tighter than `and`, then `or`. Binary operators associate left.
Parentheses group conditions. There is no implicit string-to-Boolean conversion.

| Leaf | Result |
| --- | --- |
| `true`, `false` | Boolean constants. |
| `shell NAME`, `os NAME` | Compare the immutable selected target. |
| `have WORD` | Query external executable resolution without execution or module autoload. An empty name or no executable yields false. |
| `exists WORD` | Test a literal path without wildcard interpretation. False means existence could not be confirmed, including absence or access restrictions. An empty path yields false. |
| `env NAME` | True exactly when the environment entry exists and is nonempty. |

Operand evaluation failures, inability to obtain required runtime context, and
adapter execution failures propagate as runtime errors, not ordinary false.
These are distinct from the normal negative lookup outcomes defined above.
Queries produce no successful stdout or incidental diagnostics.

The parser consumes a leaf operand before looking for an operator. Thus
`have and and env ENABLED` queries the program named `and`, then applies Boolean
`and`. Quoting the program name is permitted but not needed for disambiguation.

Condition evaluation is left to right and short-circuits:

| Expression | Result and evaluation |
| --- | --- |
| `false and e` | False; do not evaluate `e`. |
| `true and e` | Evaluate and return `e`. |
| `true or e` | True; do not evaluate `e`. |
| `false or e` | Evaluate and return `e`. |
| `error and e`, `error or e` | Propagate error; do not evaluate `e`. |
| `not error` | Propagate error. |

Error is a failure outcome, not a third ordinary truth value. Backends must
preserve grouping, short-circuit order, and failure propagation rather than
concatenating target operators and relying on their precedence.

## Branches

```text
if CONDITION
    STATEMENT*
(elif CONDITION
    STATEMENT*)*
(else
    STATEMENT*)?
end
```

Evaluate conditions in order; execute only the first matching body, or the else
body when none matches. A condition failure stops the program before later
conditions or bodies. Nested and empty bodies are valid.

Only comments and separating whitespace may follow `else` or `end`. Unmatched
delimiters, a second else, elif after else, and incomplete blocks are errors.
Data containing a keyword never opens or closes a branch.

Validate ordinary DSL syntax, names, and expansions throughout the source before
pruning, including statically dead branches. Raw bodies are opaque, but their
headers and delimiters must be valid even when unselected. External executable
availability and runtime values are not compile-time admission checks.

## Partial evaluation

Only target properties and Boolean constants are compile-time facts. Do not
query the compiler's environment/filesystem, propagate preceding environment
assignments, cache runtime queries, or reorder them.

Represent a specialization result as a Boolean constant or a residual expression.
A predictable truth value alone does not imply that evaluation may be erased.

After recursively specializing children, apply these rules:

| Expression | Replacement |
| --- | --- |
| `false and x` | `false` |
| `true and x` | `x` |
| `x and true` | `x` |
| `true or x` | `true` |
| `false or x` | `x` |
| `x or false` | `x` |
| `not true`, `not false` | `false`, `true` |
| `not not x` | `x` |

Retain residual `x and false` and `x or true`. They must still evaluate `x`
and propagate any failure. Do not deduplicate repeated runtime queries or
apply additional algebraic identities that change reached-query order.

Normalize each branch chain from left to right:

1. Remove a constant-false condition and its body.
2. Retain a residual condition and recursively specialize its body.
3. A constant-true condition becomes the fallback of retained preceding branches;
   remove all following branches. With no preceding branches, inline its body.
4. If no constant-true condition occurs, retain the specialized original fallback.

An empty selected body is a successful no-op. Empty bodies do not justify
deleting a residual condition. Backends must emit a valid empty-command
representation where the target syntax requires one.

## Code boundaries

```text
raw powershell <<PWSH
$env:MESSAGE = "native PowerShell syntax"
PWSH
```

A raw header contains a canonical shell name and an ASCII marker matching
`[A-Za-z_][A-Za-z0-9_]*`. It can have a normal whitespace-separated trailing
comment. The first following line consisting exactly of the marker ends the
block. The marker has no indentation or trailing whitespace; its LF/CRLF is not
part of the marker. A closing marker at EOF is allowed.

Inside the body, do not recognize DSL comments, quotes, escapes, expansions, or
branch delimiters. Retain all body characters and original LF/CRLF sequences,
including the newline immediately before the closing marker. Do not de-indent
or trim the body. A missing closing marker is always a source error.

A tag matching the selected shell executes the body; other tags are no-ops.
Interpret the matching body as a separate code unit, not text spliced into the
outer generated script's grammar. Do not interpolate DSL values into its source.
Exported environment variables are an explicit data bridge when needed.

Raw code and successful initializer output use the same code-unit execution
boundary. Preserve the current loading session's effects and scope. Syntax or
execution failure reported by the target interpreter is a runtime failure.
Neither boundary promises transactional rollback.

Arbitrary target code can terminate the shell, transfer control, or change native
options. Its native effects are outside unconditional portable-core guarantees.
A whole-program proof involving such code requires an explicit contract for that
code; isolation and literal transmission do not prove the payload correct.

## Errors and completion

A compilation error reports a source location and emits no partial script.
Successful compilation does not run initializers, inspect runtime file existence,
or mutate the caller's environment. A frontend check validates the DSL; it does
not certify raw bodies or initializer output as valid target programs.

Structured statements execute sequentially. On runtime error, report a diagnostic,
stop following DSL statements, and return failure to the loading caller. Do not
actively terminate the interactive shell. Earlier effects remain. Successful
structured loading returns success, including empty programs and no-op branches.
Raw control transfers retain the explicit native-code boundary above.

## Correctness obligations

The semantic state includes the environment, command-wrapper definitions, and
working directory, together with target runtime query operations. PATH lists
are decoded views of the environment, not an independently authoritative state.

Required properties are:

- Literal and expansion encoding preserves scalar values.
- Structured calls preserve argument count, order, empties, and values.
- Parsing uniquely consumes complete statements and retains semantic distinctions.
- Specialization preserves result/failure and the ordered runtime queries reached.
- Target emission preserves condition-tree grouping, short circuit, effects, and
  failure propagation under the target adapter's stated contract.
- A normally returning raw/initializer code unit remains an explicit external
  effect; stronger whole-program guarantees require its contract.

For a literal encoder `L` and value `v`, require
`Eval(Parse(L(v))) = v`. Character-wise escaping and fragment concatenation
support an induction over the value's characters and parts.

For specialization `PE_t`, require
`Eval(PE_t(e), state) = Eval(e, target=t, state)`, including failure and the
reached-query sequence. Structural induction uses the short-circuit rules above;
it cannot use the invalid right-absorbing identities. Branch-chain preservation
then follows by induction over ordered branches. Specialization should also
remove all target leaves and be idempotent.

Every target value encoder separately owes
`TargetEval(Encode_t(v)) = v`. Target-native quotation rules, including non-ASCII
quote characters, must be checked; ASCII-only escaping is not a general proof.
Finite enumeration and real-shell experiments can falsify implementations but
do not constitute a machine-checked proof of the compiler.

## Implementation status

The development checkout implements the frontend, source check, and conservative
target specialization on compilation paths. The source check remains
target-independent. It retains semantic words, structured argument vectors,
ordered branches, and raw contents.
There is no legacy parser or source-directory path anchoring. PowerShell generation
implements every statement above. Bash, Zsh, and Fish also generate every statement.
Real-interpreter CI has passed for all declared pairs, including the approved Fish
representability errors. Exact runtime versions and scope are recorded in the
development plan; this does not certify arbitrary shell versions or embedded code.

The PowerShell adapter requires 7.5+ for present-empty environment values.
The Bash adapter requires 5.2+ and rejects older versions before DSL effects.
The Fish adapter requires 4.9+; older versions are rejected before DSL effects.
Darwin's system Bash 3.2 [reads source streams using their initial file size](https://github.com/apple-oss-distributions/bash/blob/main/bash-3.2/builtins/evalfile.c#L127);
the implementation uses modern Bash rather than a separate legacy code-unit path.
PowerShell literals use single quoting; Unicode single-quote characters use numeric
UTF-16 construction. Runtime strings are checked for NUL and invalid Unicode.
Conditions retain explicit tree grouping. PATH uses the selected OS delimiter,
runtime filesystem working directory, and ordinal string equality.

Native discovery is application-only. It temporarily disables the global module
autoload preference (the lookup API ignores a function-local preference), then
restores its value or original absence in `finally`. Queries never execute the
resolved program, and invocations resolve again.

Aliases bind a public global/AllScope alias to a private global function containing
the structured words. This survives a loading function's return and replaces
ordinary built-in aliases without storing executable command strings. PowerShell
keywords and names starting with `__shed_` are reserved alias identifiers.
The adapter uses PowerShell's `Standard` native argument protocol, preserves
stdout in the pipeline and `$LASTEXITCODE`, and throws a catchable error on
nonzero exit. Caller arguments are not bound as PowerShell common parameters.
The fixture verifies the Windows native argument convention; arbitrary programs'
own parsers remain outside the encoding guarantee.

Init uses .NET `ProcessStartInfo.ArgumentList`, concurrently drains stdout/stderr,
then strictly decodes successful stdout as UTF-8. Process/stream resources are
disposed in `finally`; no temporary runtime files or service are used. Init and
raw both dot-source a separately parsed script block in the loading scope, keeping
the code text intact. Loading temporarily enables terminating interpreter/native
errors and restores the caller's error preferences on success or failure. These
are catchable errors, not interactive-shell termination. External code that
changes error handling or transfers control retains the boundary described above.

PowerShell 7.6.9 passed real-runtime CI on Windows, Linux and Darwin, including
scalar values, native arguments, late aliases, initialization, raw code, scope,
short-circuit errors and retained prior effects.

The Unix emitters lower conditions to explicit true/false/error
control flow, with positional parameters holding private temporary state. Bash
and Zsh share syntax/runtime adapters. Fish collects one complete scalar without
trimming newlines, then emits a NUL-framed scalar/status pair through `string split0`.
This keeps empty arguments distinct and orders data before status. Bash, Zsh and
Fish passed scalar/native-argument, PATH, query, short-circuit, failure, alias-scope
and code-unit checks on Linux and Darwin.
See Fish's [split0 contract](https://fishshell.com/docs/current/cmds/string-split.html).

Unix queries read actual exported entries through `printenv`; they do not read
unexported shell-local variables. External resolution walks literal PATH entries
and checks executable files, bypassing functions, builtins, autoload, and hashes.
Native variable attributes that could reinterpret assigned data are rejected at
runtime. The Fish adapter treats its intrinsic `argv` binding as an adapter error;
it does not claim ordinary environment semantics for that native slot.
Fish also rewrites empty PATH entries to `.` in its native variable machinery.
The user-approved adapter boundary (2026-11-05) is to reject any PATH assignment
containing an empty entry before mutation, including a present-empty PATH.
The existing PATH and earlier DSL effects remain intact; later statements stop.
The language's exact entry/spelling rules are unchanged for representable values
and other backends. This limitation must not be hidden by normalization or a
second environment store.

Init captures bytes into a private temporary file, verifies successful exit and
UTF-8/NUL constraints before emitting code, and sources the separate code stream
in the loading shell. Bash/Zsh use process substitution; Fish uses a source
pipeline. Successful captures are opened, unlinked and then streamed; failed
captures are removed before the failure code is released. Cleanup therefore
precedes code execution or an early failure return. Interruption before unlinking
can leave temporary captures; no persistent runtime service is introduced.
Official documentation review covers Zsh's
[reserved words and command modifiers](https://zsh.sourceforge.io/Doc/Release/Shell-Grammar.html),
[parameter expansion](https://zsh.sourceforge.io/Doc/Release/Expansion.html),
and Fish's [substitution boundaries](https://fishshell.com/docs/current/fish_for_bash_users.html),
[assignment status and path variables](https://fishshell.com/docs/current/cmds/set.html),
and [current-shell source scope](https://fishshell.com/docs/current/cmds/source.html).
These rules support the adapter construction; the development plan records the
separate execution evidence, including Fish PATH rejection, special-variable
handling, source scope, and utility availability. Unix runtime
utilities are resolved in `/usr/bin`, then `/bin`, independently of DSL PATH.

The frontend reports source-located capacity errors rather than overflowing the
main stack. Currently it permits 128 nested if-blocks and a budget of 128 condition
leaves, unary/binary operators, and grouping openings per condition. These are
compiler capacity limits, not additional language syntax. Raw payload contents
do not consume condition or nesting capacity.

Use the [development plan](../plans/dsl-development.md) for the current baseline,
stage dependencies, acceptance conditions, and the first executable task. Keep
progress there rather than changing the normative rules to match incomplete code.
