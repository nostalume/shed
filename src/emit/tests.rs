use super::literal;
use std::{fs, process::Command};

#[test]
fn powershell_scalar_codec_roundtrips_a_bounded_alphabet() {
    let alphabet: Vec<char> = "a '\"`$;\\\n\r\t#(){}[]‘’‚‛🙂".chars().collect();
    let mut values = vec![String::new()];
    let mut level = vec![String::new()];
    for _ in 0..3 {
        level = level
            .iter()
            .flat_map(|prefix| alphabet.iter().map(move |c| format!("{prefix}{c}")))
            .collect();
        values.extend(level.iter().cloned());
    }
    let root = std::env::temp_dir().join(format!("shed-codec-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let script = root.join("codec.ps1");
    let data = root.join("values");
    let mut source =
        String::from("#requires -Version 7.5\n$ErrorActionPreference='Stop'\n$values = @(\n");
    for value in &values {
        source.push_str(&literal(value));
        source.push('\n');
    }
    source.push_str(")\n$writer=[IO.BinaryWriter]::new([IO.File]::Create($env:SHED_CODEC_DATA))\ntry { foreach($value in $values) { $bytes=[Text.Encoding]::UTF8.GetBytes($value); $writer.Write([int]$bytes.Length); $writer.Write($bytes) } } finally { $writer.Dispose() }\n");
    fs::write(&script, source).unwrap();
    let output = Command::new("pwsh")
        .args(["-NoProfile", "-NonInteractive", "-File"])
        .arg(&script)
        .env("SHED_CODEC_DATA", &data)
        .output();
    let bytes = fs::read(&data);
    fs::remove_dir_all(&root).unwrap();
    let output = output.expect("PowerShell 7.5+ is required for codec conformance");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    let bytes = bytes.unwrap();
    let mut rest = bytes.as_slice();
    for value in &values {
        let len = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
        assert_eq!(&rest[4..4 + len], value.as_bytes(), "{value:?}");
        rest = &rest[4 + len..];
    }
    assert!(rest.is_empty());
}
