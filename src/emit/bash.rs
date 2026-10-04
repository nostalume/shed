use super::{Emitter, escape_double_quoted, escape_path};
use crate::ast::{Cond, IfNode, Node, PathDir};

/// Emits POSIX-compatible sh/bash/zsh.
/// `shell_name` is `"bash"` or `"zsh"` — `&'static str` means zero allocation.
pub struct BashEmitter {
    pub shell_name: &'static str,
}

impl BashEmitter {
    #[inline]
    pub fn new(name: &'static str) -> Self {
        Self { shell_name: name }
    }
}

impl Emitter for BashEmitter {
    #[inline]
    fn name(&self) -> &str {
        self.shell_name
    }

    fn emit_nodes(&self, nodes: &[Node], d: usize) -> Vec<String> {
        let mut out = Vec::with_capacity(nodes.len());
        for n in nodes {
            self.node(n, d, &mut out);
        }
        out
    }
}

/// Map the shed OS name to the `uname -s` output string.
/// Pure branchless lookup — no allocation, always inlined.
#[inline]
pub fn os_uname_name(name: &str) -> &str {
    match name {
        "darwin" => "Darwin",
        "linux" => "Linux",
        "windows" => "Windows_NT",
        other => other,
    }
}

impl BashEmitter {
    fn node(&self, n: &Node, d: usize, out: &mut Vec<String>) {
        match n {
            Node::Set { key, val } => {
                out.push(self.indent(
                    format!("export {}=\"{}\"", key, escape_double_quoted(val, "bash")),
                    d,
                ));
            }

            Node::Path { dir, direction } => {
                // Build the guard directly — no intermediate `add` binding needed.
                let rendered = escape_path(dir, "bash");
                let guard = match direction {
                    PathDir::Prepend => format!(
                        "[[ \":${{PATH}}:\" != *\":{}:\"* ]] && export PATH=\"{}:$PATH\"",
                        rendered.as_str(),
                        rendered.as_str()
                    ),
                    PathDir::Append => format!(
                        "[[ \":${{PATH}}:\" != *\":{}:\"* ]] && export PATH=\"$PATH:{}\"",
                        rendered.as_str(),
                        rendered.as_str()
                    ),
                };
                out.push(self.indent(guard, d));
            }

            Node::Call { cmd, args } => {
                let s = self.format_call(cmd, args, "eval \"$(", ")\"");
                out.push(self.indent(s, d));
            }

            Node::Alias { name, body } => {
                out.push(self.indent(format!("alias {}='{}'", name, body), d));
            }

            Node::If(node) => self.emit_if(node, d, out),
        }
    }

    /// Build the condition string.
    ///
    /// Compound nodes (`And`/`Or`) use exact-capacity `String::with_capacity`
    /// to avoid a reallocation when concatenating two already-built sub-strings.
    fn cond(&self, c: &Cond) -> String {
        match c {
            Cond::Have(cmd) => format!("command -v {} >/dev/null 2>&1", cmd),
            Cond::Exists(path) => format!("[ -e \"{}\" ]", escape_path(path, "bash")),
            Cond::Env(var) => format!("[ -n \"${{{var}:-}}\" ]"),
            Cond::Os(name) => format!("[ \"$(uname -s)\" = \"{}\" ]", os_uname_name(name)),
            Cond::Shell(name) => match name.as_str() {
                "bash" => "[ -n \"$BASH_VERSION\" ]".into(),
                "zsh" => "[ -n \"$ZSH_VERSION\" ]".into(),
                _ => "false".into(),
            },
            Cond::Not(inner) => {
                let mut s = String::from("! ");
                s.push_str(&self.cond(inner));
                s
            }
            Cond::And(lhs, rhs) => {
                let l = self.cond(lhs);
                let r = self.cond(rhs);
                let mut s = String::with_capacity(l.len() + 4 + r.len());
                s.push_str(&l);
                s.push_str(" && ");
                s.push_str(&r);
                s
            }
            Cond::Or(lhs, rhs) => {
                let l = self.cond(lhs);
                let r = self.cond(rhs);
                let mut s = String::with_capacity(l.len() + 4 + r.len());
                s.push_str(&l);
                s.push_str(" || ");
                s.push_str(&r);
                s
            }
        }
    }

    fn emit_if(&self, n: &IfNode, d: usize, out: &mut Vec<String>) {
        out.reserve(2 + n.body.len() + n.elifs.len() * 2 + n.else_.len());
        out.push(self.indent(format!("if {}; then", self.cond(&n.cond)), d));
        for node in &n.body {
            self.node(node, d + 1, out);
        }
        for (c, b) in &n.elifs {
            out.push(self.indent(format!("elif {}; then", self.cond(c)), d));
            for node in b {
                self.node(node, d + 1, out);
            }
        }
        if !n.else_.is_empty() {
            out.push(self.indent("else".into(), d));
            for node in &n.else_ {
                self.node(node, d + 1, out);
            }
        }
        out.push(self.indent("fi".into(), d));
    }
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Cond, IfNode, Node};

    fn bash() -> BashEmitter {
        BashEmitter::new("bash")
    }
    fn zsh() -> BashEmitter {
        BashEmitter::new("zsh")
    }

    fn render(e: &BashEmitter, nodes: &[Node]) -> String {
        e.render(nodes)
    }

    // ── Node::Set ─────────────────────────────────────────────────────────────

    #[test]
    fn set() {
        assert_eq!(
            render(
                &bash(),
                &[Node::Set {
                    key: "EDITOR".into(),
                    val: "nvim".into()
                }]
            ),
            "export EDITOR=\"nvim\""
        );
    }

    // ── Node::Path ────────────────────────────────────────────────────────────

    #[test]
    fn path_prepend() {
        let out = render(
            &bash(),
            &[Node::Path {
                dir: "/usr/local/bin".into(),
                direction: PathDir::Prepend,
            }],
        );
        assert!(
            out.contains("export PATH=\"/usr/local/bin:$PATH\""),
            "add: {}",
            out
        );
        assert!(out.contains("[[ "), "guard: {}", out);
    }

    #[test]
    fn path_append() {
        let out = render(
            &bash(),
            &[Node::Path {
                dir: "/opt/bin".into(),
                direction: PathDir::Append,
            }],
        );
        assert!(
            out.contains("export PATH=\"$PATH:/opt/bin\""),
            "add: {}",
            out
        );
        assert!(out.contains("[[ "), "guard: {}", out);
    }

    // ── Node::Call ────────────────────────────────────────────────────────────

    #[test]
    fn call_with_shell_placeholder() {
        assert_eq!(
            render(
                &bash(),
                &[Node::Call {
                    cmd: "starship".into(),
                    args: "init {shell}".into()
                }]
            ),
            "eval \"$(starship init bash)\""
        );
    }

    #[test]
    fn call_shell_placeholder_zsh() {
        assert_eq!(
            render(
                &zsh(),
                &[Node::Call {
                    cmd: "starship".into(),
                    args: "init {shell}".into()
                }]
            ),
            "eval \"$(starship init zsh)\""
        );
    }

    #[test]
    fn call_no_args() {
        assert_eq!(
            render(
                &bash(),
                &[Node::Call {
                    cmd: "myprog".into(),
                    args: String::new()
                }]
            ),
            "eval \"$(myprog)\""
        );
    }

    // ── Node::Alias ───────────────────────────────────────────────────────────

    #[test]
    fn alias_single_quoted() {
        assert_eq!(
            render(
                &bash(),
                &[Node::Alias {
                    name: "ll".into(),
                    body: "ls -la".into()
                }]
            ),
            "alias ll='ls -la'"
        );
    }

    // ── conditions ────────────────────────────────────────────────────────────

    #[test]
    fn cond_have() {
        assert_eq!(
            bash().cond(&Cond::Have("git".into())),
            "command -v git >/dev/null 2>&1"
        );
    }

    #[test]
    fn cond_exists() {
        assert_eq!(
            bash().cond(&Cond::Exists("/home/user/.cargo/bin".into())),
            "[ -e \"/home/user/.cargo/bin\" ]"
        );
    }

    #[test]
    fn cond_env() {
        assert_eq!(
            bash().cond(&Cond::Env("CARGO_HOME".into())),
            "[ -n \"${CARGO_HOME:-}\" ]"
        );
    }

    #[test]
    fn cond_os_darwin() {
        assert_eq!(
            bash().cond(&Cond::Os("darwin".into())),
            "[ \"$(uname -s)\" = \"Darwin\" ]"
        );
    }

    #[test]
    fn cond_os_linux() {
        assert_eq!(
            bash().cond(&Cond::Os("linux".into())),
            "[ \"$(uname -s)\" = \"Linux\" ]"
        );
    }

    #[test]
    fn cond_os_windows() {
        assert_eq!(
            bash().cond(&Cond::Os("windows".into())),
            "[ \"$(uname -s)\" = \"Windows_NT\" ]"
        );
    }

    #[test]
    fn cond_shell_bash() {
        assert_eq!(
            bash().cond(&Cond::Shell("bash".into())),
            "[ -n \"$BASH_VERSION\" ]"
        );
    }

    #[test]
    fn cond_shell_zsh() {
        assert_eq!(
            bash().cond(&Cond::Shell("zsh".into())),
            "[ -n \"$ZSH_VERSION\" ]"
        );
    }

    #[test]
    fn cond_shell_other_is_false() {
        assert_eq!(bash().cond(&Cond::Shell("fish".into())), "false");
    }

    #[test]
    fn cond_not() {
        assert_eq!(
            bash().cond(&Cond::Not(Box::new(Cond::Have("git".into())))),
            "! command -v git >/dev/null 2>&1"
        );
    }

    #[test]
    fn cond_and() {
        assert_eq!(
            bash().cond(&Cond::And(
                Box::new(Cond::Have("cargo".into())),
                Box::new(Cond::Os("linux".into())),
            )),
            "command -v cargo >/dev/null 2>&1 && [ \"$(uname -s)\" = \"Linux\" ]"
        );
    }

    #[test]
    fn cond_or() {
        assert_eq!(
            bash().cond(&Cond::Or(
                Box::new(Cond::Os("darwin".into())),
                Box::new(Cond::Os("linux".into())),
            )),
            "[ \"$(uname -s)\" = \"Darwin\" ] || [ \"$(uname -s)\" = \"Linux\" ]"
        );
    }

    // ── if / elif / else / fi ─────────────────────────────────────────────────

    #[test]
    fn if_then_fi() {
        let node = Node::If(IfNode {
            cond: Cond::Have("git".into()),
            body: vec![Node::Set {
                key: "X".into(),
                val: "1".into(),
            }],
            elifs: vec![],
            else_: vec![],
        });
        let out = render(&bash(), &[node]);
        assert!(out.contains("if command -v git"), "missing if: {}", out);
        assert!(out.contains("export X=\"1\""), "missing body: {}", out);
        assert!(out.contains("fi"), "missing fi: {}", out);
    }

    #[test]
    fn if_elif_else_fi() {
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
        let out = render(&bash(), &[node]);
        assert!(out.contains("elif"), "missing elif: {}", out);
        assert!(out.contains("else"), "missing else: {}", out);
        assert!(out.contains("fi"), "missing fi: {}", out);
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
        let out = render(&bash(), &[node]);
        assert!(
            out.lines().any(|l| l.starts_with("  export")),
            "body not indented: {}",
            out
        );
    }
}
