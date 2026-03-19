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
./ci-local.sh
```

On Windows PowerShell:

```powershell
./ci-local.ps1
```

These scripts aim to track the repository’s GitHub Actions checks:

- `cargo fmt --all --check`
- `cargo check --workspace --all-targets --locked`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
- `cargo doc --workspace --no-deps --locked`
- `cargo deny check licenses`
- `cargo audit`
- MSRV verification on Rust `1.85`

`cargo deny`, `cargo audit`, and MSRV checks depend on the corresponding local
tools being installed. GitHub Actions remains the authoritative enforcement
path.

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
