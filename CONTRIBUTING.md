# Contributing To Iron Curtain

Iron Curtain is part of the wider `iron-curtain-engine` family of repositories.
This repo is the engine workspace. Sibling repos such as `cnc-formats`,
`fixed-game-math`, and `deterministic-rng` hold reusable standalone crates.

## Before You Start

Read these files first:

- `AGENTS.md` for local implementation rules
- `CODE-INDEX.md` for current repo routing
- the canonical design docs repo:
  `https://github.com/iron-curtain-engine/iron-curtain-design-docs`

Do not silently invent canonical behavior locally when the design docs are
missing, contradictory, or infeasible. Follow the design-gap workflow in
`AGENTS.md`.

## Local Validation

Run the same core checks locally before opening a PR:

```bash
./ci lint
./ci test
./ci all
```

On Windows PowerShell:

```powershell
./ci.ps1 lint
./ci.ps1 test
./ci.ps1 all
```

The underlying host-native wrappers remain available when you want them
directly:

```powershell
./ci-local.ps1 lint
./ci-local.ps1 test
./ci-local.ps1 all
```

`ci` is the stable repo entrypoint. It dispatches to the host-native wrapper
for the current shell so contributors and agents do not have to remember the
underlying script names. `lint` is the preferred first pass because it catches
compile and lint issues before the slower test run. `test` reruns only the
workspace tests once lint is already green, and `all` adds the optional
repo-policy checks.

The core checks are:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
- `cargo doc --workspace --no-deps --locked`
- `cargo deny check licenses`
- `cargo audit`
- MSRV verification on Rust `1.85`

When a change touches non-host targets, also run the matching target checks
where the required SDK/toolchain is available:

- `cargo check --workspace --target wasm32-unknown-unknown --locked`
- `cargo clippy --workspace --target wasm32-unknown-unknown --locked -- -D warnings`
- `cargo check --workspace --target aarch64-linux-android --locked`
- `cargo clippy --workspace --target aarch64-linux-android --locked -- -D warnings`
- `cargo check --workspace --target aarch64-apple-ios --locked`
- `cargo clippy --workspace --target aarch64-apple-ios --locked -- -D warnings`

`cargo deny`, `cargo audit`, and MSRV checks depend on the corresponding local
tools being installed. GitHub Actions remains the authoritative enforcement
path and runs `check` / `clippy` / `test` on Ubuntu, Windows, and macOS plus
dedicated `wasm32-unknown-unknown`, `aarch64-linux-android`, and
`aarch64-apple-ios` `check` / `clippy` lanes.

## Pull Requests

Every implementation change should make the following easy to answer:

- Which milestone / `G*` step does this belong to?
- Which design decisions (`Dxxx`) constrain it?
- What proof shows the behavior is correct?

If you change code layout, ownership boundaries, crate names, or navigation
paths, update `CODE-INDEX.md` in the same change set.

## Commit Sign-Off

This repository requires the Developer Certificate of Origin (DCO). Sign each
commit with:

```bash
git commit -s
```

That adds a `Signed-off-by:` line to the commit message.

## Design-Doc Relationship

This repo implements the design. The design-doc repo is canonical.

When implementation and design disagree:

1. Narrow the conflict to the exact code path and design reference.
2. Open or link a design-gap / design-change request.
3. Keep any local workaround narrow until the design decision is settled.
