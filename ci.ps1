# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (c) 2025-present Iron Curtain contributors

param(
    [ValidateSet("lint", "test", "all", "quick")]
    [string]$Mode = "all",
    [ValidateSet("auto", "windows", "unix")]
    [string]$DispatchHost = "auto"
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path

switch ($DispatchHost) {
    "auto" {
        $DispatchHost = "windows"
    }
    "windows" {
        & (Join-Path $repoRoot "ci-local.ps1") -Mode $Mode
        exit $LASTEXITCODE
    }
    "unix" {
        $bashCommand = Get-Command bash -ErrorAction SilentlyContinue
        if (-not $bashCommand) {
            Write-Host "ERROR: bash is not available for Unix-host dispatch." -ForegroundColor Red
            exit 1
        }

        & $bashCommand.Source (Join-Path $repoRoot "ci-local.sh") $Mode
        exit $LASTEXITCODE
    }
}
