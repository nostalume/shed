mod ast;
mod emit;
mod lexer;
mod parser;
mod prune;
#[cfg(test)]
mod tests;

use ast::{Os, Shell, Target};
use std::{
    env, fs,
    io::{self, Read},
    process,
};

const USAGE: &str = "\
shed — Shell Environment Declaration

USAGE
  shed [--target-os OS] check [file]
  shed [--target-os OS] <shell> [file]

Omit file to read stdin. check validates the DSL without executing it.
Shells: bash, zsh, fish, powershell
OS names: linux, darwin, windows

Requires PowerShell 7.5+, Bash 5.2+, or Fish 4.9+. Unix shells target Linux/Darwin.
Fish rejects PATH values containing empty entries before mutation.
See docs/DSL.md for the language and plans/dsl-development.md for progress.
";

fn read(file: Option<&str>) -> Result<String, String> {
    match file {
        Some(path) => fs::read_to_string(path).map_err(|e| format!("{path}: {e}")),
        None => {
            let mut source = String::new();
            io::stdin()
                .read_to_string(&mut source)
                .map_err(|e| e.to_string())?;
            Ok(source)
        }
    }
}

fn run(mut args: impl Iterator<Item = String>) -> Result<String, String> {
    let Some(mut command) = args.next() else {
        return Ok(USAGE.to_owned());
    };
    let mut os = None;
    if command == "--target-os" {
        os = Some(args.next().ok_or("--target-os requires a value")?);
        command = args.next().ok_or("missing command")?;
    } else if let Some(value) = command.strip_prefix("--target-os=") {
        if value.is_empty() {
            return Err("--target-os requires a value".into());
        }
        os = Some(value.to_owned());
        command = args.next().ok_or("missing command")?;
    }
    let os = os
        .as_deref()
        .map(|name| Os::parse(name).ok_or_else(|| format!("unknown target OS {name:?}")))
        .transpose()?;
    if matches!(command.as_str(), "--help" | "-h") {
        return Ok(USAGE.to_owned());
    }
    if matches!(command.as_str(), "--version" | "-V") {
        return Ok(format!("shed {}\n", env!("CARGO_PKG_VERSION")));
    }
    let target = if command == "check" {
        None
    } else {
        let shell = Shell::parse(&command).ok_or_else(|| {
            format!("unknown shell {command:?} — choose: bash, zsh, fish, powershell")
        })?;
        let os = match os {
            Some(os) => os,
            None => match env::consts::OS {
                "linux" => Os::Linux,
                "macos" => Os::Darwin,
                "windows" => Os::Windows,
                _ => return Err("unsupported host OS; specify --target-os".into()),
            },
        };
        Some(Target::new(shell, os)?)
    };
    let file = args.next();
    if let Some(extra) = args.next() {
        return Err(format!("unexpected argument {extra:?}"));
    }
    let source = read(file.as_deref())?;
    let nodes = parser::parse(&source).map_err(|e| match file.as_deref() {
        Some(file) => format!("{file}: {e}"),
        None => e.to_string(),
    })?;
    if let Some(target) = target {
        // check stays target-independent; compilation specializes before emission.
        return emit::generate(nodes, target);
    }
    Ok(format!("ok ({} top-level nodes)\n", nodes.len()))
}

fn main() {
    match run(env::args().skip(1)) {
        Ok(output) => print!("{output}"),
        Err(error) => {
            eprintln!("shed: {error}");
            process::exit(1);
        }
    }
}
