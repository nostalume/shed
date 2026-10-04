//! Integration tests — full source → emitted-text round-trips.
//!
//! Philosophy (per AGENT.md):
//!   - Test important, integrated behaviour: parse + emit together.
//!   - One test per interesting scenario, not one test per function.
//!   - Inline emitter unit-tests (cond, node, indent) live in the emitter
//!     modules themselves and are not duplicated here.

use crate::{
    emit::{Emitter, bash::BashEmitter, fish::FishEmitter, pwsh::PwshEmitter},
    parser::Parser,
    prune::prune_nodes,
};

// ── helpers ──────────────────────────────────────────────────────────────────

fn bash(src: &str) -> String {
    let ast = prune_nodes(Parser::new(src, None).parse().unwrap(), "bash");
    BashEmitter::new("bash").render(&ast)
}
fn zsh(src: &str) -> String {
    let ast = prune_nodes(Parser::new(src, None).parse().unwrap(), "zsh");
    BashEmitter::new("zsh").render(&ast)
}
fn fish(src: &str) -> String {
    let ast = prune_nodes(Parser::new(src, None).parse().unwrap(), "fish");
    FishEmitter.render(&ast)
}
fn pwsh(src: &str) -> String {
    let ast = prune_nodes(Parser::new(src, None).parse().unwrap(), "pwsh");
    PwshEmitter.render(&ast)
}

// ── set / path / call across all shells ────────────────────────────────────

/// set emits the correct shell-specific export for each target.
#[test]
fn set_all_shells() {
    assert_eq!(bash("set EDITOR nvim"), "export EDITOR=\"nvim\"");
    assert_eq!(fish("set EDITOR nvim"), "set -gx EDITOR \"nvim\"");
    assert_eq!(pwsh("set EDITOR nvim"), "$env:EDITOR = \"nvim\"");
}

#[test]
fn pwsh_set_preserves_literal_backslashes() {
    assert_eq!(
        pwsh(r#"set TOOL C:\tools\bin"#),
        "$env:TOOL = \"C:\\tools\\bin\""
    );
}

/// path+ / path- generate the correct PATH mutation per shell,
/// wrapped in a deduplication guard (bash/pwsh) or natively deduplicating (fish).
#[test]
fn path_prepend_and_append() {
    let b_prepend = bash("path+ /usr/local/bin");
    assert!(
        b_prepend.contains("export PATH=\"/usr/local/bin:$PATH\""),
        "bash prepend add: {}",
        b_prepend
    );
    assert!(
        b_prepend.contains("[[ "),
        "bash prepend guard: {}",
        b_prepend
    );

    let b_append = bash("path- /opt/bin");
    assert!(
        b_append.contains("export PATH=\"$PATH:/opt/bin\""),
        "bash append add: {}",
        b_append
    );
    assert!(b_append.contains("[[ "), "bash append guard: {}", b_append);

    // fish_add_path deduplicates natively -- no extra guard needed.
    assert_eq!(
        fish("path+ /usr/local/bin"),
        "fish_add_path -gP \"/usr/local/bin\""
    );
    assert_eq!(fish("path- /opt/bin"), "fish_add_path -gaP \"/opt/bin\"");

    let p_prepend = pwsh("path+ C:/tools");
    assert!(
        p_prepend.contains("$env:PATH = \"C:/tools;$env:PATH\""),
        "pwsh prepend add: {}",
        p_prepend
    );
    assert!(
        p_prepend.contains("-notcontains"),
        "pwsh prepend guard: {}",
        p_prepend
    );
}

/// call replaces {shell} with the actual shell name.
#[test]
fn call_shell_placeholder() {
    assert_eq!(
        bash("call starship init {shell}"),
        "eval \"$(starship init bash)\""
    );
    assert_eq!(
        zsh("call starship init {shell}"),
        "eval \"$(starship init zsh)\""
    );
    assert_eq!(
        fish("call starship init {shell}"),
        "starship init fish | source"
    );
    assert_eq!(
        pwsh("call starship init {shell}"),
        "Invoke-Expression (& starship init powershell)"
    );
}

// ── conditional guards ────────────────────────────────────────────────────────

/// `if have` emits a command-existence check and the correct block structure.
#[test]
fn if_have_all_shells() {
    let src = "if have cargo\npath+ $HOME/.cargo/bin\nend";

    let b = bash(src);
    assert!(b.contains("command -v cargo"), "bash: {}", b);
    assert!(b.contains("export PATH"), "bash: {}", b);
    assert!(b.contains("fi"), "bash: {}", b);

    let f = fish(src);
    assert!(f.contains("type -q cargo"), "fish: {}", f);
    assert!(f.contains("fish_add_path"), "fish: {}", f);
    assert!(f.contains("end"), "fish: {}", f);

    let p = pwsh("if have cargo\npath+ C:\\cargo\\bin\nend");
    assert!(p.contains("Get-Command cargo"), "pwsh: {}", p);
    assert!(p.contains("$env:PATH"), "pwsh: {}", p);
}

/// `if os` with elif: on Linux the prune pass folds os statically,
/// so branches that match the compile OS are inlined and others dropped.
#[test]
#[allow(unused_variables)]
fn if_os_with_elif() {
    let src = "if os darwin\nset BROWSER open\nelif os linux\nset BROWSER xdg-open\nend";

    let b = bash(src);
    // On Linux: darwin→false (dropped), linux→true (inlined), no if wrapper.
    // On macOS: darwin→true (inlined), no if wrapper.
    // On Windows / other: both stay as runtime checks.
    #[cfg(target_os = "linux")]
    {
        assert!(
            b.contains("export BROWSER=\"xdg-open\""),
            "bash linux: {}",
            b
        );
        assert!(!b.contains("if "), "bash linux: unexpected if: {}", b);
    }
    #[cfg(target_os = "macos")]
    {
        assert!(b.contains("export BROWSER=\"open\""), "bash macos: {}", b);
        assert!(!b.contains("if "), "bash macos: unexpected if: {}", b);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        assert!(b.contains("Darwin"), "bash: {}", b);
        assert!(b.contains("Linux"), "bash: {}", b);
        assert!(b.contains("elif"), "bash: {}", b);
        assert!(b.contains("fi"), "bash: {}", b);
    }

    let f = fish("if os darwin\nset BROWSER open\nend");
    #[cfg(target_os = "macos")]
    assert!(f.contains("set -gx BROWSER"), "fish macos: {}", f);
    #[cfg(not(target_os = "macos"))]
    // darwin is false on non-mac → block dropped
    assert!(
        !f.contains("BROWSER"),
        "fish non-mac: darwin block should be dropped: {}",
        f
    );

    let p = pwsh("if os windows\nset SHELL_NAME pwsh\nend");
    #[cfg(target_os = "windows")]
    assert!(p.contains("$env:SHELL_NAME"), "pwsh windows: {}", p);
    #[cfg(not(target_os = "windows"))]
    assert!(
        !p.contains("SHELL_NAME"),
        "pwsh non-windows: windows block should be dropped: {}",
        p
    );
}

/// `if shell` — compile-time pruning applies for self-shell.
/// When compiled for the matching shell the prune pass inlines the body;
/// when compiled for a different shell the block is dropped entirely.
#[test]
fn if_shell_self_and_cross() {
    // each shell compiled for itself: body is inlined, no if-wrapper remains
    let b = bash("if shell bash\nset X 1\nend");
    assert!(
        b.contains("export X=\""),
        "bash self: body not inlined: {}",
        b
    );
    assert!(
        !b.contains("if "),
        "bash self: unexpected if-wrapper: {}",
        b
    );

    let f = fish("if shell fish\nset X 1\nend");
    assert!(
        f.contains("set -gx X"),
        "fish self: body not inlined: {}",
        f
    );
    assert!(
        !f.contains("if "),
        "fish self: unexpected if-wrapper: {}",
        f
    );

    let p = pwsh("if shell pwsh\nset X 1\nend");
    assert!(p.contains("$env:X"), "pwsh self: body not inlined: {}", p);
    assert!(
        !p.contains("if ("),
        "pwsh self: unexpected if-wrapper: {}",
        p
    );

    // bash compiled for fish-shell: block is dropped (empty output)
    let cross = bash("if shell fish\nset X 1\nend");
    assert!(
        cross.is_empty(),
        "bash cross: expected empty, got: {}",
        cross
    );
}

/// else block is emitted correctly.
/// Uses `have` (always runtime-unknown) so the if/else structure is always preserved.
#[test]
fn if_else_structure() {
    let src = "if have git\nset A found\nelse\nset A absent\nend";
    let b = bash(src);
    assert!(b.contains("else"), "bash: {}", b);
    assert!(b.contains("fi"), "bash: {}", b);
    let f = fish(src);
    assert!(f.contains("else"), "fish: {}", f);
    assert!(f.contains("end"), "fish: {}", f);
}

// ── multi-node realistic source ───────────────────────────────────────────────

/// Reflects the README example: set + os guard + have guards + call.
/// Validates that all emitters produce the key fragments from a real-world input.
/// Os branches are folded at compile time; we only assert the OS-independent parts here
/// and use #[cfg] guards for the OS-specific assertions.
#[test]
fn readme_example() {
    let src = "\
set EDITOR nvim
if os darwin
  set BROWSER open
elif os linux
  set BROWSER xdg-open
end
if have cargo
  path+ $HOME/.cargo/bin
end
if have starship
  call starship init {shell}
end";

    let b = bash(src);
    assert!(b.contains("export EDITOR=\"nvim\""), "EDITOR: {}", b);
    assert!(b.contains("command -v cargo"), "cargo: {}", b);
    assert!(
        b.contains("eval \"$(starship init bash)\""),
        "starship: {}",
        b
    );
    // Os branch: statically folded — assert the surviving branch per build OS
    #[cfg(target_os = "linux")]
    assert!(
        b.contains("export BROWSER=\"xdg-open\""),
        "bash linux BROWSER: {}",
        b
    );
    #[cfg(target_os = "macos")]
    assert!(
        b.contains("export BROWSER=\"open\""),
        "bash macos BROWSER: {}",
        b
    );

    let f = fish(src);
    assert!(f.contains("set -gx EDITOR"), "EDITOR: {}", f);
    assert!(f.contains("type -q cargo"), "cargo: {}", f);
    assert!(f.contains("starship init fish"), "starship: {}", f);
    #[cfg(target_os = "linux")]
    assert!(f.contains("set -gx BROWSER"), "fish linux BROWSER: {}", f);
    #[cfg(target_os = "macos")]
    assert!(f.contains("set -gx BROWSER"), "fish macos BROWSER: {}", f);

    let p = pwsh(src);
    assert!(p.contains("$env:EDITOR = \"nvim\""), "EDITOR: {}", p);
    assert!(p.contains("Get-Command cargo"), "cargo: {}", p);
    assert!(
        p.contains("Invoke-Expression (& starship init powershell)"),
        "starship: {}",
        p
    );
    // Os branch for pwsh: statically folded too
    #[cfg(target_os = "macos")]
    assert!(
        p.contains("$env:BROWSER = \"open\""),
        "pwsh macos BROWSER: {}",
        p
    );
    #[cfg(target_os = "linux")]
    assert!(
        p.contains("$env:BROWSER = \"xdg-open\""),
        "pwsh linux BROWSER: {}",
        p
    );
}

// -- alias keyword ----------------------------------------------------------

/// `alias` emits the correct shell-specific alias syntax for each target.
#[test]
fn alias_all_shells() {
    assert_eq!(bash("alias ll ls -la"), "alias ll='ls -la'");
    assert_eq!(zsh("alias ll ls -la"), "alias ll='ls -la'");
    assert_eq!(fish("alias ll ls -la"), "alias ll ls -la");
    assert_eq!(
        pwsh("alias ll ls -la"),
        "Set-Alias -Scope Global -Name ll -Value ls -la"
    );
}

/// `alias` with a single-word body.
#[test]
fn alias_single_word_body() {
    assert_eq!(bash("alias g git"), "alias g='git'");
    assert_eq!(fish("alias g git"), "alias g git");
    assert_eq!(
        pwsh("alias g git"),
        "Set-Alias -Scope Global -Name g -Value git"
    );
}

/// missing `alias` body is a parse error.
#[test]
fn alias_missing_body_is_error() {
    assert!(Parser::new("alias ll", None).parse().is_err());
    assert!(Parser::new("alias", None).parse().is_err());
}

/// Errors carry the 1-based source line and the offending token.
#[test]
fn parse_errors_carry_line_and_context() {
    let err = Parser::new("set A 1\nsett FOO bar", None)
        .parse()
        .unwrap_err();
    assert_eq!(err.line, 2, "wrong line: {}", err);
    assert!(err.msg.contains("sett"), "wrong msg: {}", err);

    // Missing set value — points to the correct line
    let err = Parser::new("set A 1\nset B 2\nset C", None)
        .parse()
        .unwrap_err();
    assert_eq!(err.line, 3, "wrong line: {}", err);

    // Unterminated if — points to the opening `if` line
    let err = Parser::new("set A 1\nif have git\nset B 2", None)
        .parse()
        .unwrap_err();
    assert_eq!(err.line, 2, "should point to if line: {}", err);
}

// ── compound conditions (not / and / or) ─────────────────────────────────────

/// `if not have` — prefix negation across all shells.
#[test]
fn if_not_have_all_shells() {
    let src = "if not have cargo\nset CARGO_ABSENT 1\nend";

    let b = bash(src);
    assert!(b.contains("! command -v cargo"), "bash: {}", b);
    assert!(b.contains("fi"), "bash: {}", b);

    let f = fish(src);
    assert!(f.contains("not type -q cargo"), "fish: {}", f);
    assert!(f.contains("end"), "fish: {}", f);

    let p = pwsh(src);
    assert!(p.contains("-not (Get-Command cargo"), "pwsh: {}", p);
}

/// `if have X and os Y` — infix `and` across all shells.
/// On Linux: `os linux` folds to AlwaysTrue, so `have cargo AND true` reduces to
/// just the `have cargo` guard (no `&&`, no `Linux` string in output).
/// On macOS: `os linux` folds to AlwaysFalse → entire block dropped.
/// On other / unknown OS: both runtime checks are preserved.
#[test]
#[allow(unused_variables)]
fn if_and_condition_all_shells() {
    let src = "if have cargo and os linux\npath+ $HOME/.cargo/bin\nend";

    let b = bash(src);
    // On Linux: os linux=true, And reduces to Have(cargo) only.
    // On macOS: os linux=false, And is false → block dropped.
    // On other: both kept as And(Have, Os).
    #[cfg(target_os = "linux")]
    {
        assert!(b.contains("command -v cargo"), "bash and lhs: {}", b);
        // os linux was folded out — the if-condition line has no && (the dedup guard
        // inside the body has &&, but the if-line itself should not).
        let if_line = b
            .lines()
            .find(|l| l.trim_start().starts_with("if "))
            .unwrap_or("");
        assert!(
            !if_line.contains("&&"),
            "bash linux: unexpected && in if-line: {}",
            if_line
        );
        assert!(
            !b.contains("Linux"),
            "bash linux: unexpected Linux string: {}",
            b
        );
    }
    #[cfg(target_os = "macos")]
    assert!(
        b.is_empty() || !b.contains("cargo/bin"),
        "bash macos: dropped: {}",
        b
    );
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        assert!(b.contains("command -v cargo"), "bash and lhs: {}", b);
        assert!(b.contains("&&"), "bash and op: {}", b);
        assert!(b.contains("Linux"), "bash and rhs: {}", b);
    }

    let f = fish(src);
    #[cfg(target_os = "linux")]
    {
        assert!(f.contains("type -q cargo"), "fish and lhs: {}", f);
        assert!(
            !f.contains(";  and  "),
            "fish linux: unexpected ;  and : {}",
            f
        );
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        assert!(f.contains("type -q cargo"), "fish and lhs: {}", f);
        assert!(f.contains(";  and  "), "fish and op: {}", f);
        assert!(f.contains("Linux"), "fish and rhs: {}", f);
    }

    let p = pwsh(src);
    #[cfg(target_os = "linux")]
    {
        assert!(p.contains("Get-Command cargo"), "pwsh and lhs: {}", p);
        assert!(!p.contains("-and"), "pwsh linux: unexpected -and: {}", p);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        assert!(p.contains("Get-Command cargo"), "pwsh and lhs: {}", p);
        assert!(p.contains("-and"), "pwsh and op: {}", p);
        assert!(p.contains("$IsLinux"), "pwsh and rhs: {}", p);
    }
}

/// `if or` — infix `or` across all shells.
/// `os darwin or os linux` on Linux: darwin=false, linux=true → Or(false,true)=AlwaysTrue → body inlined.
/// On macOS: darwin=true → Or(true,_)=AlwaysTrue → body inlined.
/// On other: both runtime checks kept.
#[test]
#[allow(unused_variables)]
fn if_or_condition_all_shells() {
    let src = "if os darwin or os linux\nset POSIX 1\nend";

    let b = bash(src);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        // Or folds to AlwaysTrue → body inlined
        assert!(b.contains("export POSIX=\"1\""), "bash unix: {}", b);
        assert!(!b.contains("if "), "bash unix: unexpected if: {}", b);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        assert!(b.contains("Darwin"), "bash or lhs: {}", b);
        assert!(b.contains("||"), "bash or op: {}", b);
        assert!(b.contains("Linux"), "bash or rhs: {}", b);
    }

    let f = fish(src);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        assert!(f.contains("set -gx POSIX"), "fish unix: {}", f);
        assert!(!f.contains("if "), "fish unix: unexpected if: {}", f);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        assert!(f.contains("Darwin"), "fish or lhs: {}", f);
        assert!(f.contains(";  or "), "fish or op: {}", f);
        assert!(f.contains("Linux"), "fish or rhs: {}", f);
    }

    let p = pwsh(src);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        assert!(p.contains("$env:POSIX = \"1\""), "pwsh unix: {}", p);
        assert!(!p.contains("if ("), "pwsh unix: unexpected if: {}", p);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        assert!(p.contains("$IsMacOS"), "pwsh or lhs: {}", p);
        assert!(p.contains("-or"), "pwsh or op: {}", p);
        assert!(p.contains("$IsLinux"), "pwsh or rhs: {}", p);
    }
}

// -- semantic pruning (shell-condition folding) ----------------------------------

/// Comprehensive single-shell pruning integration test (bash).
///
/// Covers in one source blob: self-shell inline, foreign-shell drop,
/// dead-head+else, dead-head+matching-elif, not-fold, and+true, and+false,
/// or+true, and unknown guard kept as-is.
#[test]
fn prune_comprehensive_bash() {
    let src = "\
if shell bash
  set NATIVE 1
end
if shell fish
  set FISH_ONLY 1
end
if shell fish
  set A fish
else
  set A other
end
if shell fish
  set B fish
elif shell bash
  set B bash
end
if not shell fish
  set C 1
end
if shell bash and have cargo
  set D 1
end
if shell fish and have cargo
  set E 1
end
if shell bash or have cargo
  set F 1
end
if have git
  set G 1
end";
    let b = bash(src);

    assert!(
        b.contains("export NATIVE=\""),
        "(1) self-shell not inlined: {}",
        b
    );
    assert!(
        !b.contains("FISH_ONLY"),
        "(2) dead fish block leaked: {}",
        b
    );
    assert!(
        b.contains("export A=\"other\""),
        "(3) else not inlined: {}",
        b
    );
    assert!(
        b.contains("export B=\"bash\""),
        "(4) matching elif not inlined: {}",
        b
    );
    assert!(
        b.contains("export C=\""),
        "(5) not-fold: body not inlined: {}",
        b
    );
    assert!(
        b.contains("command -v cargo"),
        "(6) and+true: have-guard missing: {}",
        b
    );
    assert!(
        b.contains("export D=\""),
        "(6) and+true: D body missing: {}",
        b
    );
    assert!(
        !b.contains("export E"),
        "(7) and+false: dead block leaked: {}",
        b
    );
    assert!(
        b.contains("export F=\""),
        "(8) or+true: body not inlined: {}",
        b
    );
    assert!(
        b.contains("command -v git"),
        "(9) unknown have-guard removed: {}",
        b
    );
    assert!(
        b.contains("export G=\""),
        "(9) unknown: G body missing: {}",
        b
    );
}

/// Multi-elif chain compiled for every shell:
/// fish/zsh/bash heads fold to the matching shell's branch;
/// for pwsh all three fold to false; then `os linux` is folded statically:
///   on Linux → body inlined; on macOS/Windows → else falls through.
#[test]
fn prune_multi_elif_chain_all_shells() {
    let src = "\
if shell fish
  set S fish
elif shell zsh
  set S zsh
elif shell bash
  set S bash
elif os linux
  set S linux
else
  set S other
end";

    let b = bash(src);
    assert!(b.contains("export S=\"bash\""), "bash branch: {}", b);
    assert!(!b.contains("if "), "bash: unexpected if-wrapper: {}", b);

    let z = zsh(src);
    assert!(z.contains("export S=\"zsh\""), "zsh branch: {}", z);
    assert!(!z.contains("if "), "zsh: unexpected if-wrapper: {}", z);

    let f = fish(src);
    assert!(f.contains("set -gx S \"fish\""), "fish branch: {}", f);
    assert!(!f.contains("if "), "fish: unexpected if-wrapper: {}", f);

    // pwsh: shell branches all dead; then os linux is folded at compile time.
    let p = pwsh(src);
    #[cfg(target_os = "linux")]
    {
        // os linux → AlwaysTrue → linux body inlined, no if wrapper
        assert!(
            p.contains("$env:S = \"linux\""),
            "pwsh linux: linux body missing: {}",
            p
        );
        assert!(
            !p.contains("if ("),
            "pwsh linux: unexpected if wrapper: {}",
            p
        );
    }
    #[cfg(target_os = "macos")]
    {
        // os linux → AlwaysFalse → else inlined, no if wrapper
        assert!(
            p.contains("$env:S = \"other\""),
            "pwsh macos: else missing: {}",
            p
        );
        assert!(
            !p.contains("if ("),
            "pwsh macos: unexpected if wrapper: {}",
            p
        );
    }
    #[cfg(target_os = "windows")]
    {
        // os linux → AlwaysFalse → else inlined
        assert!(
            p.contains("$env:S = \"other\""),
            "pwsh windows: else missing: {}",
            p
        );
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        // os linux unknown → kept as runtime check, else preserved
        assert!(p.contains("$IsLinux"), "pwsh: os guard missing: {}", p);
        assert!(
            p.contains("$env:S = \"linux\""),
            "pwsh: linux body missing: {}",
            p
        );
        assert!(
            p.contains("$env:S = \"other\""),
            "pwsh: else preserved: {}",
            p
        );
    }
}

// ── strip_quotes / Cond::Env / path-dedup integration ─────────────────────────

/// Quoted path arguments are stripped of surrounding quotes at parse time,
/// so the emitter never double-quotes them.
#[test]
fn quoted_path_is_stripped() {
    // Double-quoted dir: emitter should see the bare path, not \"dir\".
    let b = bash("path+ \"$HOME/.cargo/bin\"");
    assert!(!b.contains("\\\"$HOME"), "bash double-quote leaked: {}", b);
    assert!(b.contains("$HOME/.cargo/bin"), "bash path missing: {}", b);

    let f = fish("path+ \"$HOME/.cargo/bin\"");
    assert!(!f.contains("\\\"$HOME"), "fish double-quote leaked: {}", f);
    assert!(f.contains("$HOME/.cargo/bin"), "fish path missing: {}", f);
}

/// `if env VAR` emits the correct check in each shell and is not statically folded.
#[test]
fn if_env_all_shells() {
    let src = "if env CARGO_HOME\npath+ $CARGO_HOME/bin\nend";

    let b = bash(src);
    assert!(b.contains("CARGO_HOME"), "bash env cond: {}", b);
    assert!(b.contains("-n \""), "bash -n check: {}", b);
    assert!(b.contains("if "), "bash if wrapper: {}", b);

    let f = fish(src);
    assert!(f.contains("set -q CARGO_HOME"), "fish env cond: {}", f);
    assert!(f.contains("if "), "fish if wrapper: {}", f);

    let p = pwsh(src);
    assert!(
        p.contains("Test-Path env:CARGO_HOME"),
        "pwsh env cond: {}",
        p
    );
    assert!(p.contains("if ("), "pwsh if wrapper: {}", p);
}

/// `path+` in bash/pwsh emits a deduplication guard so re-sourcing the config
/// does not accumulate duplicate entries in PATH.
#[test]
fn path_dedup_guard_present() {
    let b = bash("path+ /usr/local/bin");
    // bash guard: [[ "${PATH}" != *\"/usr/local/bin\"* ]] && export …
    assert!(b.contains("[[ "), "bash guard missing: {}", b);
    assert!(b.contains("/usr/local/bin"), "bash dir missing: {}", b);
    // Should not appear twice
    assert_eq!(
        b.matches("/usr/local/bin").count(),
        2,
        "bash dir count: {}",
        b
    ); // once in guard, once in add

    // fish_add_path already deduplicates — no extra wrapper.
    let f = fish("path+ /usr/local/bin");
    assert!(!f.contains("[[ "), "fish: unexpected guard: {}", f);

    let p = pwsh("path+ C:/tools");
    assert!(p.contains("-notcontains"), "pwsh guard missing: {}", p);
    assert!(
        p.contains("-split ';'"),
        "pwsh component guard missing: {}",
        p
    );
    assert!(p.contains("C:/tools"), "pwsh dir missing: {}", p);
}
