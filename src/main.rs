mod ast;
mod emit;
mod parser;
mod prune;
#[cfg(test)]
mod tests;

use std::{
    env, fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process,
};

use ast::Node;
use emit::{Emitter, bash::BashEmitter, fish::FishEmitter, pwsh::PwshEmitter};
use parser::Parser;
use prune::prune_nodes_for_target;

const USAGE: &str = "\
shed — Shell Environment Declaration
compile a single env.shed to any shell dialect

USAGE
  shed [--target-os OS] <shell> [file]
                           compile (reads stdin when file is omitted)
  shed [--target-os OS] check [file]
                           parse only — reports errors or 'ok'

OPTIONS
  --target-os OS          fold `if os` for darwin, linux, or windows
                           (defaults to the compiled binary's target)

SHELLS
  bash   zsh   fish   pwsh

SHELL RC  (write once, never touch again)
  bash / zsh   eval \"$(shed bash ~/.config/shed/env.shed)\"
  fish         shed fish  ~/.config/shed/env.shed | source
  pwsh         shed pwsh  ~/.config/shed/env.shed | Invoke-Expression

DSL REFERENCE
  set   KEY value             export an env var
  path+ dir                   prepend dir to PATH
  path- dir                   append  dir to PATH
  call cmd [args]             eval-init (starship, zoxide, …)
                              use {shell} as a placeholder for the target shell name

  alias name body             define a shell alias

  if    have   <cmd>          guard: command must exist on PATH
  if    exists <path>         guard: path exists on filesystem at shell startup
  if    env    <VAR>          guard: env-var is set and non-empty
  if    os     darwin|linux|windows
  if    shell  bash|zsh|fish|pwsh
  if    not    <cond>         negate a condition
  if    <cond> and <cond>     both conditions must hold
  if    <cond> or  <cond>     either condition must hold
  elif  …
  else
  end

  # comment (inline or full-line)

PATH HANDLING
  Shell variables ($HOME, $env:USERPROFILE, ~, $CARGO_HOME, …) in path+, path-,
  set values, and exists paths are left as-is for the target shell to expand.
  Only the path delimiter is normalised: backslash to forward slash on all platforms so that
  bash/fish/zsh/pwsh all accept the output.
  Relative paths (no variable, no leading /) are resolved against the shed
  file's directory at parse time so the file is portable.

  COMPOUND CONDITIONS (precedence: not > and > or)
  if not have cargo                        negate
  if have cargo and os linux               both must hold
  if os darwin or os linux                 either holds
  if not have nvim and shell bash          not binds tighter than and
  if have cargo or os linux and shell bash and binds tighter than or
";

/// Read source from `path` (a file) or from stdin when `path` is `None`.
/// Errors include the path in the message so the caller can surface it directly.
fn read(path: Option<&str>) -> Result<String, String> {
    match path {
        Some(p) => fs::read_to_string(p).map_err(|e| format!("{}: {}", p, e)),
        None => {
            let mut s = String::new();
            io::stdin()
                .read_to_string(&mut s)
                .map_err(|e| e.to_string())?;
            Ok(s)
        }
    }
}

/// Return the directory of the shed source file as an absolute path,
/// resolving it against the current working directory when necessary.
/// Returns `None` when reading from stdin (no anchor directory).
fn base_dir(file: Option<&str>) -> Option<PathBuf> {
    let parent = Path::new(file?).parent()?;
    Some(
        env::current_dir()
            .map(|cwd| cwd.join(parent))
            .unwrap_or_else(|_| parent.to_path_buf()),
    )
}

/// Render `ast` to a shell-specific string for the named `shell`.
/// Returns an error for unknown shell names.
fn emit(shell: &str, ast: &[Node]) -> Result<String, String> {
    match shell {
        "bash" => Ok(BashEmitter::new("bash").render(ast)),
        "zsh" => Ok(BashEmitter::new("zsh").render(ast)),
        "fish" => Ok(FishEmitter.render(ast)),
        "pwsh" => Ok(PwshEmitter.render(ast)),
        other => Err(format!(
            "unknown shell {:?} — choose: bash, zsh, fish, pwsh",
            other
        )),
    }
}

/// Top-level entry point: parses `args`, reads the source, and either
/// runs the `check` subcommand or compiles and prints the target shell output.
fn run(args: &[String]) -> Result<(), String> {
    // --help / -h anywhere in the args, or bare invocation with no subcommand,
    // prints usage to stdout and exits cleanly (exit 0).
    if args.iter().any(|a| a == "--help" || a == "-h") || args.len() < 2 {
        print!("{}", USAGE);
        return Ok(());
    }

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("shed {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let mut index = 1;
    let mut target_os = None;
    if let Some(arg) = args.get(index) {
        if arg == "--target-os" {
            target_os = Some(
                args.get(index + 1)
                    .ok_or_else(|| format!("--target-os requires a value\n\n{}", USAGE))?
                    .clone(),
            );
            index += 2;
        } else if let Some(value) = arg.strip_prefix("--target-os=") {
            if value.is_empty() {
                return Err(format!("--target-os requires a value\n\n{}", USAGE));
            }
            target_os = Some(value.to_owned());
            index += 1;
        }
    }

    if let Some(os) = target_os.as_deref() {
        if !matches!(os, "darwin" | "linux" | "windows") {
            return Err(format!(
                "unknown target OS {:?} — choose: darwin, linux, windows",
                os
            ));
        }
    }

    let shell = args
        .get(index)
        .map(String::as_str)
        .ok_or_else(|| USAGE.to_owned())?;
    if args.len() > index + 2 {
        return Err(format!(
            "unexpected argument {:?}\n\n{}",
            args[index + 2],
            USAGE
        ));
    }
    let file = args.get(index + 1).map(String::as_str);
    let base = base_dir(file);

    // Parse and resolve paths in one step: Parser::new takes the base dir so
    // `path+` / `path-` tokens are normalised during parsing — no separate pass.
    let parsed =
        read(file).and_then(|src| Parser::new(&src, base).parse().map_err(|e| e.to_string()))?;

    if shell == "check" {
        println!("ok ({} top-level nodes)", parsed.len());
        return Ok(());
    }

    let ast = prune_nodes_for_target(parsed, shell, target_os.as_deref());

    emit(shell, &ast).map(|out| println!("{}", out))
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if let Err(e) = run(&args) {
        eprintln!("shed: {}", e);
        process::exit(1);
    }
}
