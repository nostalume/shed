#![allow(dead_code)] // Each integration-test binary uses a subset of these helpers.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

pub fn shed(args: &[&str], source: &[u8]) -> Output {
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

pub fn native(root: &Path) -> PathBuf {
    let path = root.join(format!("native_fixture{}", std::env::consts::EXE_SUFFIX));
    let output = Command::new("rustc")
        .args([
            "--edition=2024",
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/native.rs"),
            "-o",
        ])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    path
}

pub fn take_string(rest: &mut &[u8]) -> String {
    let len = u64::from_le_bytes(rest[..8].try_into().unwrap()) as usize;
    let value = String::from_utf8(rest[8..8 + len].to_vec()).unwrap();
    *rest = &rest[8 + len..];
    value
}

pub fn arguments(file: impl AsRef<Path>) -> Vec<String> {
    let bytes = fs::read(file).unwrap();
    let mut rest = bytes.as_slice();
    let mut values = Vec::new();
    while !rest.is_empty() {
        values.push(take_string(&mut rest));
    }
    values
}

pub fn quote(text: &str) -> String {
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
