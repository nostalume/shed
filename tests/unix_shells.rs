//! The same DSL cases and native witness run in each real Unix shell.
//! Windows reports these tests as ignored; it does not certify Unix adapters.
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Session {
    root: PathBuf,
    shell: &'static str,
    executable: &'static str,
}
impl Session {
    fn new(shell: &'static str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "shed-unix-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("captures")).unwrap();
        // Match the physical cwd used by Unix path operations (e.g. Darwin /var).
        let root = fs::canonicalize(root).unwrap();
        let out = Command::new("rustc")
            .args([
                "--edition=2024",
                concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/native.rs"),
                "-o",
            ])
            .arg(root.join("native_fixture"))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        Self {
            root,
            shell,
            executable: shell,
        }
    }
    fn run(
        &self,
        source: &str,
        success: bool,
        post: &str,
        names: &[&str],
    ) -> (Vec<Option<String>>, Vec<u8>) {
        let os = if cfg!(target_os = "macos") {
            "darwin"
        } else {
            "linux"
        };
        let mut compile = Command::new(env!("CARGO_BIN_EXE_shed"))
            .args(["--target-os", os, self.shell])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        compile
            .stdin
            .take()
            .unwrap()
            .write_all(source.as_bytes())
            .unwrap();
        let result = compile.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        fs::write(self.root.join("generated"), result.stdout).unwrap();
        let (yes, no) = if success {
            ("true", "exit 81")
        } else {
            ("exit 82", "true")
        };
        let driver = if self.shell == "fish" {
            format!(
                "builtin set -g --unexport LOCAL_ONLY shell-local\nif builtin source ./generated\n{yes}\nelse\n{no}\nend\n{post}\ncommand ./native_fixture env ./snapshot {}\n",
                names.join(" ")
            )
        } else {
            format!(
                "LOCAL_ONLY=shell-local\nif \\builtin source ./generated; then {yes}; else {no}; fi\n{post}\n\\builtin command ./native_fixture env ./snapshot {}\n",
                names.join(" ")
            )
        };
        fs::write(self.root.join("driver"), driver).unwrap();
        let mut cmd = Command::new(self.executable);
        cmd.args(match self.shell {
            "bash" => vec!["--noprofile", "--norc"],
            "zsh" => vec!["-f"],
            _ => vec!["--no-config"],
        });
        let out = cmd
            .arg("./driver")
            .current_dir(&self.root)
            .env("ROOT", &self.root)
            .env("TMPDIR", self.root.join("captures"))
            .env("SHED_NATIVE_MARKER", self.root.join("marker"))
            .env("CALLER", "'\"`$;|&<>[]*?#‘🙂’\r\n")
            .env_remove("BEFORE")
            .env_remove("AFTER")
            .env_remove("EXECUTED")
            .env_remove("LOCAL_ONLY")
            .output()
            .unwrap_or_else(|e| panic!("required shell {} unavailable: {e}", self.shell));
        assert!(
            out.status.success(),
            "{}: stdout={} stderr={}\nsource:\n{}",
            self.shell,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr).trim_start_matches('!'),
            source.chars().take(1600).collect::<String>()
        );
        assert!(out.stdout.is_empty(), "{}: {:?}", self.shell, out.stdout);
        let bytes = fs::read(self.root.join("snapshot")).unwrap();
        let mut rest = bytes.as_slice();
        let mut env = Vec::new();
        for _ in names {
            let present = rest[0];
            rest = &rest[1..];
            env.push(if present == 0 {
                None
            } else {
                Some(take_string(&mut rest))
            });
        }
        assert!(rest.is_empty());
        assert_eq!(
            fs::read_dir(self.root.join("captures")).unwrap().count(),
            0,
            "initializer capture leaked"
        );
        (env, out.stderr)
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn take_string(rest: &mut &[u8]) -> String {
    let len = u64::from_le_bytes(rest[..8].try_into().unwrap()) as usize;
    let result = String::from_utf8(rest[8..8 + len].to_vec()).unwrap();
    *rest = &rest[8 + len..];
    result
}
fn arguments(file: PathBuf) -> Vec<String> {
    let bytes = fs::read(file).unwrap();
    let mut rest = bytes.as_slice();
    let mut values = Vec::new();
    while !rest.is_empty() {
        values.push(take_string(&mut rest));
    }
    values
}
fn quote(text: &str) -> String {
    format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('$', "\\$")
            .replace('\r', "\\r")
            .replace('\n', "\\n")
            .replace('\t', "\\t")
    )
}

fn values_paths_queries_and_precedence(shell: &'static str) {
    let s = Session::new(shell);
    fs::write(s.root.join("[literal]"), "").unwrap();
    let value = "a '\"`$;|&<>[]*?#‘’‚‛🙂\r\n\t\\end\\";
    let gap = if shell == "fish" { ":.:" } else { "::" };
    let mut source = format!(
        "set VALUE {}\nset EMPTY ''\nset GONE gone\nunset GONE\nset COPY \"${{env:LOCAL_ONLY}}\"\nset SHELL \"${{shell}}\"\nset PATH \"old{gap}${{env:ROOT}}/missing:OLD\"\npath+ missing\npath+ missing\npath- missing\npath- missing\nif have native_fixture and not have cd and not have 'native_*' and exists '[literal]' and not exists '*'\nset QUERIES yes\nend\n",
        quote(value)
    );
    let mut expected = vec![
        Some(value.into()),
        Some("".into()),
        None,
        Some("".into()),
        Some(shell.into()),
        Some(format!("old{gap}OLD:{}/missing", s.root.display())),
        Some("yes".into()),
    ];
    let mut names = vec!["VALUE", "EMPTY", "GONE", "COPY", "SHELL", "PATH", "QUERIES"];
    let result_names: Vec<_> = (0..8).map(|i| format!("BOOL{i}")).collect();
    for (i, name) in result_names.iter().enumerate() {
        let (a, b, c) = (i & 1 != 0, i & 2 != 0, i & 4 != 0);
        source.push_str(&format!("set A {}\nset B {}\nset C {}\nif env A or env B and env C\nset {name} yes\nelse\nset {name} no\nend\n",quote(if a{"yes"}else{""}),quote(if b{"yes"}else{""}),quote(if c{"yes"}else{""})));
        names.push(name);
        expected.push(Some(if a || (b && c) { "yes" } else { "no" }.into()));
    }
    let post = if shell == "fish" {
        "builtin test ! -e ./marker; or exit 83"
    } else {
        "\\builtin test ! -e ./marker || exit 83"
    };
    let (values, stderr) = s.run(&source, true, post, &names);
    assert_eq!(values, expected, "{shell}");
    assert!(stderr.is_empty(), "{shell}: {:?}", stderr);
    for (start, expected) in [
        ("unset PATH", format!("{}/missing", s.root.display())),
        ("set PATH ''", format!("{}/missing:", s.root.display())),
    ] {
        if shell == "fish" && start == "set PATH ''" {
            continue; // Tested as an explicit representability error below.
        }
        let (values, stderr) = s.run(&format!("{start}\npath+ missing\n"), true, "", &["PATH"]);
        assert_eq!(values, [Some(expected)], "{shell}");
        assert!(stderr.is_empty());
    }
}

fn structured_calls_and_code_unit_scope(shell: &'static str) {
    let s = Session::new(shell);
    fs::create_dir(s.root.join("other")).unwrap();
    fs::copy(
        s.root.join("native_fixture"),
        s.root.join("other/native_fixture"),
    )
    .unwrap();
    let fixed = ["", "a b", "'\"`$;|&<>[]*?#‘🙂’", "\r\n\\"];
    let args = fixed.iter().map(|s| quote(s)).collect::<Vec<_>>().join(" ");
    let body = if shell == "fish" {
        "function Persisted\nbuiltin printf persisted\nend\n\n\n"
    } else {
        "function Persisted { \\builtin printf persisted; }\n\n\n"
    };
    fs::write(s.root.join("code"), body).unwrap();
    let source = format!(
        "set PATH \"${{env:ROOT}}\"\nalias native_fixture = native_fixture argv \"${{env:ROOT}}/args\" \"${{env:LATE}}\" {args}\nalias where = native_fixture where \"${{env:ROOT}}/location\"\nalias noargs = \"${{env:ROOT}}/native_fixture\"\ninit \"${{env:ROOT}}/native_fixture\" argv \"${{env:ROOT}}/init-args\" {args}\ninit \"${{env:ROOT}}/native_fixture\" emit \"${{env:ROOT}}/code\" 0\nset AFTER yes\n"
    );
    let source = format!(
        "raw {shell} <<CODE\nbuiltin printf '%s' 'line1\r\nline2\n\n' > ./raw-bytes\n\nCODE\n{source}"
    );
    let post = if shell == "fish" {
        "builtin set -gx LATE invocation\nnative_fixture '' --caller \"$CALLER\"; or exit 84\nnoargs; or exit 85\nbuiltin set -gx --path PATH \"$ROOT/other\"\nwhere; or exit 86\nPersisted > ./persisted"
    } else {
        "\\builtin export LATE=invocation\nnative_fixture '' --caller \"$CALLER\" || exit 84\nnoargs || exit 85\n\\builtin export PATH=\"$ROOT/other\"\nwhere || exit 86\nPersisted > ./persisted"
    };
    let (values, stderr) = s.run(&source, true, post, &["AFTER"]);
    assert_eq!(values, [Some("yes".into())]);
    assert!(
        stderr.iter().all(|b| *b == b'!'),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&stderr).trim_start_matches('!')
    );
    assert_eq!(stderr.len(), 131_072, "initializer stderr length");
    let mut expected = vec!["invocation".to_owned()];
    expected.extend(fixed.map(str::to_owned));
    expected.extend(["", "--caller", "'\"`$;|&<>[]*?#‘🙂’\r\n"].map(str::to_owned));
    assert_eq!(arguments(s.root.join("args")), expected, "{shell}");
    assert_eq!(arguments(s.root.join("init-args")), fixed, "{shell}");
    assert_eq!(fs::read(s.root.join("persisted")).unwrap(), b"persisted");
    assert_eq!(
        fs::read(s.root.join("raw-bytes")).unwrap(),
        b"line1\r\nline2\n\n"
    );
    assert_eq!(
        fs::read_to_string(s.root.join("location")).unwrap(),
        s.root.join("other/native_fixture").to_str().unwrap()
    );
}

fn scalar_and_native_argument_roundtrip(shell: &'static str) {
    let s = Session::new(shell);
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
    let args = values
        .iter()
        .map(|value| quote(value))
        .collect::<Vec<_>>()
        .join(" ");
    let source = format!(
        "alias scalars = {} argv {} {args}\n",
        quote(s.root.join("native_fixture").to_str().unwrap()),
        quote(s.root.join("scalars").to_str().unwrap())
    );
    let (_, stderr) = s.run(&source, true, "scalars", &[]);
    assert!(stderr.is_empty());
    assert_eq!(arguments(s.root.join("scalars")), values, "{shell}");
}

fn failures_propagate_without_evaluating_failed_output(shell: &'static str) {
    let s = Session::new(shell);
    let bad = if shell == "fish" {
        "builtin set -gx EXECUTED wrong\n"
    } else {
        "\\builtin export EXECUTED=wrong\n"
    };
    fs::write(s.root.join("code"), bad).unwrap();
    fs::write(s.root.join("invalid"), [bad.as_bytes(), &[0xff]].concat()).unwrap();
    fs::write(s.root.join("with-nul"), [bad.as_bytes(), &[0]].concat()).unwrap();
    let invalid_syntax = if shell == "fish" { "end" } else { "}" };
    let operations = [
        "path+ ''".to_owned(),
        "set HOME relative\nif exists \"${home}\" and false\nend".to_owned(),
        format!("raw {shell} <<CODE\n{invalid_syntax}\nCODE"),
        "init \"${env:ROOT}/native_fixture\" emit \"${env:ROOT}/code\" 7".to_owned(),
        "init \"${env:ROOT}/native_fixture\" emit \"${env:ROOT}/invalid\" 0".to_owned(),
        "init \"${env:ROOT}/native_fixture\" emit \"${env:ROOT}/with-nul\" 0".to_owned(),
    ];
    for operation in operations {
        let (values, stderr) = s.run(
            &format!("set BEFORE kept\n{operation}\nset AFTER wrong\n"),
            false,
            "",
            &["BEFORE", "AFTER", "EXECUTED"],
        );
        assert_eq!(
            values,
            [Some("kept".into()), None, None],
            "{shell}: {operation}"
        );
        assert!(!stderr.is_empty());
    }
    // Both short-circuit directions skip an invalid home operand.
    let source = "set HOME relative\nset YES yes\nset NO ''\nif env YES or exists \"${home}\"\nend\nif env NO and exists \"${home}\"\nend\n";
    let (_, stderr) = s.run(source, true, "", &[]);
    assert!(stderr.is_empty());
}

fn conformance(shell: &'static str) {
    values_paths_queries_and_precedence(shell);
    structured_calls_and_code_unit_scope(shell);
    failures_propagate_without_evaluating_failed_output(shell);
    scalar_and_native_argument_roundtrip(shell);
    if shell == "fish" {
        let s = Session::new(shell);
        for value in ["", ":old", "old:", "old::new"] {
            let source = format!(
                "set BEFORE kept\nset SAVED_PATH \"${{env:PATH}}\"\nset PATH {}\nset AFTER wrong\n",
                quote(value)
            );
            let (values, stderr) = s.run(
                &source,
                false,
                "",
                &["BEFORE", "AFTER", "PATH", "SAVED_PATH"],
            );
            assert_eq!(values[0], Some("kept".into()));
            assert_eq!(values[1], None);
            assert_eq!(values[2], values[3], "rejected PATH must remain unchanged");
            assert!(String::from_utf8_lossy(&stderr).contains("empty PATH"));
        }
    }
    if shell != "fish" {
        let s = Session::new(shell);
        let source = format!(
            "raw {shell} <<CODE\nbuiltin typeset -ix SLOT=0\nCODE\nset SLOT 'x[$(./native_fixture)]'\nset AFTER wrong\n"
        );
        let (values, stderr) = s.run(
            &source,
            false,
            "\\builtin test ! -e ./marker || exit 87",
            &["SLOT", "AFTER"],
        );
        assert_eq!(values, [Some("0".into()), None]);
        assert!(!stderr.is_empty());
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "requires real Bash on Linux/macOS")]
fn bash_conformance() {
    conformance("bash");
}

#[test]
#[cfg_attr(not(unix), ignore = "requires real Zsh on Linux/macOS")]
fn zsh_conformance() {
    conformance("zsh");
}

#[test]
#[cfg_attr(not(unix), ignore = "requires real Fish on Linux/macOS")]
fn fish_conformance() {
    conformance("fish");
}

#[test]
#[cfg(target_os = "macos")]
fn system_bash_is_rejected_before_effects() {
    let mut session = Session::new("bash");
    session.executable = "/bin/bash";
    let (values, stderr) = session.run("set AFTER wrong\n", false, "", &["AFTER"]);
    assert_eq!(values, [None]);
    assert!(String::from_utf8_lossy(&stderr).contains("Bash 5.2+ required"));
}
