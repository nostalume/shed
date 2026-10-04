pub mod bash;
pub mod fish;
pub mod pwsh;

use crate::ast::Node;

/// Escape data placed inside a target shell's double-quoted string.
///
/// `$` is intentionally preserved so documented shell variables such as
/// `$HOME` and `$env:USERPROFILE` continue to expand at runtime. Quotes,
/// backslashes, and backticks are escaped to prevent the value from breaking
/// out of the generated string.
pub(crate) fn escape_double_quoted(value: &str, shell: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match (shell, ch) {
            ("pwsh", '`') => out.push_str("``"),
            ("pwsh", '"') => out.push_str("`\""),
            (_, '\\') if shell != "pwsh" => out.push_str("\\\\"),
            (_, '"') => out.push_str("\\\""),
            (_, '`') => out.push_str("\\`"),
            (_, '\r') => out.push_str("\\r"),
            (_, '\n') => out.push_str("\\n"),
            (_, c) => out.push(c),
        }
    }
    out
}

/// Render a home-relative path using the target shell's environment variable.
pub(crate) fn render_path(dir: &str, shell: &str) -> String {
    match dir.strip_prefix('~') {
        Some(rest) => match shell {
            "pwsh" => format!("$env:USERPROFILE{}", rest),
            _ => format!("$HOME{}", rest),
        },
        None => dir.to_owned(),
    }
}

pub(crate) fn escape_path(dir: &str, shell: &str) -> String {
    let rendered = render_path(dir, shell);
    let prefix = match shell {
        "pwsh" => "$env:USERPROFILE",
        _ => "$HOME",
    };
    if let Some(rest) = rendered.strip_prefix(prefix) {
        format!("{}{}", prefix, escape_double_quoted(rest, shell))
    } else {
        escape_double_quoted(&rendered, shell)
    }
}

/// Return `s` prefixed by `depth * 2` spaces.
///
/// Identity law:  `indent(s, 0) == s`
/// Composition:   `indent(indent(s, a), b) == indent(s, a + b)`
///
/// `#[inline]` allows LLVM to see the depth == 0 fast-path at call sites and
/// eliminate the allocation entirely for the common top-level case.
#[inline]
pub fn indent(s: String, depth: usize) -> String {
    if depth == 0 {
        return s;
    }
    // Pre-allocate exactly `depth*2 + s.len()` bytes — one heap allocation.
    let pad = depth * 2;
    let mut buf = String::with_capacity(pad + s.len());
    for _ in 0..pad {
        buf.push(' ');
    }
    buf.push_str(&s);
    buf
}

pub trait Emitter {
    /// The canonical shell name (e.g. "bash", "fish").
    /// Used by concrete emitters for `{shell}` substitution in call args.
    fn name(&self) -> &str;

    /// Emit `nodes` at the given indent `depth`, returning one `String` per output line.
    fn emit_nodes(&self, nodes: &[Node], depth: usize) -> Vec<String>;

    /// Render the full node list to a newline-joined `String`.
    fn render(&self, nodes: &[Node]) -> String {
        self.emit_nodes(nodes, 0).join("\n")
    }

    /// Expand `{shell}` in `args` with the target shell name, then trim.
    /// Returns owned `String`; the trim avoids a trailing newline in output.
    #[inline]
    fn resolve_call_args(&self, args: &str) -> String {
        args.replace("{shell}", self.name()).trim().to_owned()
    }

    /// Format a `call` node using shell-specific prefix and suffix strings.
    ///
    /// `prefix` wraps the command+args (e.g. `"eval \"$("` for bash).
    /// `suffix` closes it (e.g. `")\""` for bash, `" | source"` for fish).
    ///
    /// When `args` is empty the space between cmd and args is omitted.
    /// The result is a single output line, not yet indented.
    #[inline]
    fn format_call(&self, cmd: &str, args: &str, prefix: &str, suffix: &str) -> String {
        let a = self.resolve_call_args(args);
        if a.is_empty() {
            format!("{}{}{}", prefix, cmd, suffix)
        } else {
            format!("{}{} {}{}", prefix, cmd, a, suffix)
        }
    }

    /// Return `s` prefixed by `depth * 2` spaces (delegates to the free function).
    #[inline]
    fn indent(&self, s: String, depth: usize) -> String {
        indent(s, depth)
    }
}
