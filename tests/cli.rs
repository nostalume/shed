use std::io::Write;
use std::process::{Command, Output, Stdio};

fn shed(args: &[&str], source: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_shed"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(source).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn check_accepts_the_next_language_without_evaluating_it() {
    let source = br#"set VALUE "${env:MISSING}"
alias gs = git status --short
if have this_program_does_not_exist or (env ENABLED and os linux)
    init this_program_does_not_exist "${home}"
end
raw powershell <<CODE
throw "check must not execute raw code"
CODE
"#;
    let result = shed(&["check"], source);
    assert!(result.status.success(), "{:?}", result.stderr);
    assert_eq!(result.stdout, b"ok (4 top-level nodes)\n");
    assert!(result.stderr.is_empty());
}

#[test]
fn compile_and_admission_errors_never_write_partial_scripts() {
    for (args, source, message) in [
        (
            vec!["--target-os", "linux", "bash"],
            &b"set A x\nalias if = program"[..],
            "reserved bash alias name",
        ),
        (
            vec!["--target-os=linux", "fish"],
            &b"if false\nalias status = program\nend"[..],
            "reserved fish alias name",
        ),
        (
            vec!["--target-os=linux", "zsh"],
            &b"if false\nalias nocorrect = program\nend"[..],
            "reserved zsh alias name",
        ),
        (
            vec!["--target-os=linux", "zsh"],
            &b"alias typeset = program"[..],
            "reserved zsh alias name",
        ),
        (
            vec!["--target-os=windows", "powershell"],
            &b"set A x\nalias if = program"[..],
            "reserved PowerShell alias name",
        ),
        (
            vec!["--target-os=windows", "powershell"],
            &b"if false\nalias __shed_env = program\nend"[..],
            "line 2",
        ),
        (
            vec!["--target-os", "windows", "fish"],
            &b""[..],
            "unsupported target",
        ),
        (vec!["pwsh"], &b""[..], "unknown shell"),
        (vec!["check"], &b"set A x\nend"[..], "line 2"),
        (vec!["check"], &b"set A x\nset B \"unfinished"[..], "line 2"),
        (vec!["check"], &b"\xff"[..], "UTF-8"),
        (
            vec!["--target-os=plan9", "check"],
            &b""[..],
            "unknown target OS",
        ),
        (vec!["--target-os"], &b""[..], "requires a value"),
        (vec!["check", "one", "two"], &b""[..], "unexpected argument"),
    ] {
        let result = shed(&args, source);
        assert!(!result.status.success(), "{args:?}");
        assert!(result.stdout.is_empty(), "{args:?}");
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(message),
            "{args:?}: {:?}",
            result.stderr
        );
    }
    for name in ["end", "eval", "set"] {
        let source = format!("alias {name} = program\n");
        let result = shed(&["--target-os=linux", "bash"], source.as_bytes());
        assert!(result.status.success(), "{:?}", result.stderr);
    }
}

#[test]
fn file_inputs_have_path_and_source_context_on_failure() {
    let file = std::env::temp_dir().join(format!("shed-cli-{}.shed", std::process::id()));
    std::fs::write(&file, "set A x\nend").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_shed"))
        .arg("check")
        .arg(&file)
        .output()
        .unwrap();
    std::fs::remove_file(&file).unwrap();
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    let diagnostic = String::from_utf8(result.stderr).unwrap();
    assert!(diagnostic.contains(file.to_str().unwrap()));
    assert!(diagnostic.contains("line 2"));
}

#[test]
fn the_committed_example_checks_from_file_and_stdin() {
    let source = include_bytes!("../examples/env.shed");
    let stdin = shed(&["check"], source);
    assert!(stdin.status.success());
    assert_eq!(stdin.stdout, b"ok (6 top-level nodes)\n");
    let file = Command::new(env!("CARGO_BIN_EXE_shed"))
        .args([
            "check",
            concat!(env!("CARGO_MANIFEST_DIR"), "/examples/env.shed"),
        ])
        .output()
        .unwrap();
    assert!(file.status.success());
    assert_eq!(file.stdout, stdin.stdout);
    assert!(file.stderr.is_empty());
}

#[test]
fn excessive_syntax_returns_a_diagnostic_instead_of_overflowing_the_stack() {
    for source in [
        "if true\n".repeat(128) + &"end\n".repeat(128),
        "if ".to_owned() + &"not ".repeat(127) + "true\nend",
        "if ".to_owned() + &"(".repeat(127) + "true" + &")".repeat(127) + "\nend",
    ] {
        let result = shed(&["check"], source.as_bytes());
        assert!(result.status.success(), "{:?}", result.stderr);
    }
    for source in [
        "if true\n".repeat(2_000) + &"end\n".repeat(2_000),
        "if ".to_owned() + &"not ".repeat(2_000) + "true\nend",
        "if ".to_owned() + &"(".repeat(2_000) + "true" + &")".repeat(2_000) + "\nend",
        "if ".to_owned() + &"true and ".repeat(19_999) + "true\nend",
    ] {
        let result = shed(&["check"], source.as_bytes());
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains("parser capacity"));
    }
}

#[test]
fn specialization_accepts_the_frontends_combined_capacity_without_execution() {
    for prefix in ["if true\n", "if env a\n"] {
        let source = prefix.repeat(127)
            + "if "
            + &"not ".repeat(127)
            + "env b\nend\n"
            + &"end\n".repeat(127);
        for shell in ["bash", "zsh", "fish", "powershell"] {
            let result = shed(&["--target-os=linux", shell], source.as_bytes());
            assert!(result.status.success(), "{shell}: {:?}", result.stderr);
            assert!(!result.stdout.is_empty());
            assert!(result.stderr.is_empty());
        }
    }
    let result = shed(
        &["--target-os=linux", "bash"],
        b"if false\nset A \"${unknown}\"\nend",
    );
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("line 2"));
}

#[test]
fn long_branch_chains_compile_without_recursive_emission() {
    let source = "if env A\n".to_owned() + &"elif env A\n".repeat(20_000) + "end\n";
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let result = shed(&["--target-os=linux", shell], source.as_bytes());
        assert!(result.status.success(), "{shell}: {:?}", result.stderr);
        assert!(result.stderr.is_empty());
    }
}
