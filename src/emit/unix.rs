//! Bash/Zsh share native syntax; Fish uses the same explicit result/effect flow.
use crate::ast::*;
use std::fmt::Write;

#[cfg(test)]
#[path = "unix_tests.rs"]
mod tests;

pub fn validate(nodes: &[Node], shell: Shell) -> Result<(), String> {
    let reserved = match shell {
        Shell::Bash => {
            "case coproc do done elif else esac fi for function if in select then time until while builtin"
        }
        Shell::Zsh => {
            "do done esac then elif else fi for case if while function repeat time until select coproc nocorrect foreach end declare export float integer local readonly typeset builtin command exec noglob"
        }
        Shell::Fish => {
            "_ and argparse begin break builtin case command continue else end eval exec for function if not or read return set status string switch test time while"
        }
        Shell::Powershell => unreachable!(),
    };
    for node in nodes {
        match &node.kind {
            Statement::Alias { name, .. }
                if name.starts_with("__shed_")
                    || reserved.split_whitespace().any(|keyword| keyword == name) =>
            {
                return Err(format!(
                    "line {}, column {}: reserved {} alias name {name:?}",
                    node.at.line,
                    node.at.column,
                    shell.name()
                ));
            }
            Statement::If(branch) => {
                for (_, body) in &branch.branches {
                    validate(body, shell)?;
                }
                validate(&branch.otherwise, shell)?;
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn generate(nodes: &[Node], target: Target) -> String {
    let mut emitter = Emitter {
        out: if target.shell == Shell::Fish {
            include_str!("unix.fish").to_owned()
        } else {
            include_str!("unix.sh").to_owned()
        },
        target,
    };
    if target.shell == Shell::Bash {
        emitter.out.insert_str(0, r#"if \builtin test "${BASH_VERSINFO[0]}" -lt 5 || { \builtin test "${BASH_VERSINFO[0]}" -eq 5 && \builtin test "${BASH_VERSINFO[1]}" -lt 2; }; then
    \builtin printf '%s\n' 'shed: Bash 5.2+ required' >&2
    \builtin return 1
fi
"#);
    }
    match target.shell {
        Shell::Bash => emitter.out.push_str(
            r#"
__shed_assignable() {
    if \builtin declare -p "$1" >/dev/null 2>&1; then
        case "$(\builtin declare -p "$1")" in
            'declare -- '*|'declare -x '*) \builtin return 0;;
            *) \builtin return 2;;
        esac
    fi
}
__shed_nul_free() { \builtin return 0; }
"#,
        ),
        Shell::Zsh => emitter.out.push_str(
            r#"
__shed_assignable() {
    case "${(tP)1}" in
        *readonly*|*nameref*) \builtin return 2;;
        ''|scalar|scalar-*) \builtin return 0;;
        *) \builtin return 2;;
    esac
}
__shed_nul_free() {
    while \builtin test "$#" -ne 0; do
        case "$1" in *$'\0'*) \builtin return 2;; esac
        \builtin shift
    done
}
"#,
        ),
        _ => {}
    }
    emitter.statements(nodes);
    emitter.line("\\builtin true", "builtin true");
    emitter.out
}

struct Emitter {
    out: String,
    target: Target,
}

impl Emitter {
    fn fish(&self) -> bool {
        self.target.shell == Shell::Fish
    }
    fn quote(&self, text: &str) -> String {
        if self.fish() {
            format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
        } else {
            format!("'{}'", text.replace('\'', "'\\''"))
        }
    }
    fn line(&mut self, sh: &str, fish: &str) {
        self.out.push_str(if self.fish() { fish } else { sh });
        self.out.push('\n');
    }
    fn open_function(&mut self, name: &str) {
        if self.fish() {
            writeln!(self.out, "function {name}").unwrap();
        } else {
            writeln!(self.out, "function {name} {{").unwrap();
        }
    }
    fn end_function(&mut self) {
        self.line("}", "end");
    }
    fn result(&mut self, value: &str) {
        if self.fish() {
            writeln!(self.out, "builtin set argv {value}").unwrap();
        } else {
            writeln!(self.out, "\\builtin set -- {value}").unwrap();
        }
    }
    fn decoded(&self, n: usize) -> String {
        if self.fish() {
            format!("\"$argv[{}]\"", n * 2 - 1)
        } else {
            format!("\"${{{n}%?}}\"")
        }
    }
    fn tail(&self, n: usize) -> String {
        if self.fish() {
            format!("$argv[{}..-1]", n * 2 - 1)
        } else {
            format!("\"${{@:{n}}}\"")
        }
    }
    fn return_value(&mut self, n: u8) {
        writeln!(
            self.out,
            "{}return {n}",
            if self.fish() {
                "builtin "
            } else {
                "\\builtin "
            }
        )
        .unwrap();
    }
    fn failure(&mut self, at: Location) {
        writeln!(
            self.out,
            "{}printf '%s\\n' {} >&2",
            if self.fish() {
                "builtin "
            } else {
                "\\builtin "
            },
            self.quote(&format!(
                "shed at {}:{}: runtime operation failed",
                at.line, at.column
            ))
        )
        .unwrap();
        self.return_value(1);
    }
    fn open_if(&mut self, call: &str) {
        if self.fish() {
            writeln!(self.out, "if {call}").unwrap();
        } else {
            writeln!(self.out, "if {call}; then").unwrap();
        }
    }
    fn close_if(&mut self) {
        self.line("fi", "end");
    }
    fn test_result(&self, n: u8) -> String {
        if self.fish() {
            format!("builtin test \"$argv[1]\" -eq {n}")
        } else {
            format!("\\builtin test \"$1\" -eq {n}")
        }
    }
    fn check_call(&mut self, call: &str, at: Location) {
        self.open_if(call);
        self.line("\\builtin true", "builtin true");
        self.out.push_str("else\n");
        self.failure(at);
        self.close_if();
    }
    fn push_word(&mut self, word: &Word, init: bool, condition: bool) {
        let encoded = if let Some(text) = word.literal() {
            if self.fish() {
                format!("{} 0", self.quote(text))
            } else {
                self.quote(&format!("{text}0"))
            }
        } else {
            let mut call = String::from("__shed_capture __shed_word");
            for part in &word.parts {
                let (kind, text) = match part {
                    Part::Literal(text) => ("l", text.as_str()),
                    Part::Env(name) => ("e", name.as_str()),
                    Part::Home => ("h", ""),
                    Part::TargetShell => ("l", self.target.shell.name()),
                };
                write!(call, " {kind} {}", self.quote(text)).unwrap();
            }
            if self.fish() {
                format!("({call} | builtin string split0)")
            } else {
                format!("\"$({call})\"")
            }
        };
        if self.fish() {
            writeln!(
                self.out,
                "builtin set argv {encoded} $argv\nif not builtin test \"$argv[2]\" -eq 0"
            )
            .unwrap();
        } else {
            writeln!(
                self.out,
                "\\builtin set -- {encoded} \"$@\"\ncase \"$1\" in *0) ;; *)"
            )
            .unwrap();
        }
        if init {
            self.line(
                "\\builtin printf 'return 1\\n'",
                "builtin printf 'return 1\\n'",
            );
        }
        if condition {
            self.return_value(2);
        } else {
            self.failure(word.at);
        }
        self.line(";; esac", "end");
    }
    fn command_body(&mut self, command: &Command, init: bool, caller: bool) {
        let values: Option<Vec<_>> = std::iter::once(&command.program)
            .chain(&command.args)
            .map(|value| value.literal().map(|text| self.quote(text)))
            .collect();
        if let Some(values) = values {
            writeln!(
                self.out,
                "{} {}{}",
                if init { "__shed_init" } else { "__shed_run" },
                values.join(" "),
                if caller {
                    format!(" {}", self.tail(1))
                } else {
                    String::new()
                }
            )
            .unwrap();
            return;
        }
        for value in std::iter::once(&command.program).chain(&command.args) {
            self.push_word(value, init, false);
        }
        let n = command.args.len() + 1;
        let values = (1..=n)
            .rev()
            .map(|i| self.decoded(i))
            .collect::<Vec<_>>()
            .join(" ");
        writeln!(
            self.out,
            "{} {values}{}",
            if init { "__shed_init" } else { "__shed_run" },
            if caller {
                format!(" {}", self.tail(n + 1))
            } else {
                String::new()
            }
        )
        .unwrap();
    }
    fn condition(&mut self, cond: &Cond) {
        match cond {
            Cond::Constant(value) => self.result(if *value { "0" } else { "1" }),
            Cond::Not(inner) => {
                self.condition(inner);
                self.open_if(&self.test_result(0));
                self.result("1");
                self.out.push_str("else\n");
                self.open_if(&self.test_result(1));
                self.result("0");
                self.close_if();
                self.close_if();
            }
            Cond::And(left, right) | Cond::Or(left, right) => {
                self.condition(left);
                self.open_if(&self.test_result(if matches!(cond, Cond::And(..)) { 0 } else { 1 }));
                self.condition(right);
                self.close_if();
            }
            Cond::Env(name) => self.query(&format!("__shed_env_truth {}", self.quote(name))),
            Cond::Have(value) | Cond::Exists(value) => {
                self.push_word(value, false, true);
                self.query(&format!(
                    "{} {}",
                    if matches!(cond, Cond::Have(_)) {
                        "__shed_have"
                    } else {
                        "__shed_exists"
                    },
                    self.decoded(1)
                ));
            }
            Cond::Os(_) | Cond::Shell(_) => unreachable!("specialization removes target leaves"),
        }
    }
    fn query(&mut self, call: &str) {
        self.open_if(call);
        self.result("0");
        self.out.push_str("else\n");
        self.result(if self.fish() { "$status" } else { "\"$?\"" });
        self.close_if();
    }
    fn branches(&mut self, branches: &[(Cond, Vec<Node>)], otherwise: &[Node], at: Location) {
        if branches.is_empty() {
            self.statements(otherwise);
            return;
        }
        // A one-pass block also avoids native parsers' recursive elif grammar.
        self.line("while \\builtin true; do", "while builtin true");
        for (condition, body) in branches {
            self.open_function("__shed_condition");
            self.condition(condition);
            self.line("\\builtin return \"$1\"", "builtin return $argv[1]");
            self.end_function();
            self.open_if("__shed_condition");
            self.statements(body);
            self.line("\\builtin break", "builtin break");
            self.out.push_str("else\n");
            self.open_if(if self.fish() {
                "builtin test $status -ne 1"
            } else {
                "\\builtin test \"$?\" -ne 1"
            });
            self.failure(at);
            self.close_if();
            self.close_if();
        }
        self.statements(otherwise);
        self.line("\\builtin break", "builtin break");
        self.line("done", "end");
    }
    fn code_unit(&mut self, producer: &str, at: Location) {
        let call = if self.fish() {
            format!("{producer} | builtin source")
        } else {
            format!("\\builtin source <({producer})")
        };
        self.check_call(&call, at);
    }
    fn statements(&mut self, nodes: &[Node]) {
        if nodes.is_empty() {
            self.line("\\builtin true", "builtin true");
        }
        for node in nodes {
            match &node.kind {
                Statement::If(branch) => {
                    self.branches(&branch.branches, &branch.otherwise, node.at)
                }
                Statement::Raw { body, .. } => self.code_unit(
                    &format!(
                        "{}printf '%s' {}",
                        if self.fish() {
                            "builtin "
                        } else {
                            "\\builtin "
                        },
                        self.quote(body)
                    ),
                    node.at,
                ),
                Statement::Alias { name, command } => {
                    if !self.fish() {
                        writeln!(
                            self.out,
                            "\\builtin unalias {} 2>/dev/null || \\builtin true",
                            self.quote(name)
                        )
                        .unwrap();
                    }
                    self.line("if", "if begin");
                    self.open_function(name);
                    self.command_body(command, false, true);
                    self.end_function();
                    self.line("then", "end");
                    self.line("\\builtin true", "builtin true");
                    self.out.push_str("else\n");
                    self.failure(node.at);
                    self.close_if();
                }
                Statement::Init(command) => {
                    self.open_function("__shed_initializer");
                    self.command_body(command, true, false);
                    self.end_function();
                    self.code_unit("__shed_initializer", node.at);
                }
                Statement::Set { value, .. } | Statement::Path { value, .. } => {
                    self.open_function("__shed_statement");
                    self.push_word(value, false, false);
                    let call = match &node.kind {
                        Statement::Set { name, .. } => {
                            format!("__shed_set {} {}", self.quote(name), self.decoded(1))
                        }
                        Statement::Path { direction, .. } => format!(
                            "__shed_path {} {}",
                            self.decoded(1),
                            if *direction == PathDir::Prepend {
                                "prepend"
                            } else {
                                "append"
                            }
                        ),
                        _ => unreachable!(),
                    };
                    self.out.push_str(&call);
                    self.out.push('\n');
                    self.end_function();
                    self.check_call("__shed_statement", node.at);
                }
                Statement::Unset(name) => {
                    let call = format!("__shed_unset {}", self.quote(name));
                    self.check_call(&call, node.at);
                }
            }
        }
    }
}
