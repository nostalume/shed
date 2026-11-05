//! Real-session tests: no generated-text assertions certify runtime behavior.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Session {
    root: PathBuf,
    scripts: usize,
}

impl Session {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "shed-session-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self { root, scripts: 0 }
    }

    fn file(&self, name: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    fn script(&mut self, source: &str) -> String {
        let os = if cfg!(windows) {
            "windows"
        } else if cfg!(target_os = "macos") {
            "darwin"
        } else {
            "linux"
        };
        let mut compiler = Command::new(env!("CARGO_BIN_EXE_shed"))
            .args(["--target-os", os, "powershell"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        compiler
            .stdin
            .take()
            .unwrap()
            .write_all(source.as_bytes())
            .unwrap();
        let output = compiler.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let name = format!("{}.ps1", self.scripts);
        self.scripts += 1;
        self.file(&name, output.stdout);
        format!(". \"$env:SHED_TEST_ROOT/{name}\"")
    }

    fn native(&self) -> PathBuf {
        let path = self
            .root
            .join(format!("native_fixture{}", std::env::consts::EXE_SUFFIX));
        let output = Command::new("rustc")
            .arg("--edition=2024")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/native.rs"
            ))
            .arg("-o")
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

    fn run(&self, body: &str) -> Output {
        let driver = self.file("driver.ps1", format!(r#"
$ErrorActionPreference = 'Stop'
function Assert([bool] $condition, [string] $message) {{ if (-not $condition) {{ throw $message }} }}
Set-Location -LiteralPath $env:SHED_TEST_ROOT
{body}
"#));
        let output = Command::new("pwsh")
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
            .arg(driver)
            .env("SHED_TEST_ROOT", &self.root)
            .env_remove("SHED_AFTER")
            .env_remove("SHED_BEFORE")
            .env_remove("SHED_UNSET")
            .env_remove("SHED_NATIVE_MARKER")
            .env_remove("SHED_AUTOLOADED")
            .output()
            .expect("PowerShell 7.5+ is required for backend conformance tests");
        assert!(
            output.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr).trim_start_matches('!')
        );
        output
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn dsl(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        out.push_str(match c {
            '\\' => "\\\\",
            '"' => "\\\"",
            '$' => "\\$",
            '\n' => "\\n",
            '\r' => "\\r",
            '\t' => "\\t",
            _ => {
                out.push(c);
                continue;
            }
        });
    }
    out.push('"');
    out
}

fn argv(path: impl AsRef<Path>) -> Vec<String> {
    let bytes = fs::read(path).unwrap();
    let mut rest = bytes.as_slice();
    let mut args = Vec::new();
    while !rest.is_empty() {
        let len = u64::from_le_bytes(rest[..8].try_into().unwrap()) as usize;
        args.push(String::from_utf8(rest[8..8 + len].to_vec()).unwrap());
        rest = &rest[8 + len..];
    }
    args
}

#[test]
fn scalar_state_paths_and_literal_queries() {
    let mut session = Session::new();
    let values = [
        "",
        "a b",
        "'\"`$;|&<>[]*?#",
        "‘’‚‛“”中文🙂",
        "\r\n\t",
        "${env:OTHER}",
        "$(throw 'injection')",
        "C:\\trailing\\",
        "~",
    ];
    let mut source = String::new();
    for (i, value) in values.iter().enumerate() {
        session.file(&format!("value{i}"), value);
        source.push_str(&format!("set SHED_V{i} {}\n", dsl(value)));
    }
    source.push_str("set SHED_COPY \"pre${env:SHED_V2}post${env:SHED_UNSET}${shell}\"\nset SHED_UNSET yes\nunset SHED_UNSET\nif exists '[literal]' and not exists '*'\nset SHED_EXISTS yes\nend\n");
    session.file("[literal]", "");
    let load = session.script(&source);
    let empty = session.script("if env SHED_UNSET\nend\n");
    let paths = session.script("path+ relative\npath+ relative\npath- relative\npath- relative\n");
    let prepend = session.script("path+ relative\n");
    let home = session.script("set SHED_HOME \"${home}\"\n");
    let body = format!(
        r#"
{load}
Assert $? 'load status'
foreach ($i in 0..8) {{
    $value = [Environment]::GetEnvironmentVariable("SHED_V$i")
    Assert ($null -ne $value) "present value $i"
    Assert ($value -ceq [IO.File]::ReadAllText("$env:SHED_TEST_ROOT/value$i")) "scalar $i"
}}
Assert ($null -eq [Environment]::GetEnvironmentVariable('SHED_UNSET')) 'unset'
Assert ($env:SHED_COPY -ceq ('pre' + $env:SHED_V2 + 'postpowershell')) 'expansion boundaries'
Assert ($env:SHED_EXISTS -ceq 'yes') 'literal filesystem lookup'
{empty}
Assert $? 'empty branch returns success'
{home}
Assert ($env:SHED_HOME -ceq $HOME) 'home'
$p = [IO.Path]::Join($env:SHED_TEST_ROOT, 'relative')
$delimiter = [IO.Path]::PathSeparator
[Environment]::SetEnvironmentVariable('PATH','')
{paths}
Assert ($env:PATH -ceq ([string]$delimiter + $p)) 'empty PATH entry, idempotence and append'
Remove-Item Env:PATH
{paths}
Assert ($env:PATH -ceq $p) 'absent PATH'
$upper = $p.ToUpperInvariant()
$env:PATH = 'old' + $delimiter + $p + $delimiter + $upper + $delimiter + $delimiter + $p
{prepend}
Assert ($env:PATH -ceq ($p + $delimiter + 'old' + $delimiter + $upper + $delimiter)) 'prepend preserves old entries'
{paths}
Assert ($env:PATH -ceq ('old' + $delimiter + $upper + $delimiter + $delimiter + $p)) 'exact ordinal equality and old spelling'
"#
    );
    let output = session.run(&body);
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn ordered_short_circuit_errors_and_code_unit_failure() {
    let mut session = Session::new();
    let skipped = session.script("if env SHED_TRUE or exists \"${home}\"\nset SHED_BRANCH first\nelif exists \"${home}\"\nset SHED_BRANCH wrong\nend\nif env SHED_FALSE and exists \"${home}\"\nset SHED_BRANCH wrong\nend\n");
    let failed = session.script(
        "set SHED_BEFORE kept\nif exists \"${home}\" and false\nend\nset SHED_AFTER wrong\n",
    );
    let invalid_path = session.script("set SHED_BEFORE kept\npath+ ''\nset SHED_AFTER wrong\n");
    let syntax =
        session.script("set SHED_BEFORE kept\nraw powershell <<PS\n}\nPS\nset SHED_AFTER wrong\n");
    let execution = session.script("set SHED_BEFORE kept\nraw powershell <<PS\nWrite-Error 'unit failure'\n$env:SHED_AFTER = 'wrong'\nPS\nset SHED_AFTER wrong\n");
    let mixed = session.script("if env SHED_A or env SHED_B and env SHED_C\nset SHED_RESULT yes\nelse\nset SHED_RESULT no\nend\n");
    let noop = session.script("if false\nset UNUSED no\nend\nraw bash <<SH\nexit 88\nSH\n");
    let raw = "$env:SHED_RAW_TEXT = $MyInvocation.MyCommand.ScriptBlock.ToString()\r\nfunction ShedSessionFunction { 'persisted' }\r\n";
    session.file("raw-body", raw);
    let raw_load = session.script(&format!("raw powershell <<PS\n{raw}PS\n"));
    let output = session.run(&format!(r#"
$env:SHED_TRUE = 'yes'
$env:SHED_FALSE = ''
$oldHome = $HOME
Set-Variable HOME -Value 'relative' -Force
{skipped}
Assert ($env:SHED_BRANCH -ceq 'first') 'short circuit and first branch'
$caught = $false
try {{ {failed} }} catch {{ $caught = $true; Assert ($_.Exception.Message.Contains('home')) 'left error identity' }}
Assert $caught 'error and false must fail'
Set-Variable HOME -Value $oldHome -Force
foreach ($a in @('', 'yes')) {{ foreach ($b in @('', 'yes')) {{ foreach ($c in @('', 'yes')) {{
    $env:SHED_A=$a; $env:SHED_B=$b; $env:SHED_C=$c
    {mixed}
    Assert (($env:SHED_RESULT -ceq 'yes') -eq (([bool]$a) -or (([bool]$b) -and ([bool]$c)))) 'DSL precedence'
}} }} }}
{noop}
Assert $? 'empty specialization returns success'
{raw_load}
Assert ((ShedSessionFunction) -ceq 'persisted') 'raw function survives in loading scope'
Assert ($env:SHED_RAW_TEXT -ceq [IO.File]::ReadAllText("$env:SHED_TEST_ROOT/raw-body")) 'raw CRLF bytes'
$ErrorActionPreference = 'Continue'
$PSNativeCommandUseErrorActionPreference = $false
foreach ($unit in @({{ {invalid_path} }}, {{ {syntax} }}, {{ {execution} }})) {{
    $caught = $false
    try {{ . $unit }} catch {{ $caught = $true }}
    Assert $caught 'runtime error'
    Assert ($env:SHED_BEFORE -ceq 'kept') 'prior effect retained'
    Assert ($null -eq $env:SHED_AFTER) 'later effects skipped'
    Assert ($ErrorActionPreference -eq 'Continue') 'preference restored'
    Assert (-not $PSNativeCommandUseErrorActionPreference) 'native preference restored'
}}
"#,
    ));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn native_calls_alias_lifetime_and_fresh_resolution() {
    let mut session = Session::new();
    session.native();
    let native = format!(
        "${{env:SHED_TEST_ROOT}}/native_fixture{}",
        std::env::consts::EXE_SUFFIX
    );
    let ext = std::env::consts::EXE_SUFFIX;
    let source = format!(
        r#"
if have "{native}" and not have Get-Item and not have ShedProbe and not have 'native_*'
set SHED_HAVE yes
end
alias native_fixture = native_fixture argv "${{env:SHED_TEST_ROOT}}/argv" "${{env:SHED_LATE}}" '' 'a b' "quote\"end\\" '‘🙂’' '$(throw)' '*.txt'
alias zero = "{native}" argv "${{env:SHED_TEST_ROOT}}/zero"
alias native_error = "{native}" exit 7
alias location = native_fixture where "${{env:SHED_TEST_ROOT}}/location"
alias echo = "{native}" out
alias relative_native = './native_fixture{ext}' argv "${{env:SHED_TEST_ROOT}}/relative" '--erroraction' 'literal'
alias noargs = "${{env:SHED_LATE_PROGRAM}}"
"#
    );
    let load = session.script(&source);
    let module = session.root.join("modules/ShedProbe");
    fs::create_dir_all(&module).unwrap();
    fs::write(
        module.join("ShedProbe.psm1"),
        "$env:SHED_AUTOLOADED='wrong'; function ShedProbe { throw 'wrong' }",
    )
    .unwrap();
    fs::create_dir(session.root.join("other")).unwrap();
    fs::copy(
        session.root.join(format!("native_fixture{ext}")),
        session.root.join(format!("other/native_fixture{ext}")),
    )
    .unwrap();
    let output = session.run(&format!(r#"
$env:PSModulePath = "$env:SHED_TEST_ROOT/modules" + [IO.Path]::PathSeparator + $env:PSModulePath
$env:PATH = $env:SHED_TEST_ROOT + [IO.Path]::PathSeparator + $env:PATH
$env:SHED_NATIVE_MARKER = "$env:SHED_TEST_ROOT/marker"
function native_fixture {{ throw 'function must be bypassed' }}
function DefineWrappers {{ {load} }}
DefineWrappers
Assert ($env:SHED_HAVE -ceq 'yes') 'external-only queries'
Assert (-not [IO.File]::Exists($env:SHED_NATIVE_MARKER)) 'have never executes'
Assert ($null -eq $env:SHED_AUTOLOADED) 'have never autoloads'
Assert ($null -eq $ExecutionContext.SessionState.PSVariable.Get('global:PSModuleAutoLoadingPreference')) 'absent autoload preference restored'
$global:PSModuleAutoLoadingPreference = 'All'
Assert ($null -eq (__shed_resolve ShedProbe)) 'external-only lookup with existing preference'
Assert ($global:PSModuleAutoLoadingPreference -ceq 'All') 'existing autoload preference restored'
$env:SHED_LATE = 'invocation'
$env:SHED_LATE_PROGRAM = "$env:SHED_TEST_ROOT/native_fixture{ext}"
noargs
native_fixture '' '--flag=value' 'caller quote"' 'caller space' ("line" + [char]13 + [char]10)
zero
$caught = $false
try {{ native_error }} catch {{ $caught = $true }}
Assert $caught 'native nonzero result'
Assert ($LASTEXITCODE -eq 7) 'native exit status'
Assert ((echo) -ceq 'native output') 'alias replaces built-in alias and preserves output pipeline'
$PSNativeCommandArgumentPassing = 'Legacy'
relative_native '-ErrorAction' 'Continue' ''
Assert ($PSNativeCommandArgumentPassing -ceq 'Legacy') 'native preference scoped to invocation'
$bad = [string][char]0xd800
[IO.File]::Delete($env:SHED_NATIVE_MARKER)
$caught = $false
try {{ native_fixture $bad }} catch {{ $caught = $true }}
Assert $caught 'unrepresentable caller argument rejected'
Assert (-not [IO.File]::Exists($env:SHED_NATIVE_MARKER)) 'invalid argv cannot launch'
$env:PATH = "$env:SHED_TEST_ROOT/other"
location
Assert ([IO.File]::ReadAllText("$env:SHED_TEST_ROOT/location") -ceq [IO.Path]::GetFullPath("$env:SHED_TEST_ROOT/other/native_fixture{ext}")) 'fresh resolution after PATH change'
"#));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert_eq!(
        argv(session.root.join("argv")),
        [
            "invocation",
            "",
            "a b",
            "quote\"end\\",
            "‘🙂’",
            "$(throw)",
            "*.txt",
            "",
            "--flag=value",
            "caller quote\"",
            "caller space",
            "line\r\n"
        ]
    );
    assert_eq!(argv(session.root.join("zero")), Vec::<String>::new());
    assert_eq!(
        argv(session.root.join("relative")),
        ["--erroraction", "literal", "-ErrorAction", "Continue", ""]
    );
}

#[test]
fn initializer_bytes_scope_and_failures() {
    let mut session = Session::new();
    session.native();
    let native = format!(
        "\"${{env:SHED_TEST_ROOT}}/native_fixture{}\"",
        std::env::consts::EXE_SUFFIX
    );
    let init_values = ["", "a b", "'\"`$;|&<>[]*?#", "‘🙂’", "\\end\\", "\r\n"];
    let init_args = init_values
        .iter()
        .map(|value| dsl(value))
        .collect::<Vec<_>>()
        .join(" ");
    let argument_load = session.script(&format!(
        "init {native} argv \"${{env:SHED_TEST_ROOT}}/init-argv\" {init_args}\n"
    ));
    let code = "[IO.File]::WriteAllText(\"$env:SHED_TEST_ROOT/init-text\", $MyInvocation.MyCommand.ScriptBlock.ToString())\r\nfunction ShedInitFunction { 'initialized' }\r\n#".to_owned() + &"x".repeat(262_144) + "\r\n\r\n";
    session.file("code", &code);
    let success = session.script(&format!(
        "init {native} emit \"${{env:SHED_TEST_ROOT}}/code\" 0\nset SHED_AFTER yes\n"
    ));
    let mut invalid = b"$env:SHED_DANGEROUS='wrong'\n".to_vec();
    invalid.push(0xff);
    session.file("invalid", invalid);
    session.file("with-nul", b"$env:SHED_DANGEROUS='wrong'\n\0");
    session.file("nonzero", "$env:SHED_DANGEROUS='wrong'\n");
    session.file("empty-code", "");
    session.file("syntax-code", "}\n");
    session.file(
        "runtime-code",
        "Write-Error 'initializer code failed'\n$env:SHED_DANGEROUS='wrong'\n",
    );
    let mut failures = Vec::new();
    for (name, exit) in [
        ("invalid", 0),
        ("with-nul", 0),
        ("nonzero", 7),
        ("syntax-code", 0),
        ("runtime-code", 0),
    ] {
        failures.push(session.script(&format!("set SHED_BEFORE kept\ninit {native} emit \"${{env:SHED_TEST_ROOT}}/{name}\" {exit}\nset SHED_AFTER wrong\n")));
    }
    let empty = session.script(&format!(
        "init {native} emit \"${{env:SHED_TEST_ROOT}}/empty-code\" 0\nset SHED_AFTER yes\n"
    ));
    let raw_native = session.script(&format!("raw powershell <<PS\n& \"$env:SHED_TEST_ROOT/native_fixture{}\" exit 9\nPS\nset SHED_AFTER wrong\n",std::env::consts::EXE_SUFFIX));
    let missing = session.script(&format!("if have {native}\nraw powershell <<PS\n[IO.File]::Delete(\"$env:SHED_TEST_ROOT/native_fixture{}\")\nPS\nend\nif have {native}\nset SHED_STALE wrong\nend\ninit {native} exit 0\nset SHED_AFTER wrong\n",std::env::consts::EXE_SUFFIX));
    let mut body = format!(
        r#"
{success}
{argument_load}
Assert ($env:SHED_AFTER -ceq 'yes') 'init success continues'
Assert ((ShedInitFunction) -ceq 'initialized') 'init function persists'
Assert ([IO.File]::ReadAllText("$env:SHED_TEST_ROOT/init-text") -ceq [IO.File]::ReadAllText("$env:SHED_TEST_ROOT/code")) 'exact init CRLF and trailing newlines'
Remove-Item Env:SHED_AFTER
{empty}
Assert ($env:SHED_AFTER -ceq 'yes') 'empty initializer succeeds'
Remove-Item Env:SHED_AFTER
"#
    );
    for failure in failures.iter().chain([&raw_native, &missing]) {
        body.push_str(&format!(
            r#"
$caught = $false
try {{ {failure} }} catch {{ $caught = $true }}
Assert $caught 'initializer failure {failure}'
Assert ($null -eq $env:SHED_DANGEROUS) 'invalid or nonzero stdout must not execute'
Assert ($null -eq $env:SHED_AFTER) 'later effect skipped'
Assert ($null -eq $env:SHED_STALE) 'have cannot use a disappeared executable'
"#
        ));
    }
    let output = session.run(&body);
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, vec![b'!'; 131_072 * 7]);
    assert_eq!(argv(session.root.join("init-argv")), init_values);
}
