# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (c) 2025-present Iron Curtain contributors

# ci-local.ps1 - Local CI validation for Iron Curtain (PowerShell)
# Best-effort local wrapper for the repository's core GitHub Actions checks.

$ErrorActionPreference = "Stop"

Write-Host "=== Iron Curtain - Local CI ===" -ForegroundColor Cyan

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

Write-Host "* Using cargo: $(Get-Command cargo | Select-Object -ExpandProperty Source)" -ForegroundColor Green
Write-Host "Rust version: $(& rustc --version)" -ForegroundColor Magenta
Write-Host ""

function Run-Check {
    param(
        [string]$Name,
        [string]$Command
    )

    Write-Host "Running: $Name" -ForegroundColor Blue
    Write-Host "Command: $Command" -ForegroundColor Gray

    $startTime = Get-Date
    $oldErrorAction = $ErrorActionPreference
    $ErrorActionPreference = "Continue"

    try {
        Invoke-Expression "$Command 2>&1" | ForEach-Object { Write-Host $_ }
        $exitCode = $LASTEXITCODE
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

if (-not (Test-Path "Cargo.toml")) {
    Write-Host "ERROR: Cargo.toml not found. Run this from the project root." -ForegroundColor Red
    exit 1
}

Write-Host "Validating UTF-8 encoding..." -ForegroundColor Cyan

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

$utf8Files = @("README.md", "AGENTS.md", "CODE-INDEX.md", "Cargo.toml")
foreach ($file in $utf8Files) {
    if (-not (Test-Utf8Encoding $file)) { exit 1 }
}

$projectFiles = Get-ChildItem -Path "crates", ".github" -Recurse -File -Include *.rs,*.md,*.toml,*.yml
foreach ($file in $projectFiles) {
    if (-not (Test-Utf8Encoding $file.FullName)) { exit 1 }
}
Write-Host ""

Run-Check "Format check" "cargo fmt --all --check"
Run-Check "Workspace check" "cargo check --workspace --all-targets --locked"
Run-Check "Clippy" "cargo clippy --workspace --all-targets --locked -- -D warnings"
Run-Check "Tests" "cargo test --workspace --locked"

$env:RUSTDOCFLAGS = "-D warnings"
Run-Check "Documentation" "cargo doc --workspace --no-deps --locked"
$env:RUSTDOCFLAGS = $null

Write-Host "Running license check..." -ForegroundColor Cyan
if (Get-Command cargo-deny -ErrorAction SilentlyContinue) {
    Run-Check "License check (cargo deny)" "cargo deny check licenses"
} else {
    Write-Host "WARNING: cargo-deny not found. Install it with: cargo install cargo-deny --locked" -ForegroundColor Yellow
    Write-Host ""
}

Write-Host "Running security audit..." -ForegroundColor Cyan
if (Get-Command cargo-audit -ErrorAction SilentlyContinue) {
    Run-Check "Security audit" "cargo audit"
} else {
    Write-Host "WARNING: cargo-audit not found. Install it with: cargo install cargo-audit --locked" -ForegroundColor Yellow
    Write-Host ""
}

Write-Host "Checking MSRV (1.85)..." -ForegroundColor Cyan
if (Get-Command rustup -ErrorAction SilentlyContinue) {
    $env:CARGO_TARGET_DIR = "target/msrv"
    Run-Check "MSRV check" "rustup run 1.85 cargo check --workspace --all-targets --locked"
    Run-Check "MSRV clippy" "rustup run 1.85 cargo clippy --workspace --all-targets --locked -- -D warnings"
    Run-Check "MSRV tests" "rustup run 1.85 cargo test --workspace --locked"
    $env:CARGO_TARGET_DIR = $null
} else {
    Write-Host "WARNING: rustup not found. Skipping MSRV check." -ForegroundColor Yellow
    Write-Host ""
}

Write-Host "All local CI checks passed." -ForegroundColor Green
