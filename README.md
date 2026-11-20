# z200-chill

`z200-chill` is an open-source compiler and lightweight package/build tool (`z200c`) for the **CHILL** (**ITU-T Z.200**) programming language targeting **Linux**.

CHILL was designed for large, long-lived, high-reliability systems (such as telecommunications switching and event-driven control). `z200-chill` aims to make a practical subset of CHILL approachable, buildable, and testable on modern Linux without requiring legacy proprietary workstations or closed hardware.

## Project Status

Early development. The `z200c` CLI can currently load and validate `Chill.toml` (edition and integer-width profiles), tokenize and parse the v0 subset, perform mode checking, enforce `GRANT`/`SEIZE` visibility between modules, validate `SPEC MODULE` interfaces against their bodies, compile multi-file packages, link package-directory dependencies for `run`, interpret a package end to end (including `DO FOR`, `CASE`, and labelled `EXIT`), and emit a TOML grant file with `z200c grant`. `check` and `grant` can also resolve `SEIZE` against grant-file dependencies; `run` does not link those interface-only dependencies. Diagnostics carry stable codes and multiple checker errors are reported together; see [docs/diagnostics.md](docs/diagnostics.md).

```sh
cargo run -- check examples/hello
cargo run -- run examples/hello
cargo run -- grant examples/spec
```

`examples/hello` is the first acceptance test: it computes `11` and verifies it with `ASSERT`, exiting successfully with no output. `examples/modules` demonstrates `GRANT` / `SEIZE`, including a shared seized location, and `examples/spec` demonstrates a `SPEC MODULE` paired with its body.

## Repository Structure

```text
z200-chill/
├── AGENTS.md           # Guidelines and conventions for coding agents & contributors
├── Cargo.toml          # Rust package for z200c
├── README.md           # This file
├── src/                # CLI, lexer, parser, AST, mode checker, interpreter, manifest
├── tests/              # CLI integration tests
├── examples/
│   ├── hello/          # Starter CHILL package and manifest example
│   ├── modules/        # GRANT/SEIZE visibility example
│   ├── spec/           # SPEC MODULE + body example
│   ├── control/        # DO FOR / CASE / EXIT example
│   ├── multifile/      # Manifest `sources` example
│   ├── liblocal/       # Dependency package used by `app`
│   └── app/            # Directory-dependency linking example
└── docs/               # subset.md, manifest.md, grant-file.md, diagnostics.md, ir.md, decisions.md
