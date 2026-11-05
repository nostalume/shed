use crate::ast::{Location, ParseError, Part, Word};
use std::iter::Peekable;
use std::str::Chars;

pub(crate) enum Token {
    Word { value: Word, bare: bool },
    Open(Location),
    Close(Location),
}

impl Token {
    pub fn at(&self) -> Location {
        match self {
            Self::Word { value, .. } => value.at,
            Self::Open(at) | Self::Close(at) => *at,
        }
    }

    pub fn keyword(&self) -> Option<&str> {
        match self {
            Self::Word { value, bare: true } => value.literal(),
            _ => None,
        }
    }
}

pub(crate) fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

pub(crate) fn lex(text: &str, line: usize) -> Result<Vec<Token>, ParseError> {
    let mut scanner = Scanner {
        chars: text.chars().peekable(),
        at: Location { line, column: 1 },
    };
    let mut tokens = Vec::new();
    let mut comment_boundary = true;
    while let Some(&ch) = scanner.chars.peek() {
        match ch {
            ' ' | '\t' => {
                scanner.bump();
                comment_boundary = true;
            }
            '#' if comment_boundary => break,
            '(' => {
                tokens.push(Token::Open(scanner.at));
                scanner.bump();
                comment_boundary = false;
            }
            ')' => {
                tokens.push(Token::Close(scanner.at));
                scanner.bump();
                comment_boundary = false;
            }
            _ => {
                tokens.push(scanner.word()?);
                comment_boundary = false;
            }
        }
    }
    Ok(tokens)
}

struct Scanner<'a> {
    chars: Peekable<Chars<'a>>,
    at: Location,
}

fn flush(parts: &mut Vec<Part>, text: &mut String) {
    if !text.is_empty() {
        parts.push(Part::Literal(std::mem::take(text)));
    }
}

impl Scanner<'_> {
    fn bump(&mut self) -> Option<char> {
        let ch = self.chars.next()?;
        self.at.column += 1;
        Some(ch)
    }

    fn word(&mut self) -> Result<Token, ParseError> {
        let at = self.at;
        let mut parts = Vec::new();
        let mut text = String::new();
        let mut bare = true;
        while let Some(&ch) = self.chars.peek() {
            match ch {
                ' ' | '\t' | '(' | ')' => break,
                '\'' | '"' => {
                    bare = false;
                    self.quoted(ch, &mut parts, &mut text)?;
                }
                '$' | '\u{0060}' | ';' | '&' | '|' | '<' | '>' | '{' | '}' | '[' | ']' | '\r'
                | '\n' => {
                    return Err(ParseError::new(
                        self.at,
                        format!("quote reserved character {ch:?}"),
                    ));
                }
                _ => {
                    text.push(ch);
                    self.bump();
                }
            }
        }
        flush(&mut parts, &mut text);
        Ok(Token::Word {
            value: Word { at, parts },
            bare,
        })
    }

    fn quoted(
        &mut self,
        quote: char,
        parts: &mut Vec<Part>,
        text: &mut String,
    ) -> Result<(), ParseError> {
        let opening = self.at;
        self.bump();
        while let Some(ch) = self.bump() {
            if ch == quote {
                return Ok(());
            }
            match ch {
                '\\' if quote == '"' => {
                    let at = Location {
                        column: self.at.column - 1,
                        ..self.at
                    };
                    let escaped = match self.bump() {
                        Some('\\') => '\\',
                        Some('"') => '"',
                        Some('$') => '$',
                        Some('n') => '\n',
                        Some('r') => '\r',
                        Some('t') => '\t',
                        _ => return Err(ParseError::new(at, "invalid double-quote escape")),
                    };
                    text.push(escaped);
                }
                '$' if quote == '"' => {
                    let at = Location {
                        column: self.at.column - 1,
                        ..self.at
                    };
                    if self.bump() != Some('{') {
                        return Err(ParseError::new(
                            at,
                            "expected an expansion such as ${env:NAME}",
                        ));
                    }
                    let mut name = String::new();
                    loop {
                        match self.bump() {
                            Some('}') => break,
                            Some(c) => name.push(c),
                            None => return Err(ParseError::new(at, "unterminated expansion")),
                        }
                    }
                    let part = match name.as_str() {
                        "home" => Part::Home,
                        "shell" => Part::TargetShell,
                        _ => match name.strip_prefix("env:").filter(|name| identifier(name)) {
                            Some(name) => Part::Env(name.to_owned()),
                            None => {
                                return Err(ParseError::new(
                                    at,
                                    format!("unknown expansion {name:?}"),
                                ));
                            }
                        },
                    };
                    flush(parts, text);
                    parts.push(part);
                }
                '\r' | '\n' => return Err(ParseError::new(opening, "newline inside quoted word")),
                _ => text.push(ch),
            }
        }
        Err(ParseError::new(opening, "unterminated quote"))
    }
}
