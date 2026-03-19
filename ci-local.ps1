# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (c) 2025-present Iron Curtain contributors

# ci-local.ps1 - Host-native local validation for Iron Curtain (PowerShell)
#
# The script is mode-based so Windows contributors can lint first, fix host-
# specific issues such as `cfg(target_os = "windows")` branches, and then run
# tests once the workspace is green.

param(
    [ValidateSet("lint", "test", "all", "quick")]
    [string]$Mode = "all"
)

$ErrorActionPreference = "Stop"

Write-Host "=== Iron Curtain - Local CI ===" -ForegroundColor Cyan

function Register-RustToolShim {
    param(
        [string]$ToolName,
        [string]$ToolPath
    )

    if (-not (Test-Path $ToolPath)) {
        return
    }

    # WSL-to-Windows PowerShell launches do not always refresh PATH lookup for
    # freshly prepended tool directories. A script-scoped function shim keeps
    # later `cargo`, `rustc`, and `rustup` invocations pinned to the exact
    # executable we discovered during startup.
    $shim = [scriptblock]::Create("& '$ToolPath' @args")
    Set-Item -Path ("Function:\script:$ToolName") -Value $shim
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoPaths = @(
        "$env:USERPROFILE\.cargo\bin\cargo.exe",
        "$env:HOME\.cargo\bin\cargo.exe",
        "$env:HOME\.cargo\bin\cargo",
        "C:\Users\$env:USERNAME\.cargo\bin\cargo.exe"
    )

    $cargoFound = $false
    foreach ($cargoPath in $cargoPaths) {
        if (Test-Path $cargoPath) {
            $env:PATH = (Split-Path $cargoPath) + ";" + $env:PATH
            Write-Host "* Found cargo at: $cargoPath" -ForegroundColor Green
            $cargoFound = $true
            break
        }
    }

    if (-not $cargoFound -and -not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Host "ERROR: cargo not found. Install Rust from https://rustup.rs/" -ForegroundColor Red
        exit 1
    }
}

$cargoExecutable = @(
    (Get-Command cargo -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue),
    "$env:USERPROFILE\.cargo\bin\cargo.exe",
    "$env:HOME\.cargo\bin\cargo.exe",
    "$env:HOME\.cargo\bin\cargo",
    "C:\Users\$env:USERNAME\.cargo\bin\cargo.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1

if (-not $cargoExecutable) {
    Write-Host "ERROR: cargo could not be resolved after PATH setup." -ForegroundColor Red
    exit 1
}

$script:CargoToolPath = $cargoExecutable
Register-RustToolShim "cargo" $script:CargoToolPath

$rustToolDir = Split-Path $script:CargoToolPath
$script:RustcToolPath = Join-Path $rustToolDir "rustc.exe"
$script:RustupToolPath = Join-Path $rustToolDir "rustup.exe"
Register-RustToolShim "rustc" $script:RustcToolPath
Register-RustToolShim "rustup" $script:RustupToolPath

$rustVersion = if (Test-Path $script:RustcToolPath) {
    $rustVersionOutput = & $script:RustcToolPath --version
    ($rustVersionOutput | Out-String).Trim()
} else {
    "rustc.exe not found next to cargo.exe"
}

Write-Host "* Using cargo: $script:CargoToolPath" -ForegroundColor Green
Write-Host "Rust version: $rustVersion" -ForegroundColor Magenta
Write-Host "Host OS: $([System.Runtime.InteropServices.RuntimeInformation]::OSDescription) [$([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture)]" -ForegroundColor Magenta
Write-Host "Mode: $Mode" -ForegroundColor Magenta
Write-Host "Validation scope: current host only. Use ci-local.sh on Unix-like hosts and GitHub Actions for the full OS matrix." -ForegroundColor DarkGray
Write-Host ""

function Run-Check {
    param(
        [string]$Name,
        [scriptblock]$Action,
        [string]$CommandText
    )

    Write-Host "Running: $Name" -ForegroundColor Blue
    Write-Host "Command: $CommandText" -ForegroundColor Gray

    $startTime = Get-Date
    $oldErrorAction = $ErrorActionPreference
    $ErrorActionPreference = "Continue"

    try {
        $global:LASTEXITCODE = 0
        & $Action 2>&1 | ForEach-Object { Write-Host $_ }
        $exitCode = if ($null -eq $LASTEXITCODE) { 0 } else { $LASTEXITCODE }
        $ErrorActionPreference = $oldErrorAction

        if ($exitCode -ne 0) {
            throw "Command failed with exit code $exitCode"
        }

        $duration = ((Get-Date) - $startTime).TotalSeconds
        Write-Host "PASS: $Name ($([math]::Round($duration))s)" -ForegroundColor Green
        Write-Host ""
    } catch {
        $ErrorActionPreference = $oldErrorAction
        $duration = ((Get-Date) - $startTime).TotalSeconds
        Write-Host "FAIL: $Name ($([math]::Round($duration))s)" -ForegroundColor Red
        exit 1
    }
}

function Run-Utf8Checks {
    Write-Host "Validating UTF-8 encoding..." -ForegroundColor Cyan

    $utf8Files = @("README.md", "AGENTS.md", "CODE-INDEX.md", "Cargo.toml")
    foreach ($file in $utf8Files) {
        if (-not (Test-Utf8Encoding $file)) { exit 1 }
    }

    $projectFiles = Get-ChildItem -Path "crates", ".github" -Recurse -File -Include *.rs,*.md,*.toml,*.yml
    foreach ($file in $projectFiles) {
        if (-not (Test-Utf8Encoding $file.FullName)) { exit 1 }
    }
    Write-Host ""
}

function Run-LintSuite {
    Run-Utf8Checks
    Run-Check "Format check" { & $script:CargoToolPath fmt --all --check } "cargo fmt --all --check"
    Run-Check "Clippy" { & $script:CargoToolPath clippy --workspace --all-targets --locked -- -D warnings } "cargo clippy --workspace --all-targets --locked -- -D warnings"

    $env:RUSTDOCFLAGS = "-D warnings"
    try {
        Run-Check "Documentation" { & $script:CargoToolPath doc --workspace --no-deps --locked } "cargo doc --workspace --no-deps --locked"
    } finally {
        $env:RUSTDOCFLAGS = $null
    }
}

function Run-TestSuite {
    Run-Check "Tests" { & $script:CargoToolPath test --workspace --locked } "cargo test --workspace --locked"
}

function Run-OptionalPolicySuite {
    Write-Host "Running license check..." -ForegroundColor Cyan
    if (Get-Command cargo-deny -ErrorAction SilentlyContinue) {
        Run-Check "License check (cargo deny)" { cargo-deny check licenses } "cargo-deny check licenses"
    } else {
        Write-Host "WARNING: cargo-deny not found. Install it with: cargo install cargo-deny --locked" -ForegroundColor Yellow
        Write-Host ""
    }

    Write-Host "Running security audit..." -ForegroundColor Cyan
    if (Get-Command cargo-audit -ErrorAction SilentlyContinue) {
        Run-Check "Security audit" { cargo-audit } "cargo-audit"
    } else {
        Write-Host "WARNING: cargo-audit not found. Install it with: cargo install cargo-audit --locked" -ForegroundColor Yellow
        Write-Host ""
    }

    Write-Host "Checking MSRV (1.85)..." -ForegroundColor Cyan
    if (Get-Command rustup -ErrorAction SilentlyContinue) {
        $env:CARGO_TARGET_DIR = "target/msrv"
        try {
            Run-Check "MSRV clippy" { & $script:RustupToolPath run 1.85 cargo clippy --workspace --all-targets --locked -- -D warnings } "rustup run 1.85 cargo clippy --workspace --all-targets --locked -- -D warnings"
            Run-Check "MSRV tests" { & $script:RustupToolPath run 1.85 cargo test --workspace --locked } "rustup run 1.85 cargo test --workspace --locked"
        } finally {
            $env:CARGO_TARGET_DIR = $null
        }
    } else {
        Write-Host "WARNING: rustup not found. Skipping MSRV check." -ForegroundColor Yellow
        Write-Host ""
    }
}

if (-not (Test-Path "Cargo.toml")) {
    Write-Host "ERROR: Cargo.toml not found. Run this from the project root." -ForegroundColor Red
    exit 1
}

function Test-Utf8Encoding {
    param([string]$FilePath)

    try {
        $null = Get-Content $FilePath -Encoding UTF8 -ErrorAction Stop
        $bytes = [System.IO.File]::ReadAllBytes($FilePath)
        if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
            Write-Host "ERROR: $FilePath has UTF-8 BOM (remove it)" -ForegroundColor Red
            return $false
        }
        Write-Host "  OK: $FilePath" -ForegroundColor Green
        return $true
    } catch {
        Write-Host "ERROR: $FilePath is not valid UTF-8" -ForegroundColor Red
        return $false
    }
}

switch ($Mode) {
    "quick" {
        Run-Utf8Checks
        Run-Check "Format check" { & $script:CargoToolPath fmt --all --check } "cargo fmt --all --check"
        Run-Check "Clippy" { & $script:CargoToolPath clippy --workspace --all-targets --locked -- -D warnings } "cargo clippy --workspace --all-targets --locked -- -D warnings"
    }
    "lint" {
        Run-LintSuite
    }
    "test" {
        Run-TestSuite
    }
    "all" {
        Run-LintSuite
        Run-TestSuite
        Run-OptionalPolicySuite
    }
}

Write-Host "All local CI checks passed." -ForegroundColor Green
