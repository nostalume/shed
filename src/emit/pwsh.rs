use super::{Emitter, escape_double_quoted, escape_path};
use crate::ast::{Cond, IfNode, Node, PathDir};

pub struct PwshEmitter;

impl Emitter for PwshEmitter {
    #[inline]
    /// Returns "pwsh" — the shed DSL identifier used in routing (main.rs)
    /// and Cond::Shell comparisons. {shell} substitution in call args is
    /// handled by the overridden resolve_call_args below, which maps to
    /// "powershell" (the actual init-command name).
    fn name(&self) -> &str {
        "pwsh"
    }

    /// Override: replace `{shell}` with "powershell" rather than "pwsh",
    /// because tools like starship expect `starship init powershell`.
    #[inline]
    fn resolve_call_args(&self, args: &str) -> String {
        args.replace("{shell}", "powershell").trim().to_owned()
    }

    fn emit_nodes(&self, nodes: &[Node], d: usize) -> Vec<String> {
        let mut out = Vec::with_capacity(nodes.len());
        for n in nodes {
            self.node(n, d, &mut out);
        }
        out
    }
}

impl PwshEmitter {
    fn node(&self, n: &Node, d: usize, out: &mut Vec<String>) {
        match n {
            Node::Set { key, val } => {
                out.push(self.indent(
                    format!("$env:{} = \"{}\"", key, escape_double_quoted(val, "pwsh")),
                    d,
                ));
            }

            Node::Path { dir, direction } => {
                // Build the guard directly — no intermediate binding needed.
                let add = match direction {
                    PathDir::Prepend => {
                        format!("$env:PATH = \"{};$env:PATH\"", escape_path(dir, "pwsh"))
                    }
                    PathDir::Append => {
                        format!("$env:PATH = \"$env:PATH;{}\"", escape_path(dir, "pwsh"))
                    }
                };
                let needle = escape_path(dir, "pwsh");
                let guard =
                    format!("if (($env:PATH -split ';') -notcontains \"{needle}\") {{ {add} }}");
                out.push(self.indent(guard, d));
            }

            Node::Call { cmd, args } => {
                // Use format_call from the trait; {shell} → "powershell" via resolve_call_args.
                let s = self.format_call(cmd, args, "Invoke-Expression (& ", ")");
                out.push(self.indent(s, d));
            }

            Node::Alias { name, body } => {
                // -Name and -Value are explicit named parameters (avoids positional
                // ambiguity with multi-word bodies).
                // -Scope Global ensures the alias survives past the dot-sourced
                // script frame and is visible in the user's interactive session.
                out.push(self.indent(
                    format!("Set-Alias -Scope Global -Name {} -Value {}", name, body),
                    d,
                ));
            }

            Node::If(node) => self.emit_if(node, d, out),
        }
    }

    fn cond(&self, c: &Cond) -> String {
        match c {
            Cond::Have(cmd) => format!("Get-Command {} -ErrorAction SilentlyContinue", cmd),
            Cond::Exists(path) => format!("Test-Path \"{}\"", escape_path(path, "pwsh")),
            Cond::Env(var) => format!("(Test-Path env:{})", var),
            Cond::Os(name) => match name.as_str() {
                "darwin" => "$IsMacOS".into(),
                "linux" => "$IsLinux".into(),
                "windows" => "$IsWindows".into(),
                other => format!("$false # unknown os: {}", other),
            },
            // Shell name is compared to "pwsh"; resolve_call_args uses "powershell" for
            // {shell} substitution, but the shed DSL uses "pwsh" as the shell identifier.
            Cond::Shell(name) => if name == "pwsh" { "$true" } else { "$false" }.into(),
            Cond::Not(inner) => {
                let inner_s = self.cond(inner);
                let mut s = String::with_capacity(8 + inner_s.len() + 2);
                s.push_str("(-not (");
                s.push_str(&inner_s);
                s.push_str("))");
                s
            }
            Cond::And(lhs, rhs) => {
                let l = self.cond(lhs);
                let r = self.cond(rhs);
                let mut s = String::with_capacity(1 + l.len() + 8 + r.len() + 1);
                s.push('(');
                s.push_str(&l);
                s.push_str(") -and (");
                s.push_str(&r);
                s.push(')');
                s
            }
            Cond::Or(lhs, rhs) => {
                let l = self.cond(lhs);
                let r = self.cond(rhs);
                let mut s = String::with_capacity(1 + l.len() + 7 + r.len() + 1);
                s.push('(');
                s.push_str(&l);
                s.push_str(") -or (");
                s.push_str(&r);
                s.push(')');
                s
            }
        }
    }

    fn emit_if(&self, n: &IfNode, d: usize, out: &mut Vec<String>) {
        out.reserve(2 + n.body.len() + n.elifs.len() * 2 + n.else_.len());
        out.push(self.indent(format!("if ({}) {{", self.cond(&n.cond)), d));
        for node in &n.body {
            self.node(node, d + 1, out);
        }
        for (c, b) in &n.elifs {
            out.push(self.indent(format!("}} elseif ({}) {{", self.cond(c)), d));
            for node in b {
                self.node(node, d + 1, out);
            }
        }
        if !n.else_.is_empty() {
            out.push(self.indent("} else {".into(), d));
            for node in &n.else_ {
                self.node(node, d + 1, out);
            }
        }
        out.push(self.indent("}".into(), d));
    }
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Cond, IfNode, Node};

    fn render(nodes: &[Node]) -> String {
        PwshEmitter.render(nodes)
    }

    // ── Node::Set ─────────────────────────────────────────────────────────────

    #[test]
    fn set_basic() {
        assert_eq!(
            render(&[Node::Set {
                key: "EDITOR".into(),
                val: "nvim".into()
            }]),
            "$env:EDITOR = \"nvim\""
        );
    }

    /// Parser strips outer quotes; emitter must wrap exactly once — never double-wrap.
    #[test]
    fn set_no_double_wrap() {
        let out = render(&[Node::Set {
            key: "FOO".into(),
            val: "bar".into(),
        }]);
        assert!(!out.contains("\"\""), "double-quote in output: {}", out);
        assert_eq!(out, "$env:FOO = \"bar\"");
    }

    // ── Node::Path ────────────────────────────────────────────────────────────

    #[test]
    fn path_prepend() {
        let out = render(&[Node::Path {
            dir: "C:/tools".into(),
            direction: PathDir::Prepend,
        }]);
        assert!(
            out.contains("$env:PATH = \"C:/tools;$env:PATH\""),
            "add: {}",
            out
        );
        assert!(out.contains("-notcontains"), "guard: {}", out);
    }

    #[test]
    fn path_append() {
        let out = render(&[Node::Path {
            dir: "C:/opt".into(),
            direction: PathDir::Append,
        }]);
        assert!(
            out.contains("$env:PATH = \"$env:PATH;C:/opt\""),
            "add: {}",
            out
        );
        assert!(out.contains("-notcontains"), "guard: {}", out);
    }

    /// resolve_path normalises separators; forward slashes must survive into emitted output.
    #[test]
    fn path_forward_slash_passed_through() {
        let out = render(&[Node::Path {
            dir: "/usr/local/bin".into(),
            direction: PathDir::Prepend,
        }]);
        assert!(out.contains("/usr/local/bin"), "dir missing: {}", out);
    }

    // ── Node::Call ────────────────────────────────────────────────────────────

    #[test]
    fn call_no_args() {
        assert_eq!(
            render(&[Node::Call {
                cmd: "myprog".into(),
                args: String::new()
            }]),
            "Invoke-Expression (& myprog)"
        );
    }

    /// {shell} in args must be replaced with "powershell" (PwshEmitter::name()).
    #[test]
    fn call_with_shell_placeholder() {
        assert_eq!(
            render(&[Node::Call {
                cmd: "starship".into(),
                args: "init {shell}".into()
            }]),
            "Invoke-Expression (& starship init powershell)"
        );
    }

    #[test]
    fn call_with_args_no_placeholder() {
        assert_eq!(
            render(&[Node::Call {
                cmd: "zoxide".into(),
                args: "init nushell".into()
            }]),
            "Invoke-Expression (& zoxide init nushell)"
        );
    }

    // ── Node::Alias ───────────────────────────────────────────────────────────

    /// Set-Alias without -Scope Global is restricted to the script's scope and
    /// vanishes when the dot-sourced frame exits.
    #[test]
    fn alias_uses_scope_global() {
        let out = render(&[Node::Alias {
            name: "ll".into(),
            body: "ls -la".into(),
        }]);
        assert!(
            out.contains("-Scope Global"),
            "missing -Scope Global: {}",
            out
        );
        assert!(out.contains("-Name ll"), "missing -Name: {}", out);
        assert!(out.contains("-Value ls -la"), "missing -Value: {}", out);
        assert_eq!(out, "Set-Alias -Scope Global -Name ll -Value ls -la");
    }

    /// Multi-word body must land in -Value without ambiguity.
    #[test]
    fn alias_named_params_multiword_body() {
        assert_eq!(
            render(&[Node::Alias {
                name: "gs".into(),
                body: "git status".into()
            }]),
            "Set-Alias -Scope Global -Name gs -Value git status"
        );
    }

    /// Single-word body also uses -Name / -Value.
    #[test]
    fn alias_exact_output_single_word() {
        assert_eq!(
            render(&[Node::Alias {
                name: "np".into(),
                body: "notepad".into()
            }]),
            "Set-Alias -Scope Global -Name np -Value notepad"
        );
    }

    // ── conditions ────────────────────────────────────────────────────────────

    #[test]
    fn cond_have() {
        assert_eq!(
            PwshEmitter.cond(&Cond::Have("git".into())),
            "Get-Command git -ErrorAction SilentlyContinue"
        );
    }

    #[test]
    fn cond_exists() {
        assert_eq!(
            PwshEmitter.cond(&Cond::Exists("C:/Users/user/.cargo/bin".into())),
            "Test-Path \"C:/Users/user/.cargo/bin\""
        );
    }

    #[test]
    fn cond_env() {
        assert_eq!(
            PwshEmitter.cond(&Cond::Env("CARGO_HOME".into())),
            "(Test-Path env:CARGO_HOME)"
        );
    }

    #[test]
    fn cond_os_darwin() {
        assert_eq!(PwshEmitter.cond(&Cond::Os("darwin".into())), "$IsMacOS");
    }
    #[test]
    fn cond_os_linux() {
        assert_eq!(PwshEmitter.cond(&Cond::Os("linux".into())), "$IsLinux");
    }
    #[test]
    fn cond_os_windows() {
        assert_eq!(PwshEmitter.cond(&Cond::Os("windows".into())), "$IsWindows");
    }

    #[test]
    fn cond_shell_pwsh_is_true() {
        assert_eq!(PwshEmitter.cond(&Cond::Shell("pwsh".into())), "$true");
    }

    #[test]
    fn cond_shell_other_is_false() {
        assert_eq!(PwshEmitter.cond(&Cond::Shell("bash".into())), "$false");
    }

    #[test]
    fn cond_not() {
        assert_eq!(
            PwshEmitter.cond(&Cond::Not(Box::new(Cond::Have("git".into())))),
            "(-not (Get-Command git -ErrorAction SilentlyContinue))"
        );
    }

    #[test]
    fn cond_and() {
        assert_eq!(
            PwshEmitter.cond(&Cond::And(
                Box::new(Cond::Have("cargo".into())),
                Box::new(Cond::Os("linux".into())),
            )),
            "(Get-Command cargo -ErrorAction SilentlyContinue) -and ($IsLinux)"
        );
    }

    #[test]
    fn cond_or() {
        assert_eq!(
            PwshEmitter.cond(&Cond::Or(
                Box::new(Cond::Os("darwin".into())),
                Box::new(Cond::Os("linux".into())),
            )),
            "($IsMacOS) -or ($IsLinux)"
        );
    }

    // ── if / elseif / else ────────────────────────────────────────────────────

    #[test]
    fn if_braces() {
        let node = Node::If(IfNode {
            cond: Cond::Have("git".into()),
            body: vec![Node::Set {
                key: "X".into(),
                val: "1".into(),
            }],
            elifs: vec![],
            else_: vec![],
        });
        let out = render(&[node]);
        assert!(out.contains("if (Get-Command git"), "missing if: {}", out);
        assert!(out.contains("$env:X = \"1\""), "missing body: {}", out);
        assert!(out.ends_with('}'), "missing close brace: {}", out);
    }

    #[test]
    fn if_elif_else_braces() {
        let node = Node::If(IfNode {
            cond: Cond::Os("darwin".into()),
            body: vec![Node::Set {
                key: "A".into(),
                val: "1".into(),
            }],
            elifs: vec![(
                Cond::Os("linux".into()),
                vec![Node::Set {
                    key: "A".into(),
                    val: "2".into(),
                }],
            )],
            else_: vec![Node::Set {
                key: "A".into(),
                val: "3".into(),
            }],
        });
        let out = render(&[node]);
        assert!(out.contains("elseif"), "missing elseif: {}", out);
        assert!(out.contains("} else {"), "missing else: {}", out);
    }

    #[test]
    fn indent_depth() {
        let node = Node::If(IfNode {
            cond: Cond::Have("cargo".into()),
            body: vec![Node::Set {
                key: "Y".into(),
                val: "z".into(),
            }],
            elifs: vec![],
            else_: vec![],
        });
        let out = render(&[node]);
        assert!(
            out.lines().any(|l| l.starts_with("  $env:")),
            "body not indented: {}",
            out
        );
    }
}
