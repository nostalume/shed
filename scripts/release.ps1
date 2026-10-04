<#
.SYNOPSIS
  Update Cargo's package version and create the matching annotated Git tag.
.EXAMPLE
  .\scripts\release.ps1 0.1.6
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Version
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$Version = $Version.TrimStart('v')
if ($Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+$') {
    throw "release version must be MAJOR.MINOR.PATCH"
}

if (git status --porcelain) {
    throw "release requires a clean working tree"
}

$branch = (git branch --show-current).Trim()
if ($branch -ne 'main') {
    throw "release must run from the main branch"
}

$cargoPath = Join-Path (Get-Location) 'Cargo.toml'
$cargo = Get-Content -Raw $cargoPath
$updated = [regex]::Replace(
    $cargo,
    '(?m)^(version[ \t]*=[ \t]*")[^"]+("[ \t]*)$',
    { param($m) $m.Groups[1].Value + $Version + $m.Groups[2].Value },
    1
)
if ($updated -eq $cargo) { throw "Cargo.toml package version was not found" }
Set-Content -Path $cargoPath -Value $updated -NoNewline

$lockPath = Join-Path (Get-Location) 'Cargo.lock'
$lock = Get-Content -Raw $lockPath
$updated = [regex]::Replace(
    $lock,
    '(?ms)(\[\[package\]\][ \t]*\r?\nname = "shed"[ \t]*\r?\nversion = ")[^"]+("[ \t]*)',
    { param($m) $m.Groups[1].Value + $Version + $m.Groups[2].Value },
    1
)
if ($updated -eq $lock) { throw "Cargo.lock shed package version was not found" }
Set-Content -Path $lockPath -Value $updated -NoNewline

cargo check --locked
git add Cargo.toml Cargo.lock
git diff --cached --quiet
if ($LASTEXITCODE -ne 0) {
    git commit -m "chore: release v$Version"
}

git tag -a "v$Version" -m "Release v$Version"
git push --atomic origin "HEAD:$branch" "v$Version"
Write-Host "released v$Version"
