use super::{Emitter, escape_double_quoted, escape_path};
use crate::ast::{Cond, IfNode, Node, PathDir};
use crate::emit::bash::os_uname_name;

pub struct FishEmitter;

impl Emitter for FishEmitter {
    #[inline]
    fn name(&self) -> &str {
        "fish"
    }

    fn emit_nodes(&self, nodes: &[Node], d: usize) -> Vec<String> {
        let mut out = Vec::with_capacity(nodes.len());
        for n in nodes {
            self.node(n, d, &mut out);
        }
        out
    }
}

impl FishEmitter {
    fn node(&self, n: &Node, d: usize, out: &mut Vec<String>) {
        match n {
            Node::Set { key, val } => {
                out.push(self.indent(
                    format!("set -gx {} \"{}\"", key, escape_double_quoted(val, "fish")),
                    d,
                ));
            }

            // `fish_add_path` deduplicates automatically — no double-PATH problem.
            Node::Path { dir, direction } => {
                let flag = match direction {
                    PathDir::Prepend => "-gP",
                    PathDir::Append => "-gaP",
                };
                out.push(self.indent(
                    format!("fish_add_path {} \"{}\"", flag, escape_path(dir, "fish")),
                    d,
                ));
            }

            Node::Call { cmd, args } => {
                let s = self.format_call(cmd, args, "", " | source");
                out.push(self.indent(s, d));
            }

            Node::Alias { name, body } => {
                out.push(self.indent(format!("alias {} {}", name, body), d));
            }

            Node::If(node) => self.emit_if(node, d, out),
        }
    }

    fn cond(&self, c: &Cond) -> String {
        match c {
            Cond::Have(cmd) => format!("type -q {}", cmd),
            Cond::Exists(path) => format!("test -e \"{}\"", escape_path(path, "fish")),
            Cond::Env(var) => format!("set -q {}", var),
            Cond::Os(name) => format!("test (uname -s) = \"{}\"", os_uname_name(name)),
            Cond::Shell(name) => {
                if name == "fish" {
                    "true".into()
                } else {
                    "false".into()
                }
            }
            Cond::Not(inner) => {
                let mut s = String::from("not ");
                s.push_str(&self.cond(inner));
                s
            }
            Cond::And(lhs, rhs) => {
                let l = self.cond(lhs);
                let r = self.cond(rhs);
                // fish uses `; and` separator — exact-capacity concat.
                let mut s = String::with_capacity(l.len() + 6 + r.len());
                s.push_str(&l);
                s.push_str(";  and ");
                s.push_str(&r);
                s
            }
            Cond::Or(lhs, rhs) => {
                let l = self.cond(lhs);
                let r = self.cond(rhs);
                let mut s = String::with_capacity(l.len() + 5 + r.len());
                s.push_str(&l);
                s.push_str(";  or ");
                s.push_str(&r);
                s
            }
        }
    }

    fn emit_if(&self, n: &IfNode, d: usize, out: &mut Vec<String>) {
        out.reserve(2 + n.body.len() + n.elifs.len() * 2 + n.else_.len());
        out.push(self.indent(format!("if {}", self.cond(&n.cond)), d));
        for node in &n.body {
            self.node(node, d + 1, out);
        }
        for (c, b) in &n.elifs {
            out.push(self.indent(format!("else if {}", self.cond(c)), d));
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
        out.push(self.indent("end".into(), d));
    }
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Cond, IfNode, Node};

    fn render(nodes: &[Node]) -> String {
        FishEmitter.render(nodes)
    }

    // ── Node::Set ─────────────────────────────────────────────────────────────

    #[test]
    fn set() {
        assert_eq!(
            render(&[Node::Set {
                key: "EDITOR".into(),
                val: "nvim".into()
            }]),
            "set -gx EDITOR \"nvim\""
        );
    }

    // ── Node::Path ────────────────────────────────────────────────────────────

    #[test]
    fn path_prepend() {
        assert_eq!(
            render(&[Node::Path {
                dir: "/usr/local/bin".into(),
                direction: PathDir::Prepend
            }]),
            "fish_add_path -gP \"/usr/local/bin\""
        );
    }

    #[test]
    fn path_append() {
        assert_eq!(
            render(&[Node::Path {
                dir: "/opt/bin".into(),
                direction: PathDir::Append
            }]),
            "fish_add_path -gaP \"/opt/bin\""
        );
    }

    // ── Node::Call ────────────────────────────────────────────────────────────

    #[test]
    fn call_with_shell_placeholder() {
        assert_eq!(
            render(&[Node::Call {
                cmd: "starship".into(),
                args: "init {shell}".into()
            }]),
            "starship init fish | source"
        );
    }

    #[test]
    fn call_no_args() {
        assert_eq!(
            render(&[Node::Call {
                cmd: "myprog".into(),
                args: String::new()
            }]),
            "myprog | source"
        );
    }

    // ── Node::Alias ───────────────────────────────────────────────────────────

    #[test]
    fn alias_bare() {
        assert_eq!(
            render(&[Node::Alias {
                name: "ll".into(),
                body: "ls -la".into()
            }]),
            "alias ll ls -la"
        );
    }

    // ── conditions ────────────────────────────────────────────────────────────

    #[test]
    fn cond_have() {
        assert_eq!(FishEmitter.cond(&Cond::Have("git".into())), "type -q git");
    }

    #[test]
    fn cond_exists() {
        assert_eq!(
            FishEmitter.cond(&Cond::Exists("/home/user/.cargo/bin".into())),
            "test -e \"/home/user/.cargo/bin\""
        );
    }

    #[test]
    fn cond_env() {
        assert_eq!(
            FishEmitter.cond(&Cond::Env("CARGO_HOME".into())),
            "set -q CARGO_HOME"
        );
    }

    #[test]
    fn cond_os_darwin() {
        assert_eq!(
            FishEmitter.cond(&Cond::Os("darwin".into())),
            "test (uname -s) = \"Darwin\""
        );
    }

    #[test]
    fn cond_os_linux() {
        assert_eq!(
            FishEmitter.cond(&Cond::Os("linux".into())),
            "test (uname -s) = \"Linux\""
        );
    }

    #[test]
    fn cond_shell_fish_is_true() {
        assert_eq!(FishEmitter.cond(&Cond::Shell("fish".into())), "true");
    }

    #[test]
    fn cond_shell_other_is_false() {
        assert_eq!(FishEmitter.cond(&Cond::Shell("bash".into())), "false");
    }

    #[test]
    fn cond_not() {
        assert_eq!(
            FishEmitter.cond(&Cond::Not(Box::new(Cond::Have("git".into())))),
            "not type -q git"
        );
    }

    #[test]
    fn cond_and() {
        assert_eq!(
            FishEmitter.cond(&Cond::And(
                Box::new(Cond::Have("cargo".into())),
                Box::new(Cond::Os("linux".into())),
            )),
            "type -q cargo;  and test (uname -s) = \"Linux\""
        );
    }

    #[test]
    fn cond_or() {
        assert_eq!(
            FishEmitter.cond(&Cond::Or(
                Box::new(Cond::Os("darwin".into())),
                Box::new(Cond::Os("linux".into())),
            )),
            "test (uname -s) = \"Darwin\";  or test (uname -s) = \"Linux\""
        );
    }

    // ── if / elif / else / end ────────────────────────────────────────────────

    #[test]
    fn if_end() {
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
        assert!(out.contains("if type -q git"), "missing if: {}", out);
        assert!(out.contains("set -gx X"), "missing body: {}", out);
        assert!(out.contains("end"), "missing end: {}", out);
    }

    #[test]
    fn if_elif_else_end() {
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
        assert!(out.contains("else if"), "missing else if: {}", out);
        assert!(out.contains("else"), "missing else: {}", out);
        assert!(out.contains("end"), "missing end: {}", out);
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
            out.lines().any(|l| l.starts_with("  set -gx")),
            "body not indented: {}",
            out
        );
    }
}
