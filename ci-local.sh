#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (c) 2025-present Iron Curtain contributors

# ci-local.sh - Host-native local validation for Iron Curtain
#
# This wrapper is intentionally mode-based:
# - `lint` gets the repo to a clean host-native lint/doc state first
# - `test` runs the workspace tests once lint is already green
# - `all` runs lint + tests + optional policy checks
#
# The script only validates the OS that is currently running it. That matters
# for `cfg(target_os = "...")` code: Linux/WSL cannot prove Windows-only paths.
# Cross-platform enforcement lives in the GitHub Actions matrix.

set -euo pipefail

MODE="${1:-all}"

case "$MODE" in
    lint|test|all|quick)
        ;;
    *)
        echo "ERROR: unsupported mode '$MODE'"
        echo "Usage: ./ci-local.sh [lint|test|all|quick]"
        exit 2
        ;;
esac

echo "=== Iron Curtain - Local CI ==="

if ! command -v cargo >/dev/null 2>&1; then
    CARGO_PATHS=(
        "$HOME/.cargo/bin/cargo"
        "$HOME/.cargo/bin/cargo.exe"
        "/c/Users/$(whoami)/.cargo/bin/cargo.exe"
    )

    for cargo_path in "${CARGO_PATHS[@]}"; do
        if [[ -x "$cargo_path" ]]; then
            export PATH="$(dirname "$cargo_path"):$PATH"
            echo "* Found cargo at: $cargo_path"
            break
        fi
    done

    if ! command -v cargo >/dev/null 2>&1; then
        echo "ERROR: cargo not found. Install Rust from https://rustup.rs/"
        exit 1
    fi
fi

echo "* Using cargo: $(command -v cargo)"
echo "Rust version: $(rustc --version)"
echo "Host OS: $(uname -s) ($(uname -m))"
echo "Mode: $MODE"
echo "Validation scope: current host only. Use ci-local.ps1 on Windows and GitHub Actions for the full OS matrix."
echo

run_check() {
    local name="$1"
    local command="$2"

    echo "Running: $name"
    echo "Command: $command"

    local start_time
    start_time=$(date +%s)

    if eval "$command"; then
        local end_time
        end_time=$(date +%s)
        local duration=$((end_time - start_time))
        echo "PASS: $name (${duration}s)"
        echo
    else
        local end_time
        end_time=$(date +%s)
        local duration=$((end_time - start_time))
        echo "FAIL: $name (${duration}s)"
        exit 1
    fi
}

run_utf8_checks() {
    echo "Validating UTF-8 encoding..."

    for file in README.md AGENTS.md CODE-INDEX.md Cargo.toml; do
        check_utf8 "$file" || exit 1
        check_no_bom "$file" || exit 1
    done

    find crates .github -type f \( -name "*.rs" -o -name "*.md" -o -name "*.toml" -o -name "*.yml" \) | while read -r file; do
        check_utf8 "$file" || exit 1
        check_no_bom "$file" || exit 1
    done
    echo
}

run_lint_suite() {
    run_utf8_checks
    run_check "Format check" "cargo fmt --all --check"
    run_check "Clippy" "cargo clippy --workspace --all-targets --locked -- -D warnings"
    run_check "Documentation" "RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --locked"
}

run_test_suite() {
    run_check "Tests" "cargo test --workspace --locked"
}

run_optional_policy_suite() {
    echo "Running license check..."
    if command -v cargo-deny >/dev/null 2>&1; then
        run_check "License check (cargo deny)" "cargo deny check licenses"
    else
        echo "WARNING: cargo-deny not found. Install it with: cargo install cargo-deny --locked"
        echo
    fi

    echo "Running security audit..."
    if command -v cargo-audit >/dev/null 2>&1; then
        run_check "Security audit" "cargo audit"
    else
        echo "WARNING: cargo-audit not found. Install it with: cargo install cargo-audit --locked"
        echo
    fi

    echo "Checking MSRV (1.85)..."
    if command -v rustup >/dev/null 2>&1; then
        export CARGO_TARGET_DIR="target/msrv"
        run_check "MSRV clippy" "rustup run 1.85 cargo clippy --workspace --all-targets --locked -- -D warnings"
        run_check "MSRV tests" "rustup run 1.85 cargo test --workspace --locked"
        unset CARGO_TARGET_DIR
    else
        echo "WARNING: rustup not found. Skipping MSRV check."
        echo
    fi
}

if [[ ! -f "Cargo.toml" ]]; then
    echo "ERROR: Cargo.toml not found. Run this from the project root."
    exit 1
fi

check_utf8() {
    local file="$1"

    if command -v file >/dev/null 2>&1; then
        local file_output
        file_output=$(file "$file")
        if echo "$file_output" | grep -q "UTF-8\|ASCII\|text\|[Ss]ource"; then
            echo "  OK: $file"
            return 0
        fi
        echo "ERROR: $file is not valid UTF-8"
        return 1
    fi

    echo "  OK: $file (assumed)"
    return 0
}

check_no_bom() {
    local file="$1"
    if command -v xxd >/dev/null 2>&1; then
        if head -c 3 "$file" | xxd | grep -qE "ef[ ]?bb[ ]?bf"; then
            echo "ERROR: $file has UTF-8 BOM (remove it)"
            return 1
        fi
    elif command -v od >/dev/null 2>&1; then
        if head -c 3 "$file" | od -t x1 | grep -qE "ef[ ]?bb[ ]?bf"; then
            echo "ERROR: $file has UTF-8 BOM (remove it)"
            return 1
        fi
    fi
    return 0
}

case "$MODE" in
    quick)
        run_utf8_checks
        run_check "Format check" "cargo fmt --all --check"
        run_check "Clippy" "cargo clippy --workspace --all-targets --locked -- -D warnings"
        ;;
    lint)
        run_lint_suite
        ;;
    test)
        run_test_suite
        ;;
    all)
        run_lint_suite
        run_test_suite
        run_optional_policy_suite
        ;;
esac

echo "All local CI checks passed."
