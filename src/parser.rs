use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use crate::ast::{Cond, IfNode, Node, ParseError, PathDir};

pub struct Parser {
    /// Each entry is `(source_line_number, tokens)`. Line numbers are 1-based
    /// and reflect the original file so error messages are actionable.
    lines: Vec<(usize, Vec<String>)>,
    pos: usize,
    /// Anchor directory used to resolve relative `path+` / `path-` tokens.
    /// `None` when reading from stdin (no meaningful anchor).
    base: Option<PathBuf>,
    lex_error: Option<ParseError>,
}

// ── free helpers ─────────────────────────────────────────────────────────────

/// Strip a single layer of surrounding `"…"` or `'…'` from `s`.
/// Only acts when the *entire* token is wrapped; partial quotes are left alone.
/// Returns a borrowed slice when no stripping is needed (zero allocation).
fn strip_quotes(s: &str) -> Cow<'_, str> {
    let b = s.as_bytes();
    let quoted = b.len() >= 2
        && ((b[0] == b'"' && b[b.len() - 1] == b'"') || (b[0] == b'\'' && b[b.len() - 1] == b'\''));
    if quoted {
        Cow::Owned(s[1..s.len() - 1].to_owned())
    } else {
        Cow::Borrowed(s)
    }
}

/// Return the token at index `idx`, or a `ParseError` with `msg`.
fn tok(toks: &[String], idx: usize, ln: usize, msg: &str) -> Result<String, ParseError> {
    toks.get(idx)
        .ok_or_else(|| ParseError::at(ln, msg))
        .cloned()
}

/// Return token at `idx` with outer quotes stripped.
fn tok_stripped(toks: &[String], idx: usize, ln: usize, msg: &str) -> Result<String, ParseError> {
    toks.get(idx)
        .ok_or_else(|| ParseError::at(ln, msg))
        .map(|s| strip_quotes(s).into_owned())
}

/// Join tokens from index `from` onward with spaces, stripping outer quotes
/// from the joined result. Returns `None` when the slice is empty.
fn tail_joined(toks: &[String], from: usize) -> Option<String> {
    let rest = toks.get(from..)?;
    if rest.is_empty() {
        None
    } else {
        Some(rest.join(" "))
    }
}

/// Like `tail_joined` but also strips outer quotes from the joined string.
fn tail_stripped(toks: &[String], from: usize) -> Option<String> {
    tail_joined(toks, from).map(|s| strip_quotes(&s).into_owned())
}

/// Find the rightmost position of `op` in `toks` satisfying the
/// left-associative binary-operator constraint:
///   - at least two tokens to the left  (`i >= 2`)
///   - at least one token to the right  (`i + 1 < toks.len()`)
fn last_op_pos(toks: &[String], op: &str) -> Option<usize> {
    toks.iter()
        .enumerate()
        .rev()
        .find_map(|(i, t)| (t == op && i >= 2 && i + 1 < toks.len()).then_some(i))
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Return `true` when `s` contains a shell variable reference that must be
/// left for the target shell to expand at runtime.
///
/// Detected forms:
///   `$WORD`         — POSIX / bash / fish / pwsh bare variable
///   `${WORD}`       — POSIX brace form
///   `$env:WORD`     — PowerShell env-drive form
///   `~`             — tilde shorthand (expanded by the shell at runtime,
///                     NOT inside double-quoted strings in any shell)
///
/// Any path containing these must NOT be passed through `PathBuf`, which
/// would corrupt the variable name by treating it as a literal path component.
fn contains_shell_variable(s: &str) -> bool {
    // Tilde at the start is a shell expansion that must be left to the shell.
    if s.starts_with('~') {
        return true;
    }
    // Scan for $ anywhere in the token.
    s.contains('$')
}

fn is_windows_absolute(s: &str) -> bool {
    let b = s.as_bytes();
    (b.len() >= 3 && b[1] == b':' && matches!(b[2], b'/' | b'\\'))
        || s.starts_with("\\\\")
        || s.starts_with("//")
}

fn valid_env_key(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn valid_command(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| !c.is_whitespace() && !matches!(c, ';' | '&' | '|' | '$' | '`' | '(' | ')'))
}

/// Tokenize one source line while preserving quote characters for the parser's
/// existing outer-quote handling. Whitespace inside quotes stays in the same
/// token, and `#` starts a comment only outside quotes.
fn tokenize_line(raw: &str) -> Result<Vec<String>, &'static str> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut chars = raw.chars();

    while let Some(ch) = chars.next() {
        if let Some(q) = quote {
            if ch == '\\' {
                current.push(ch);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            } else {
                current.push(ch);
                if ch == q {
                    quote = None;
                }
            }
            continue;
        }

        match ch {
            '\'' | '"' if current.is_empty() => {
                quote = Some(ch);
                current.push(ch);
            }
            '\'' | '"' => current.push(ch),
            '#' => break,
            c if c.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if quote.is_some() {
        return Err("unterminated quote");
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

/// Resolve a path token from a shed source file.
///
/// Rules (applied in order):
/// 1. Shell variable detected (`$…`, `${…}`, `$env:…`, leading `~`) →
///    return the raw token **unchanged**.  The target shell expands it.
///    Note: `~` is NOT expanded inside double-quoted strings by any shell,
///    so we must not emit it inside quotes; emitters handle quoting.
/// 2. Relative path → join onto `base` (the shed file's directory).
///    When `base` is `None` (stdin) relative paths are kept as-is.
/// 3. Absolute path → returned unchanged.
///
/// In all cases the returned string uses forward slashes so bash/fish/zsh
/// and PowerShell all accept it.
pub fn resolve_path(raw: &str, base: Option<&Path>) -> String {
    // ── Step 1: guard shell variables — leave them for the target shell ───────
    if contains_shell_variable(raw) {
        // Only normalise the delimiter; do not touch variable names.
        return if raw.contains('\\') {
            raw.replace('\\', "/")
        } else {
            raw.to_owned()
        };
    }

    // ── Step 2: plain path — resolve relative paths against base dir ──────────
    if is_windows_absolute(raw) {
        return raw.replace('\\', "/");
    }
    let expanded = PathBuf::from(raw);
    let resolved = if expanded.is_relative() {
        base.map(|b| b.join(&expanded)).unwrap_or(expanded)
    } else {
        expanded
    };

    // ── Step 3: normalise separators ─────────────────────────────────────────
    let s = resolved.to_string_lossy();
    if s.contains('\\') {
        s.replace('\\', "/")
    } else {
        s.into_owned()
    }
}

// ── Parser ───────────────────────────────────────────────────────────────────

impl Parser {
    /// Construct a parser for `src`.
    ///
    /// `base` is the directory of the shed source file, used to resolve
    /// relative and home-prefixed `path+` / `path-` tokens at parse time.
    /// Pass `None` when reading from stdin (no anchor directory).
    pub fn new(src: &str, base: Option<PathBuf>) -> Self {
        let mut lines = Vec::new();
        let mut lex_error = None;
        for (i, raw) in src.lines().enumerate() {
            match tokenize_line(raw.trim()) {
                Ok(toks) if !toks.is_empty() => lines.push((i + 1, toks)),
                Ok(_) => {}
                Err(msg) if lex_error.is_none() => {
                    lex_error = Some(ParseError::at(i + 1, msg));
                }
                Err(_) => {}
            }
        }
        Self {
            lines,
            pos: 0,
            base,
            lex_error,
        }
    }

    pub fn parse(&mut self) -> Result<Vec<Node>, ParseError> {
        if let Some(error) = self.lex_error.take() {
            return Err(error);
        }
        self.block(&[])
    }

    // ── internal ─────────────────────────────────────────────────────────────

    /// Return `(line_number, token_slice)` for the current line without advancing.
    fn peek(&self) -> Option<(usize, &[String])> {
        self.lines.get(self.pos).map(|(ln, t)| (*ln, t.as_slice()))
    }

    fn block(&mut self, stops: &[&str]) -> Result<Vec<Node>, ParseError> {
        let mut nodes = Vec::new();
        while let Some((_, t)) = self.peek() {
            if stops.contains(&t[0].as_str()) {
                break;
            }
            nodes.push(self.parse_statement()?);
        }
        Ok(nodes)
    }

    fn parse_statement(&mut self) -> Result<Node, ParseError> {
        // SAFETY: only called after peek() confirms a line exists at self.pos.
        let (ln, toks) = &self.lines[self.pos];
        let ln = *ln;
        // SAFETY: filter_map in new() guarantees every stored line has >=1 token.
        let node = match toks[0].as_str() {
            "set" => {
                let key = tok(toks, 1, ln, "usage: set KEY VALUE")?;
                if !valid_env_key(&key) {
                    return Err(ParseError::at(
                        ln,
                        "usage: set KEY VALUE (KEY must be a shell variable name)",
                    ));
                }
                let val = tail_stripped(toks, 2)
                    .ok_or_else(|| ParseError::at(ln, "usage: set KEY VALUE"))?;
                Node::Set { key, val }
            }
            kw @ ("path+" | "path-") => {
                if toks.len() != 2 {
                    return Err(ParseError::at(ln, format!("usage: {} DIR", kw)));
                }
                let direction = match kw {
                    "path+" => PathDir::Prepend,
                    _ => PathDir::Append,
                };
                let raw = tok_stripped(toks, 1, ln, &format!("usage: {} DIR", kw))?;
                let dir = resolve_path(&raw, self.base.as_deref());
                Node::Path { dir, direction }
            }
            "call" => {
                let cmd = tok_stripped(toks, 1, ln, "usage: call CMD [ARGS]")?;
                if !valid_command(&cmd) {
                    return Err(ParseError::at(ln, "call requires one command token"));
                }
                let args = tail_joined(toks, 2).unwrap_or_default();
                Node::Call { cmd, args }
            }
            "alias" => {
                let name = tok(toks, 1, ln, "usage: alias NAME BODY")?;
                let body = tail_stripped(toks, 2)
                    .ok_or_else(|| ParseError::at(ln, "usage: alias NAME BODY"))?;
                Node::Alias { name, body }
            }
            "if" => {
                let cond_slice = toks
                    .get(1..)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| ParseError::at(ln, "usage: if <cond-type> <value>"))?;
                let cond = self.parse_cond(ln, cond_slice)?;
                self.pos += 1;
                return Ok(Node::If(self.parse_if(ln, cond)?));
            }
            kw => return Err(ParseError::at(ln, format!("unknown keyword {:?}", kw))),
        };
        self.pos += 1;
        Ok(node)
    }

    fn parse_cond(&self, ln: usize, toks: &[String]) -> Result<Cond, ParseError> {
        self.parse_or(ln, toks)
    }

    /// Lowest precedence: `or`. Left-associative.
    ///
    /// Splits at the LAST `or`; left subtree recurses through `parse_or`,
    /// right side is parsed by `parse_and`.
    fn parse_or(&self, ln: usize, toks: &[String]) -> Result<Cond, ParseError> {
        if let Some(op) = last_op_pos(toks, "or") {
            let left = self.parse_or(ln, &toks[..op])?;
            let right = self.parse_and(ln, &toks[op + 1..])?;
            return Ok(Cond::Or(Box::new(left), Box::new(right)));
        }
        self.parse_and(ln, toks)
    }

    /// Medium precedence: `and`. Left-associative.
    fn parse_and(&self, ln: usize, toks: &[String]) -> Result<Cond, ParseError> {
        if let Some(op) = last_op_pos(toks, "and") {
            let left = self.parse_and(ln, &toks[..op])?;
            let right = self.parse_not(ln, &toks[op + 1..])?;
            return Ok(Cond::And(Box::new(left), Box::new(right)));
        }
        self.parse_not(ln, toks)
    }

    /// Highest precedence: prefix `not`. Right-associative (naturally).
    fn parse_not(&self, ln: usize, toks: &[String]) -> Result<Cond, ParseError> {
        if toks.first().map(|s| s.as_str()) == Some("not") {
            let rest = toks
                .get(1..)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| ParseError::at(ln, "'not' requires a condition"))?;
            return Ok(Cond::Not(Box::new(self.parse_not(ln, rest)?)));
        }
        self.parse_leaf(ln, toks)
    }

    /// Parse a leaf condition: `<type> <value>`.
    fn parse_leaf(&self, ln: usize, toks: &[String]) -> Result<Cond, ParseError> {
        if toks.len() != 2 {
            return Err(ParseError::at(ln, "condition requires exactly one value"));
        }
        let kind = toks
            .first()
            .ok_or_else(|| ParseError::at(ln, "condition requires a type"))?;
        let val = tok_stripped(toks, 1, ln, &format!("{} requires a value", kind))?;
        match kind.as_str() {
            "have" if valid_command(&val) => Ok(Cond::Have(val)),
            "have" => Err(ParseError::at(ln, "have requires one command token")),
            // `exists` takes a filesystem path — apply the same variable guard
            // and forward-slash normalisation used for path+/path-.
            "exists" => Ok(Cond::Exists(resolve_path(&val, self.base.as_deref()))),
            "env" if valid_env_key(&val) => Ok(Cond::Env(val)),
            "env" => Err(ParseError::at(ln, "env requires a shell variable name")),
            "os" => Ok(Cond::Os(val)),
            "shell" => Ok(Cond::Shell(val)),
            other => Err(ParseError::at(
                ln,
                format!(
                    "unknown condition {:?} -- use: have | exists | env | os | shell",
                    other
                ),
            )),
        }
    }

    fn parse_if(&mut self, if_ln: usize, cond: Cond) -> Result<IfNode, ParseError> {
        let mut node = IfNode {
            cond,
            body: self.block(&["elif", "else", "end"])?,
            elifs: Vec::new(),
            else_: Vec::new(),
        };

        loop {
            match self
                .peek()
                .and_then(|(ln, t)| t.first().map(|kw| (ln, kw.clone())))
            {
                None => break,
                Some((_, kw)) if kw == "end" => {
                    self.pos += 1;
                    return Ok(node);
                }
                Some((ln, kw)) if kw == "elif" => {
                    let cond_slice = self.lines[self.pos]
                        .1
                        .get(1..)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| ParseError::at(ln, "elif requires a condition"))?;
                    let cond = self.parse_cond(ln, cond_slice)?;
                    self.pos += 1;
                    let b = self.block(&["elif", "else", "end"])?;
                    node.elifs.push((cond, b));
                }
                Some((_, kw)) if kw == "else" => {
                    self.pos += 1;
                    node.else_ = self.block(&["end"])?;
                }
                Some((ln, kw)) => {
                    return Err(ParseError::at(
                        ln,
                        format!("unexpected {:?} inside if-block", kw),
                    ));
                }
            }
        }

        Err(ParseError::at(
            if_ln,
            "unterminated if-block (missing 'end')",
        ))
    }
}

// ── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Cond, Node, PathDir};

    fn parse(src: &str) -> Result<Vec<Node>, ParseError> {
        Parser::new(src, None).parse()
    }

    /// Parse `src`, assert exactly one node, return it.
    fn parse_one(src: &str) -> Node {
        let mut nodes = parse(src).expect("parse failed");
        assert_eq!(nodes.len(), 1, "expected 1 node, got {}", nodes.len());
        nodes.remove(0)
    }

    // ── set ───────────────────────────────────────────────────────────────────

    #[test]
    fn set_parses_key_and_value() {
        match parse_one("set EDITOR nvim") {
            Node::Set { key, val } => {
                assert_eq!(key, "EDITOR");
                assert_eq!(val, "nvim");
            }
            n => panic!("{:?}", n),
        }
    }

    #[test]
    fn set_multiword_value() {
        match parse_one("set GREETING hello world") {
            Node::Set { val, .. } => assert_eq!(val, "hello world"),
            n => panic!("{:?}", n),
        }
    }

    #[test]
    fn quoted_spaces_and_hashes_are_preserved() {
        match parse_one("set GREETING \"hello world # users\" # comment") {
            Node::Set { val, .. } => assert_eq!(val, "hello world # users"),
            n => panic!("{:?}", n),
        }
    }

    #[test]
    fn unterminated_quotes_are_reported() {
        let error = parse("set GREETING \"hello").expect_err("quote must fail");
        assert!(error.msg.contains("unterminated quote"));
    }

    /// Outer double-quotes must be stripped; emitters will re-wrap the value.
    #[test]
    fn set_strips_outer_double_quotes() {
        match parse_one(r#"set EDITOR "nvim""#) {
            Node::Set { val, .. } => assert_eq!(val, "nvim", "double-quotes not stripped"),
            n => panic!("{:?}", n),
        }
    }

    /// Outer single-quotes must also be stripped.
    #[test]
    fn set_strips_outer_single_quotes() {
        match parse_one("set EDITOR 'nvim'") {
            Node::Set { val, .. } => assert_eq!(val, "nvim", "single-quotes not stripped"),
            n => panic!("{:?}", n),
        }
    }

    /// Partial / interior quotes must NOT be removed.
    #[test]
    fn set_partial_quotes_left_alone() {
        match parse_one(r#"set KEY foo"bar"#) {
            Node::Set { val, .. } => assert_eq!(val, r#"foo"bar"#),
            n => panic!("{:?}", n),
        }
    }

    /// Multi-word value that is NOT wholly wrapped: no stripping.
    #[test]
    fn set_multiword_no_strip() {
        match parse_one(r#"set G "hello" world"#) {
            Node::Set { val, .. } => assert_eq!(val, r#""hello" world"#),
            n => panic!("{:?}", n),
        }
    }

    #[test]
    fn set_missing_key_is_error() {
        assert!(parse("set").is_err());
    }

    #[test]
    fn set_missing_value_is_error() {
        assert!(parse("set ONLY").is_err());
    }

    // ── alias ─────────────────────────────────────────────────────────────────

    #[test]
    fn alias_parses_name_and_body() {
        match parse_one("alias gs git status") {
            Node::Alias { name, body } => {
                assert_eq!(name, "gs");
                assert_eq!(body, "git status");
            }
            n => panic!("{:?}", n),
        }
    }

    /// Double-quoted body must be stripped once so emitters do not double-wrap.
    #[test]
    fn alias_strips_outer_double_quotes() {
        match parse_one(r#"alias ll "ls -la""#) {
            Node::Alias { body, .. } => assert_eq!(body, "ls -la", "quotes not stripped"),
            n => panic!("{:?}", n),
        }
    }

    /// Single-quoted body must also be stripped.
    #[test]
    fn alias_strips_outer_single_quotes() {
        match parse_one("alias ll 'ls -la'") {
            Node::Alias { body, .. } => assert_eq!(body, "ls -la", "quotes not stripped"),
            n => panic!("{:?}", n),
        }
    }

    #[test]
    fn alias_missing_name_is_error() {
        assert!(parse("alias").is_err());
    }

    #[test]
    fn alias_missing_body_is_error() {
        assert!(parse("alias gs").is_err());
    }

    // ── path+ / path- ─────────────────────────────────────────────────────────

    #[test]
    fn path_plus_prepend() {
        match parse_one("path+ /usr/local/bin") {
            Node::Path { direction, .. } => assert_eq!(direction, PathDir::Prepend),
            n => panic!("{:?}", n),
        }
    }

    #[test]
    fn path_minus_append() {
        match parse_one("path- /opt/bin") {
            Node::Path { direction, .. } => assert_eq!(direction, PathDir::Append),
            n => panic!("{:?}", n),
        }
    }

    /// Quoted path token must have its quotes stripped before storage.
    #[test]
    fn path_strips_outer_quotes() {
        match parse_one(r#"path+ "/usr/local/bin""#) {
            Node::Path { dir, .. } => assert_eq!(dir, "/usr/local/bin"),
            n => panic!("{:?}", n),
        }
    }

    /// resolve_path normalises Windows backslashes to forward slashes.
    #[test]
    fn path_forward_slash_normalised() {
        let result = resolve_path("C:\\Users\\user\\.cargo\\bin", None);
        assert!(!result.contains('\\'), "backslash in: {}", result);
        assert!(result.contains('/'), "no forward-slash in: {}", result);
    }

    /// Forward slashes must appear in the stored dir on any OS.
    #[test]
    fn path_no_backslash_in_stored_dir() {
        match parse_one("path+ /usr/local/bin") {
            Node::Path { dir, .. } => assert!(!dir.contains('\\'), "backslash in dir: {}", dir),
            n => panic!("{:?}", n),
        }
    }

    #[test]
    fn path_missing_dir_is_error() {
        assert!(parse("path+").is_err());
        assert!(parse("path-").is_err());
    }

    // ── call ──────────────────────────────────────────────────────────────────

    #[test]
    fn call_no_args() {
        match parse_one("call myprog") {
            Node::Call { cmd, args } => {
                assert_eq!(cmd, "myprog");
                assert_eq!(args, "");
            }
            n => panic!("{:?}", n),
        }
    }

    #[test]
    fn call_with_args() {
        match parse_one("call starship init {shell}") {
            Node::Call { cmd, args } => {
                assert_eq!(cmd, "starship");
                assert_eq!(args, "init {shell}");
            }
            n => panic!("{:?}", n),
        }
    }

    #[test]
    fn call_missing_cmd_is_error() {
        assert!(parse("call").is_err());
    }

    // ── if / elif / else / end ────────────────────────────────────────────────

    #[test]
    fn if_elif_else_end_structure() {
        let src = "if os darwin\nset A 1\nelif os linux\nset A 2\nelse\nset A 3\nend";
        match parse_one(src) {
            Node::If(n) => {
                assert_eq!(n.body.len(), 1, "body");
                assert_eq!(n.elifs.len(), 1, "elifs");
                assert_eq!(n.else_.len(), 1, "else");
                assert!(matches!(&n.cond, Cond::Os(s) if s == "darwin"));
            }
            n => panic!("{:?}", n),
        }
    }

    #[test]
    fn nested_if_parses() {
        let src = "if have cargo\nif os darwin\nset A 1\nend\nend";
        match parse_one(src) {
            Node::If(outer) => match &outer.body[0] {
                Node::If(_) => {}
                n => panic!("expected nested if: {:?}", n),
            },
            n => panic!("{:?}", n),
        }
    }

    #[test]
    fn if_missing_cond_is_error() {
        assert!(parse("if\nend").is_err());
    }

    #[test]
    fn if_unknown_cond_type_is_error() {
        assert!(parse("if foobar baz\nend").is_err());
    }

    #[test]
    fn if_missing_cond_value_is_error() {
        assert!(parse("if have\nend").is_err());
    }

    #[test]
    fn unterminated_if_reports_opening_line() {
        let err = parse("set A 1\nif have git\nset B 2").unwrap_err();
        assert_eq!(err.line, 2, "should point to the if line: {}", err);
    }

    // ── comments & blanks ─────────────────────────────────────────────────────

    #[test]
    fn comments_and_blanks_ignored() {
        let nodes = parse("# comment\n\nset A B # inline").unwrap();
        assert_eq!(nodes.len(), 1);
    }

    // ── error line numbers ────────────────────────────────────────────────────

    #[test]
    fn errors_carry_line_number() {
        let err = parse("set A 1\nsett FOO bar").unwrap_err();
        assert_eq!(err.line, 2, "wrong line: {}", err);
        assert!(err.msg.contains("sett"), "wrong msg: {}", err);
    }

    #[test]
    fn error_display_includes_line_prefix() {
        let err = parse("bad keyword here").unwrap_err();
        assert!(format!("{}", err).starts_with("line "));
    }

    // ── compound condition precedence ─────────────────────────────────────────

    #[test]
    fn not_leaf() {
        match parse_one("if not have cargo\nend") {
            Node::If(n) => {
                assert!(matches!(&n.cond, Cond::Not(c) if matches!(c.as_ref(), Cond::Have(_))))
            }
            n => panic!("{:?}", n),
        }
    }

    #[test]
    fn not_and_precedence() {
        match parse_one("if not have cargo and os linux\nend") {
            Node::If(n) => match &n.cond {
                Cond::And(l, r) => {
                    assert!(matches!(l.as_ref(), Cond::Not(_)), "lhs should be Not");
                    assert!(matches!(r.as_ref(), Cond::Os(_)), "rhs should be Os");
                }
                c => panic!("expected And, got {:?}", c),
            },
            n => panic!("{:?}", n),
        }
    }

    #[test]
    fn or_not_precedence() {
        match parse_one("if have cargo or not shell fish\nend") {
            Node::If(n) => match &n.cond {
                Cond::Or(l, r) => {
                    assert!(matches!(l.as_ref(), Cond::Have(_)), "lhs should be Have");
                    assert!(matches!(r.as_ref(), Cond::Not(_)), "rhs should be Not");
                }
                c => panic!("expected Or, got {:?}", c),
            },
            n => panic!("{:?}", n),
        }
    }

    #[test]
    fn and_or_precedence() {
        match parse_one("if have cargo and os linux or shell bash\nend") {
            Node::If(n) => match &n.cond {
                Cond::Or(l, r) => {
                    assert!(matches!(l.as_ref(), Cond::And(_, _)), "lhs should be And");
                    assert!(matches!(r.as_ref(), Cond::Shell(_)), "rhs should be Shell");
                }
                c => panic!("expected Or(And, Shell), got {:?}", c),
            },
            n => panic!("{:?}", n),
        }
    }

    #[test]
    fn or_and_right_precedence() {
        match parse_one("if os linux or have cargo and shell bash\nend") {
            Node::If(n) => match &n.cond {
                Cond::Or(l, r) => {
                    assert!(matches!(l.as_ref(), Cond::Os(_)), "lhs should be Os");
                    assert!(matches!(r.as_ref(), Cond::And(_, _)), "rhs should be And");
                }
                c => panic!("expected Or(Os, And), got {:?}", c),
            },
            n => panic!("{:?}", n),
        }
    }

    #[test]
    fn and_left_associative() {
        match parse_one("if have cargo and os linux and shell bash\nend") {
            Node::If(n) => match &n.cond {
                Cond::And(l, r) => {
                    assert!(
                        matches!(l.as_ref(), Cond::And(_, _)),
                        "outer lhs should be And"
                    );
                    assert!(
                        matches!(r.as_ref(), Cond::Shell(_)),
                        "outer rhs should be Shell"
                    );
                }
                c => panic!("expected And(And, Shell), got {:?}", c),
            },
            n => panic!("{:?}", n),
        }
    }

    // ── resolve_path tests ────────────────────────────────────────────────────

    #[test]
    fn resolve_path_absolute_unix_unchanged() {
        assert_eq!(resolve_path("/usr/local/bin", None), "/usr/local/bin");
    }

    #[test]
    fn resolve_path_backslash_normalised() {
        let r = resolve_path("C:\\Users\\user\\.cargo\\bin", None);
        assert!(!r.contains('\\'), "backslash in: {}", r);
        assert!(r.contains('/'));
    }

    /// Shell variable tokens must be returned as-is (only delimiter fixed).
    #[test]
    fn resolve_path_dollar_home_left_intact() {
        let r = resolve_path("$HOME/.cargo/bin", None);
        assert_eq!(r, "$HOME/.cargo/bin", "variable was mutated: {}", r);
    }

    #[test]
    fn resolve_path_tilde_left_intact() {
        let r = resolve_path("~/.cargo/bin", None);
        assert_eq!(r, "~/.cargo/bin", "tilde was mutated: {}", r);
    }

    #[test]
    fn resolve_path_env_colon_left_intact() {
        let r = resolve_path("$env:USERPROFILE/.cargo/bin", None);
        assert_eq!(r, "$env:USERPROFILE/.cargo/bin");
    }

    /// Backslash in a variable path must still be normalised to forward slash.
    #[test]
    fn resolve_path_variable_backslash_normalised() {
        let r = resolve_path("$HOME\\.cargo\\bin", None);
        assert!(!r.contains('\\'), "backslash in: {}", r);
        assert_eq!(r, "$HOME/.cargo/bin");
    }

    // ── parse_leaf exists tests ───────────────────────────────────────────────

    #[test]
    fn parse_leaf_exists_absolute_unchanged() {
        match parse_one("if exists /usr/bin/git\nend") {
            Node::If(n) => match &n.cond {
                Cond::Exists(p) => assert_eq!(p, "/usr/bin/git"),
                c => panic!("expected Exists, got {:?}", c),
            },
            n => panic!("{:?}", n),
        }
    }

    /// `$HOME` in an exists path must be stored as-is, not corrupted by PathBuf.
    #[test]
    fn parse_leaf_exists_dollar_home_intact() {
        match parse_one("if exists $HOME/.cargo/bin\nend") {
            Node::If(n) => match &n.cond {
                Cond::Exists(p) => assert_eq!(p, "$HOME/.cargo/bin"),
                c => panic!("expected Exists, got {:?}", c),
            },
            n => panic!("{:?}", n),
        }
    }

    #[test]
    fn parse_leaf_exists_tilde_intact() {
        match parse_one("if exists ~/go/bin\nend") {
            Node::If(n) => match &n.cond {
                Cond::Exists(p) => assert_eq!(p, "~/go/bin"),
                c => panic!("expected Exists, got {:?}", c),
            },
            n => panic!("{:?}", n),
        }
    }

    /// Backslash in a plain (non-variable) exists path must be normalised.
    #[test]
    fn parse_leaf_exists_normalises_backslash() {
        match parse_one("if exists C:\\tools\\bin\nend") {
            Node::If(n) => match &n.cond {
                Cond::Exists(p) => assert_eq!(p, "C:/tools/bin"),
                c => panic!("expected Exists, got {:?}", c),
            },
            n => panic!("{:?}", n),
        }
    }

    #[test]
    fn relative_exists_path_uses_source_base() {
        let mut parser = Parser::new("if exists bin\nend", Some(PathBuf::from("/tmp/project")));
        match parser.parse().expect("parse failed").as_slice() {
            [Node::If(n)] => match &n.cond {
                Cond::Exists(path) => assert_eq!(path, "/tmp/project/bin"),
                other => panic!("expected Exists, got {:?}", other),
            },
            nodes => panic!("expected one if node, got {:?}", nodes),
        }
    }

    // ── variable-path handling (the $HOME / ~ injection guard) ───────────────

    /// $HOME in an exists condition is left intact — not treated as a relative path.
    #[test]
    fn dollar_home_in_exists_cond_is_intact() {
        match parse_one("if exists $HOME/.cargo/bin\nend") {
            Node::If(n) => {
                assert!(matches!(&n.cond, Cond::Exists(p) if p == "$HOME/.cargo/bin"));
            }
            n => panic!("{:?}", n),
        }
    }

    /// $HOME in a path+ directive is left intact.
    #[test]
    fn dollar_home_in_path_dir_is_intact() {
        match parse_one("path+ $HOME/.cargo/bin") {
            Node::Path { dir, .. } => {
                assert_eq!(dir, "$HOME/.cargo/bin");
                assert!(!dir.contains('\\'), "backslash in: {}", dir);
            }
            n => panic!("{:?}", n),
        }
    }

    /// $HOME in a set value is left intact.
    #[test]
    fn dollar_home_in_set_value_is_intact() {
        match parse_one("set CARGO_HOME $HOME/.cargo") {
            Node::Set { val, .. } => assert_eq!(val, "$HOME/.cargo"),
            n => panic!("{:?}", n),
        }
    }

    /// ~ in a set value is left intact (shells do NOT expand ~ inside quotes).
    #[test]
    fn tilde_in_set_value_is_intact() {
        match parse_one("set STARSHIP_CONFIG ~/.config/starship.toml") {
            Node::Set { val, .. } => assert_eq!(val, "~/.config/starship.toml"),
            n => panic!("{:?}", n),
        }
    }

    /// $env:USERPROFILE (pwsh style) in a path is left intact.
    #[test]
    fn pwsh_env_var_in_path_is_intact() {
        match parse_one("path+ $env:USERPROFILE/.cargo/bin") {
            Node::Path { dir, .. } => assert_eq!(dir, "$env:USERPROFILE/.cargo/bin"),
            n => panic!("{:?}", n),
        }
    }

    /// A backslash in a variable path is normalised to forward slash.
    #[test]
    fn dollar_home_backslash_normalised() {
        match parse_one("path+ $HOME\\.cargo\\bin") {
            Node::Path { dir, .. } => {
                assert!(!dir.contains('\\'), "backslash in: {}", dir);
                assert_eq!(dir, "$HOME/.cargo/bin");
            }
            n => panic!("{:?}", n),
        }
    }

    /// compound Or: two exists with different variable prefixes.
    #[test]
    fn compound_or_two_exists_variable_paths() {
        match parse_one("if exists $HOME/aqua.yml or exists ~/aqua.yaml\nend") {
            Node::If(n) => match &n.cond {
                Cond::Or(l, r) => {
                    assert!(matches!(l.as_ref(), Cond::Exists(p) if p == "$HOME/aqua.yml"));
                    assert!(matches!(r.as_ref(), Cond::Exists(p) if p == "~/aqua.yaml"));
                }
                c => panic!("expected Or, got {:?}", c),
            },
            n => panic!("{:?}", n),
        }
    }

    /// multiword set value with no quoting (fzf pattern).
    #[test]
    fn multiword_set_no_quotes() {
        match parse_one("set FZF_DEFAULT_COMMAND fd --type f --hidden --follow --exclude .git") {
            Node::Set { key, val } => {
                assert_eq!(key, "FZF_DEFAULT_COMMAND");
                assert_eq!(val, "fd --type f --hidden --follow --exclude .git");
            }
            n => panic!("{:?}", n),
        }
    }

    /// alias with single-word body (tool replacement pattern).
    #[test]
    fn alias_tool_replacement() {
        match parse_one("alias grep rg") {
            Node::Alias { name, body } => {
                assert_eq!(name, "grep");
                assert_eq!(body, "rg");
            }
            n => panic!("{:?}", n),
        }
    }

    /// call with {shell} placeholder stored raw.
    #[test]
    fn call_shell_placeholder_stored_raw() {
        match parse_one("call starship init {shell}") {
            Node::Call { cmd, args } => {
                assert_eq!(cmd, "starship");
                assert_eq!(args, "init {shell}");
            }
            n => panic!("{:?}", n),
        }
    }

    /// os condition with alias body (windows explorer pattern).
    #[test]
    fn os_windows_alias() {
        match parse_one("if os windows\nalias e explorer.exe\nend") {
            Node::If(n) => {
                assert!(matches!(&n.cond, Cond::Os(s) if s == "windows"));
                match &n.body[0] {
                    Node::Alias { name, body } => {
                        assert_eq!(name, "e");
                        assert_eq!(body, "explorer.exe");
                    }
                    x => panic!("{:?}", x),
                }
            }
            n => panic!("{:?}", n),
        }
    }

    /// Full cargo block: exists guard + path + two set nodes with $HOME values.
    #[test]
    fn cargo_block_structure() {
        let src = "if exists $HOME/.cargo/bin\npath+ $HOME/.cargo/bin\nset CARGO_HOME $HOME/.cargo\nset RUSTUP_HOME $HOME/.rustup\nend";
        match parse_one(src) {
            Node::If(n) => {
                assert!(matches!(&n.cond, Cond::Exists(p) if p == "$HOME/.cargo/bin"));
                assert_eq!(n.body.len(), 3);
                assert!(matches!(&n.body[0], Node::Path { dir, direction }
                    if dir == "$HOME/.cargo/bin" && *direction == PathDir::Prepend));
                assert!(matches!(&n.body[1], Node::Set { key, val }
                    if key == "CARGO_HOME" && val == "$HOME/.cargo"));
                assert!(matches!(&n.body[2], Node::Set { key, val }
                    if key == "RUSTUP_HOME" && val == "$HOME/.rustup"));
            }
            n => panic!("{:?}", n),
        }
    }

    #[test]
    fn rejects_invalid_runtime_condition_tokens() {
        assert!(parse("if have bad;cmd\nend").is_err());
        assert!(parse("if env BAD-NAME\nend").is_err());
    }

    // ── strip_quotes unit tests ───────────────────────────────────────────────

    #[test]
    fn strip_quotes_removes_double() {
        assert_eq!(strip_quotes(r#""hello""#).as_ref(), "hello");
    }

    #[test]
    fn strip_quotes_removes_single() {
        assert_eq!(strip_quotes("'hello'").as_ref(), "hello");
    }

    #[test]
    fn strip_quotes_leaves_unquoted() {
        assert_eq!(strip_quotes("hello").as_ref(), "hello");
    }

    #[test]
    fn strip_quotes_leaves_partial() {
        assert_eq!(strip_quotes(r#"foo"bar"#).as_ref(), r#"foo"bar"#);
    }

    #[test]
    fn strip_quotes_leaves_mismatched() {
        assert_eq!(strip_quotes(r#""hello'"#).as_ref(), r#""hello'"#);
    }

    #[test]
    fn strip_quotes_too_short() {
        assert_eq!(strip_quotes("\"").as_ref(), "\"");
    }
}
