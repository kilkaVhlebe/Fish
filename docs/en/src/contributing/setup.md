# Development setup

## Windows

1. Install Rust 1.99.1 (MSVC host) with `rustup`; `rust-toolchain.toml` pins it.
2. Install Visual Studio 2022 with the C++ workload and a Windows SDK.
3. `cargo install mdbook cargo-llvm-cov cargo-deny`

## Linux

`sudo apt-get install libasound2-dev libudev-dev`, then the same `cargo install` as above.

## Workflow

```text
cargo xtask ci         # fmt, clippy -D warnings, tests, config + docs checks
cargo xtask coverage   # per-crate line coverage gates from fish.toml
```

One commit per roadmap item, message `P<n>.<m>: <summary>`. Tick the item in both
`docs/en/src/roadmap.md` and `docs/ru/src/roadmap.md`; every page exists in both languages.
