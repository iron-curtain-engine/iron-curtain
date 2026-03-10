# Iron Curtain

A modern, open-source RTS engine built in Rust — starting with Command & Conquer.

*Red Alert first. Tiberian Dawn alongside it. The rest of the C&C family to follow.*

## Status

> ⚠️ **Early development** — Phase 0 (Foundation & Format Literacy). No playable build exists yet.

## Design Documents

All architectural decisions, design rationale, and roadmap are maintained in the
[Iron Curtain Design Documentation](https://github.com/iron-curtain-engine/iron-curtain-design-docs)
repository. The hosted book is available at:

**<https://iron-curtain-engine.github.io/iron-curtain-design-docs/>**

Read `AGENTS.md` in this repo for implementation-specific rules.

## Building

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo deny check licenses
```

## Crate Structure

| Crate          | Purpose                                              |
| -------------- | ---------------------------------------------------- |
| `ic-protocol`  | Shared wire types (`PlayerOrder`, `TimestampedOrder`) |
| `ra-formats`   | RA1 asset parsers (`.mix`, `.shp`, `.pal`, `.aud`)   |

Additional crates will be added as development progresses through the
[milestone roadmap](https://iron-curtain-engine.github.io/iron-curtain-design-docs/08-ROADMAP.html).

## Standalone Crates (MIT/Apache-2.0)

These general-purpose libraries are extracted into separate repositories
under permissive licenses for maximum community reuse (D076):

| Crate               | Repository                                                                     | Purpose                              |
| -------------------- | ------------------------------------------------------------------------------ | ------------------------------------ |
| `cnc-formats`        | [cnc-formats](https://github.com/iron-curtain-engine/cnc-formats)              | Clean-room C&C binary format parsers |
| `fixed-game-math`    | [fixed-game-math](https://github.com/iron-curtain-engine/fixed-game-math)      | Deterministic fixed-point arithmetic |
| `deterministic-rng`  | [deterministic-rng](https://github.com/iron-curtain-engine/deterministic-rng)  | Seedable platform-identical PRNG     |

## Contributing

Interested in Rust game dev, RTS design, format parsing, networking, AI/ML, or art — open an issue or say hello.

All contributions require a Developer Certificate of Origin (DCO) — add `Signed-off-by` to your commit messages (`git commit -s`).

## License

Engine source code is licensed under **GPL-3.0-or-later** with an explicit modding exception
(YAML, Lua, and WASM mods loaded through the engine's data interfaces are NOT derivative works).
See [LICENSE](LICENSE) for the full text.

## Trademark Disclaimer

Red Alert, Tiberian Dawn, Command & Conquer, and C&C are trademarks of Electronic Arts Inc.
Iron Curtain is **not** affiliated with, endorsed by, or sponsored by Electronic Arts.
These names are used solely to identify the games and formats the engine is designed to be
compatible with (nominative fair use).
