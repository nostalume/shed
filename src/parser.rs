use crate::ast::{
    Command, Cond, IfNode, Location, Node, Os, ParseError, PathDir, Shell, Statement, Word,
};
use crate::lexer::{Token, identifier, lex};
use std::iter::Enumerate;
use std::str::SplitInclusive;

// Bound recursive construction and subsequent AST destruction on the main stack.
const PARSE_LIMIT: usize = 128;

pub fn parse(source: &str) -> Result<Vec<Node>, ParseError> {
    if let Some(offset) = source.find('\0') {
        let prefix = &source[..offset];
        let at = Location {
            line: prefix.bytes().filter(|&b| b == b'\n').count() + 1,
            column: prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1,
        };
        return Err(ParseError::new(at, "NUL is not allowed in source"));
    }
    let mut parser = Parser {
        lines: source.split_inclusive('\n').enumerate(),
    };
    parser.program()
}

struct Parser<'a> {
    lines: Enumerate<SplitInclusive<'a, char>>,
}

fn line_content(line: &str) -> &str {
    match line.strip_suffix('\n') {
        Some(line) => line.strip_suffix('\r').unwrap_or(line),
        None => line,
    }
}

impl Parser<'_> {
    fn program(&mut self) -> Result<Vec<Node>, ParseError> {
        let mut nodes = Vec::new();
        let mut frames: Vec<Frame> = Vec::new();
        while let Some((index, raw)) = self.lines.next() {
            let line = index + 1;
            let text = line_content(raw);
            let trimmed = text.trim_start_matches([' ', '\t']);
            let node = if trimmed
                .strip_prefix("raw")
                .is_some_and(|tail| tail.is_empty() || tail.starts_with([' ', '\t']))
            {
                self.raw(
                    trimmed,
                    Location {
                        line,
                        column: text.len() - trimmed.len() + 1,
                    },
                )?
            } else {
                let tokens = lex(text, line)?;
                if tokens.is_empty() {
                    continue;
                }
                let mut tokens = Tokens {
                    items: tokens.into_iter(),
                    end: Location {
                        line,
                        column: text.chars().count() + 1,
                    },
                    budget: PARSE_LIMIT,
                };
                let first = tokens.next()?;
                let at = first.at();
                let kind = match first.keyword() {
                    Some("if") => {
                        if frames.len() == PARSE_LIMIT {
                            return Err(ParseError::new(at, "if nesting exceeds parser capacity"));
                        }
                        let cond = tokens.condition()?;
                        tokens.finish()?;
                        frames.push(Frame {
                            at,
                            branches: vec![(cond, Vec::new())],
                            otherwise: None,
                        });
                        continue;
                    }
                    Some("elif" | "else") => {
                        let frame = frames
                            .last_mut()
                            .ok_or_else(|| ParseError::new(at, "unexpected branch delimiter"))?;
                        if frame.otherwise.is_some() {
                            return Err(ParseError::new(at, "branch after else"));
                        }
                        if first.keyword() == Some("elif") {
                            let cond = tokens.condition()?;
                            tokens.finish()?;
                            frame.branches.push((cond, Vec::new()));
                        } else {
                            tokens.finish()?;
                            frame.otherwise = Some(Vec::new());
                        }
                        continue;
                    }
                    Some("end") => {
                        tokens.finish()?;
                        let frame = frames
                            .pop()
                            .ok_or_else(|| ParseError::new(at, "unexpected end"))?;
                        let node = Node {
                            at: frame.at,
                            kind: Statement::If(IfNode {
                                branches: frame.branches,
                                otherwise: frame.otherwise.unwrap_or_default(),
                            }),
                        };
                        if let Some(parent) = frames.last_mut() {
                            parent.body().push(node);
                        } else {
                            nodes.push(node);
                        }
                        continue;
                    }
                    Some("set") => Statement::Set {
                        name: tokens.name(false)?,
                        value: tokens.word()?,
                    },
                    Some("unset") => Statement::Unset(tokens.name(false)?),
                    Some(kw @ ("path+" | "path-")) => Statement::Path {
                        value: tokens.word()?,
                        direction: if kw == "path+" {
                            PathDir::Prepend
                        } else {
                            PathDir::Append
                        },
                    },
                    Some("alias") => {
                        let name = tokens.name(true)?;
                        tokens.expect("=")?;
                        Statement::Alias {
                            name,
                            command: tokens.command()?,
                        }
                    }
                    Some("init") => Statement::Init(tokens.command()?),
                    _ => return Err(ParseError::new(at, "expected a statement keyword")),
                };
                tokens.finish()?;
                Node { at, kind }
            };
            if let Some(frame) = frames.last_mut() {
                frame.body().push(node);
            } else {
                nodes.push(node);
            }
        }
        if let Some(frame) = frames.last() {
            return Err(ParseError::new(
                frame.at,
                "unterminated if-block (missing end)",
            ));
        }
        Ok(nodes)
    }
    fn raw(&mut self, header: &str, at: Location) -> Result<Node, ParseError> {
        let error = || ParseError::new(at, "usage: raw SHELL <<MARKER");
        let mut fields = header.split([' ', '\t']).filter(|field| !field.is_empty());
        fields.next();
        let shell = fields.next().and_then(Shell::parse).ok_or_else(error)?;
        let marker = fields
            .next()
            .and_then(|field| field.strip_prefix("<<"))
            .filter(|marker| identifier(marker))
            .ok_or_else(error)?;
        if fields.next().is_some_and(|field| !field.starts_with('#')) {
            return Err(error());
        }
        let mut body = String::new();
        for (_, line) in self.lines.by_ref() {
            if line_content(line) == marker {
                return Ok(Node {
                    at,
                    kind: Statement::Raw { shell, body },
                });
            }
            body.push_str(line);
        }
        Err(ParseError::new(
            at,
            format!("unterminated raw block (missing {marker})"),
        ))
    }
}

struct Frame {
    at: Location,
    branches: Vec<(Cond, Vec<Node>)>,
    otherwise: Option<Vec<Node>>,
}

impl Frame {
    fn body(&mut self) -> &mut Vec<Node> {
        if let Some(body) = &mut self.otherwise {
            return body;
        }
        // Every frame starts with a condition/body pair.
        &mut self
            .branches
            .last_mut()
            .expect("frame has a first branch")
            .1
    }
}
struct Tokens {
    items: std::vec::IntoIter<Token>,
    end: Location,
    budget: usize,
}

impl Tokens {
    fn peek(&self) -> Option<&Token> {
        self.items.as_slice().first()
    }
    fn at(&self) -> Location {
        self.peek().map_or(self.end, Token::at)
    }

    fn next(&mut self) -> Result<Token, ParseError> {
        self.items
            .next()
            .ok_or_else(|| ParseError::new(self.end, "missing operand"))
    }

    fn eat(&mut self, keyword: &str) -> bool {
        if self.peek().and_then(Token::keyword) != Some(keyword) {
            return false;
        }
        self.items.next();
        true
    }

    fn expect(&mut self, keyword: &str) -> Result<(), ParseError> {
        if self.eat(keyword) {
            Ok(())
        } else {
            Err(ParseError::new(self.at(), format!("expected {keyword:?}")))
        }
    }

    fn word(&mut self) -> Result<Word, ParseError> {
        match self.next()? {
            Token::Word { value, .. } => Ok(value),
            token => Err(ParseError::new(token.at(), "expected a word")),
        }
    }

    fn name(&mut self, alias: bool) -> Result<String, ParseError> {
        let word = self.word()?;
        let valid = word.literal().is_some_and(|name| {
            if alias {
                let mut chars = name.chars();
                matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
                    && chars.all(|c| c == '_' || c == '-' || c.is_ascii_alphanumeric())
            } else {
                identifier(name)
            }
        });
        if !valid {
            return Err(ParseError::new(word.at, "expected a static identifier"));
        }
        match word.parts.into_iter().next() {
            Some(crate::ast::Part::Literal(name)) => Ok(name),
            _ => unreachable!("valid identifiers are nonempty literals"),
        }
    }

    fn command(&mut self) -> Result<Command, ParseError> {
        let program = self.word()?;
        let mut args = Vec::new();
        while self.peek().is_some() {
            args.push(self.word()?);
        }
        Ok(Command { program, args })
    }

    fn finish(self) -> Result<(), ParseError> {
        match self.peek() {
            Some(token) => Err(ParseError::new(token.at(), "unexpected trailing token")),
            None => Ok(()),
        }
    }

    fn condition(&mut self) -> Result<Cond, ParseError> {
        let mut left = self.and()?;
        while self.eat("or") {
            self.spend()?;
            left = Cond::Or(Box::new(left), Box::new(self.and()?));
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Cond, ParseError> {
        let mut left = self.unary()?;
        while self.eat("and") {
            self.spend()?;
            left = Cond::And(Box::new(left), Box::new(self.unary()?));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Cond, ParseError> {
        self.spend()?;
        if self.eat("not") {
            return Ok(Cond::Not(Box::new(self.unary()?)));
        }
        if matches!(self.peek(), Some(Token::Open(_))) {
            self.items.next();
            let cond = self.condition()?;
            match self.next()? {
                Token::Close(_) => return Ok(cond),
                token => return Err(ParseError::new(token.at(), "expected closing parenthesis")),
            }
        }
        let token = self.next()?;
        match token.keyword() {
            Some("true") => Ok(Cond::Constant(true)),
            Some("false") => Ok(Cond::Constant(false)),
            Some("have") => Ok(Cond::Have(self.word()?)),
            Some("exists") => Ok(Cond::Exists(self.word()?)),
            Some("env") => Ok(Cond::Env(self.name(false)?)),
            Some("os" | "shell") => {
                let word = self.word()?;
                let at = word.at;
                match (token.keyword(), word.literal()) {
                    (Some("os"), Some(name)) => Os::parse(name).map(Cond::Os),
                    (Some("shell"), Some(name)) => Shell::parse(name).map(Cond::Shell),
                    _ => None,
                }
                .ok_or_else(|| ParseError::new(at, "unknown or nonliteral target name"))
            }
            _ => Err(ParseError::new(token.at(), "expected a condition")),
        }
    }

    fn spend(&mut self) -> Result<(), ParseError> {
        if self.budget == 0 {
            return Err(ParseError::new(
                self.at(),
                "condition exceeds parser capacity",
            ));
        }
        self.budget -= 1;
        Ok(())
    }
}
