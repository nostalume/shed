use super::*;
use std::{
    io::Write,
    process::{Command, Stdio},
};

// Check literal construction independently of environment and native argv adapters.
fn scalar_codec(shell: Shell) {
    let alphabet: Vec<_> = "a '\"`$;\\\n\r\t#(){}[]‘’‚‛🙂".chars().collect();
    let mut values = vec![String::new()];
    let mut level = vec![String::new()];
    for _ in 0..3 {
        level = level
            .iter()
            .flat_map(|prefix| alphabet.iter().map(move |c| format!("{prefix}{c}")))
            .collect();
        values.extend(level.iter().cloned());
    }
    let emitter = Emitter {
        out: String::new(),
        target: Target::new(shell, Os::Linux).unwrap(),
    };
    let mut script = String::from("builtin printf '%s\\000'");
    let mut expected = Vec::new();
    for value in &values {
        script.push(' ');
        script.push_str(&emitter.quote(value));
        expected.extend_from_slice(value.as_bytes());
        expected.push(0);
    }
    script.push('\n');
    let mut command = Command::new(shell.name());
    command.args(match shell {
        Shell::Bash => vec!["--noprofile", "--norc"],
        Shell::Zsh => vec!["-f"],
        _ => vec!["--no-config"],
    });
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the selected Unix shell is required for codec conformance");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success(), "{:?}", result.stderr);
    assert!(result.stderr.is_empty(), "{:?}", result.stderr);
    assert_eq!(result.stdout, expected, "{} scalar codec", shell.name());
}

#[test]
#[cfg_attr(not(unix), ignore = "requires real Bash on Linux/macOS")]
fn bash_scalar_codec() {
    scalar_codec(Shell::Bash);
}

#[test]
#[cfg_attr(not(unix), ignore = "requires real Zsh on Linux/macOS")]
fn zsh_scalar_codec() {
    scalar_codec(Shell::Zsh);
}

#[test]
#[cfg_attr(not(unix), ignore = "requires real Fish on Linux/macOS")]
fn fish_scalar_codec() {
    scalar_codec(Shell::Fish);
}
