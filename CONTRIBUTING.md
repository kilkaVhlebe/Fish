# Contributing

## Setup

See `docs/en/src/contributing/setup.md`. Short version: Rust 1.99.1 (pinned by
`rust-toolchain.toml`), then `cargo xtask ci`.

## Rules

- One commit per roadmap item, message `P<n>.<m>: <summary>`.
- Tick the item in **both** `docs/en/src/roadmap.md` and `docs/ru/src/roadmap.md`.
- Docs exist in English and Russian; `cargo xtask docs-check` enforces identical file sets
  and roadmap state.
- Before committing: `cargo fmt`, `cargo clippy --workspace --all-targets --locked -- -D warnings`,
  `cargo test --workspace --locked`.
- Public items in `fish-abi`, `fish-metadata`, `fish-pack`, `fish-api`, `fish-loader` need rustdoc.
