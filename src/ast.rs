//! Semantic values admitted by the frontend.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    pub at: Location,
    pub message: String,
}

impl ParseError {
    pub fn new(at: Location, message: impl Into<String>) -> Self {
        Self {
            at,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "line {}, column {}: {}",
            self.at.line, self.at.column, self.message
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Powershell,
}

impl Shell {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "bash" => Some(Self::Bash),
            "zsh" => Some(Self::Zsh),
            "fish" => Some(Self::Fish),
            "powershell" => Some(Self::Powershell),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
            Self::Powershell => "powershell",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Darwin,
    Windows,
}

impl Os {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "linux" => Some(Self::Linux),
            "darwin" => Some(Self::Darwin),
            "windows" => Some(Self::Windows),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Darwin => "darwin",
            Self::Windows => "windows",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub shell: Shell,
    pub os: Os,
}

impl Target {
    pub fn new(shell: Shell, os: Os) -> Result<Self, String> {
        if os == Os::Windows && shell != Shell::Powershell {
            return Err(format!("unsupported target {}/{}", shell.name(), os.name()));
        }
        Ok(Self { shell, os })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Word {
    pub at: Location,
    pub parts: Vec<Part>,
}

impl Word {
    pub fn literal(&self) -> Option<&str> {
        match self.parts.as_slice() {
            [] => Some(""),
            [Part::Literal(value)] => Some(value),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Part {
    Literal(String),
    Env(String),
    Home,
    TargetShell,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Command {
    pub program: Word,
    pub args: Vec<Word>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Node {
    pub at: Location,
    pub kind: Statement,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Statement {
    Set { name: String, value: Word },
    Unset(String),
    Path { value: Word, direction: PathDir },
    Alias { name: String, command: Command },
    Init(Command),
    If(IfNode),
    Raw { shell: Shell, body: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathDir {
    Prepend,
    Append,
}

#[derive(Debug, PartialEq, Eq)]
pub struct IfNode {
    pub branches: Vec<(Cond, Vec<Node>)>,
    pub otherwise: Vec<Node>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Cond {
    Constant(bool),
    Have(Word),
    Exists(Word),
    Env(String),
    Os(Os),
    Shell(Shell),
    Not(Box<Cond>),
    And(Box<Cond>, Box<Cond>),
    Or(Box<Cond>, Box<Cond>),
}
