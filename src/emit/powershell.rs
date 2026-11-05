//! Emit semantic values, never source words or shell command strings.
use crate::ast::*;
use std::fmt::Write;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub fn generate(nodes: &[Node], target: Target) -> String {
    let mut out = include_str!("../powershell.ps1").to_owned();
    out.push_str(
        r#"
$__shed_preferences = @($ErrorActionPreference, $PSNativeCommandUseErrorActionPreference)
try {
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
"#,
    );
    statements(&mut out, nodes, target);
    out.push_str(r#"$null = $null
} catch {
throw [System.InvalidOperationException]::new("shed at ${__shed_at}: $($_.Exception.Message)", $_.Exception)
} finally {
$ErrorActionPreference = $__shed_preferences[0]
$PSNativeCommandUseErrorActionPreference = $__shed_preferences[1]
}
"#);
    out
}

pub fn validate(nodes: &[Node]) -> Result<(), String> {
    for node in nodes {
        match &node.kind {
            Statement::Alias { name, .. } => {
                // Keywords need expression invocation, and runtime helpers must remain private.
                let lower = name.to_ascii_lowercase();
                if lower.starts_with("__shed_")
                    || matches!(
                        lower.as_str(),
                        "begin"
                            | "break"
                            | "catch"
                            | "class"
                            | "clean"
                            | "continue"
                            | "data"
                            | "define"
                            | "do"
                            | "dynamicparam"
                            | "else"
                            | "elseif"
                            | "end"
                            | "enum"
                            | "exit"
                            | "filter"
                            | "finally"
                            | "for"
                            | "foreach"
                            | "from"
                            | "function"
                            | "if"
                            | "in"
                            | "inlinescript"
                            | "parallel"
                            | "param"
                            | "process"
                            | "return"
                            | "sequence"
                            | "switch"
                            | "throw"
                            | "trap"
                            | "try"
                            | "until"
                            | "using"
                            | "var"
                            | "while"
                            | "workflow"
                    )
                {
                    return Err(format!(
                        "line {}, column {}: reserved PowerShell alias name {name:?}",
                        node.at.line, node.at.column
                    ));
                }
            }
            Statement::If(branch) => {
                for (_, body) in &branch.branches {
                    validate(body)?;
                }
                validate(&branch.otherwise)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn literal(value: &str) -> String {
    // PowerShell also recognizes Unicode single quotes as delimiters. Use numeric
    // UTF-16 only for those strings; ordinary data uses compact single quoting.
    if value.contains(['\u{2018}', '\u{2019}', '\u{201a}', '\u{201b}']) {
        let mut out = String::from("[string]::new([char[]]@(");
        for (i, unit) in value.encode_utf16().enumerate() {
            if i != 0 {
                out.push(',');
            }
            write!(out, "0x{unit:04x}").unwrap();
        }
        out.push_str("))");
        out
    } else {
        format!("'{}'", value.replace('\'', "''"))
    }
}

fn word(word: &Word, target: Target) -> String {
    let mut parts = vec!["''".to_owned()];
    for part in &word.parts {
        parts.push(match part {
            Part::Literal(text) => literal(text),
            Part::Env(name) => format!("(__shed_env {})", literal(name)),
            Part::Home => "(__shed_home)".into(),
            Part::TargetShell => literal(target.shell.name()),
        });
    }
    format!("({})", parts.join(" + "))
}

fn condition(cond: &Cond, target: Target) -> String {
    match cond {
        Cond::Constant(value) => format!("${value}"),
        Cond::Env(name) => format!("((__shed_env {}).Length -ne 0)", literal(name)),
        Cond::Have(value) => format!("($null -ne (__shed_resolve {}))", word(value, target)),
        Cond::Exists(value) => format!("(__shed_exists {})", word(value, target)),
        Cond::Not(inner) => format!("(-not {})", condition(inner, target)),
        Cond::And(lhs, rhs) => format!(
            "({} -and {})",
            condition(lhs, target),
            condition(rhs, target)
        ),
        Cond::Or(lhs, rhs) => format!(
            "({} -or {})",
            condition(lhs, target),
            condition(rhs, target)
        ),
        Cond::Os(_) | Cond::Shell(_) => unreachable!("specialization removes target leaves"),
    }
}

fn arguments(command: &Command, target: Target) -> String {
    let values = std::iter::once(&command.program)
        .chain(&command.args)
        .map(|v| word(v, target))
        .collect::<Vec<_>>();
    format!("([string[]]@({}))", values.join("; "))
}

fn code_unit(out: &mut String, value: &str) {
    writeln!(
        out,
        ". ([scriptblock]::Create({value}))\nif (-not $?) {{ throw 'code unit failed' }}"
    )
    .unwrap();
}

fn statements(out: &mut String, nodes: &[Node], target: Target) {
    for node in nodes {
        writeln!(out, "$__shed_at = '{}:{}'", node.at.line, node.at.column).unwrap();
        match &node.kind {
            Statement::Set { name, value } => {
                writeln!(
                    out,
                    "[Environment]::SetEnvironmentVariable({}, {})",
                    literal(name),
                    word(value, target)
                )
                .unwrap();
            }
            Statement::Unset(name) => {
                writeln!(
                    out,
                    "if ($null -ne [Environment]::GetEnvironmentVariable({})) {{",
                    literal(name)
                )
                .unwrap();
                writeln!(out, "Microsoft.PowerShell.Management\\Remove-Item -LiteralPath {} -ErrorAction Stop\n}}", literal(&format!("Env:{name}"))).unwrap();
            }
            Statement::Path { value, direction } => {
                let delimiter = if target.os == Os::Windows { ';' } else { ':' };
                writeln!(
                    out,
                    "__shed_path {} ${} '{delimiter}'",
                    word(value, target),
                    matches!(direction, PathDir::Prepend)
                )
                .unwrap();
            }
            Statement::Alias { name, command } => {
                let wrapper = format!("__shed_alias_{name}");
                writeln!(out, "Microsoft.PowerShell.Management\\Set-Item -LiteralPath {} -ErrorAction Stop -Value {{", literal(&format!("Function:global:{wrapper}"))).unwrap();
                out.push_str("$__shed_call = [Collections.Generic.List[string]]::new()\n");
                writeln!(out, "$__shed_call.AddRange({})", arguments(command, target)).unwrap();
                out.push_str(r#"foreach ($__shed_arg in $args) { $__shed_call.Add((__shed_text ([string]$__shed_arg))) }
$__shed_program = __shed_resolve $__shed_call[0]
if ($null -eq $__shed_program) { throw "external executable not found: $($__shed_call[0])" }
$__shed_tail = $__shed_call.GetRange(1, $__shed_call.Count - 1).ToArray()
$PSNativeCommandArgumentPassing = 'Standard'
$PSNativeCommandUseErrorActionPreference = $false
& $__shed_program @__shed_tail
if ($LASTEXITCODE -ne 0) { throw "native command exited with status $LASTEXITCODE" }
}
"#);
                writeln!(out, "Microsoft.PowerShell.Utility\\Set-Alias -Name {} -Value {} -Scope Global -Option AllScope -Force -ErrorAction Stop", literal(name), literal(&wrapper)).unwrap();
            }
            Statement::Init(command) => {
                writeln!(
                    out,
                    "$__shed_code = __shed_init {}",
                    arguments(command, target)
                )
                .unwrap();
                code_unit(out, "$__shed_code");
            }
            Statement::Raw { body, .. } => code_unit(out, &literal(body)),
            Statement::If(branch) => {
                for (i, (cond, body)) in branch.branches.iter().enumerate() {
                    writeln!(
                        out,
                        "{} ({}) {{",
                        if i == 0 { "if" } else { "elseif" },
                        condition(cond, target)
                    )
                    .unwrap();
                    statements(out, body, target);
                    out.push_str("}\n");
                }
                if !branch.otherwise.is_empty() {
                    out.push_str("else {\n");
                    statements(out, &branch.otherwise, target);
                    out.push_str("}\n");
                }
            }
        }
    }
}
