# z200-chill

`z200-chill` is an open-source compiler and lightweight package/build tool (`z200c`) for the **CHILL** (**ITU-T Z.200**) programming language targeting **Linux**.

CHILL was designed for large, long-lived, high-reliability systems (such as telecommunications switching and event-driven control). `z200-chill` aims to make a practical subset of CHILL approachable, buildable, and testable on modern Linux without requiring legacy proprietary workstations or closed hardware.

## Project Status

Early development. The `z200c` CLI can currently load and validate `Chill.toml`, tokenize and parse the v0 subset, perform mode checking, enforce qualified `GRANT`/`SEIZE` visibility (`Module!name`, `GRANT ALL` / `SEIZE ALL`, `PREFIXED` output, `TO` friends, static structure-mode `FORBID`) between modules, validate `SPEC MODULE` interfaces against their bodies, compile multi-file packages, link package-directory and source-bearing grant-file dependencies for `run`, lower checked programs to a typed expression/statement IR, and interpret them (including multiple `CASE` selectors, `DO FOR`, labelled `EXIT`, cooperative processes, regions, and task objects). Single `.ch` files can be checked or run without a manifest (`z200c run hello.ch`). `z200c grant` emits a TOML interface file; `check` and `grant` can resolve `SEIZE` against interface-only grant files, while `run` links them only when the grant file records its source package. Modules are identified by package plus name, so different packages may reuse a module name. Diagnostics carry stable codes and multiple checker errors are reported together; see [docs/diagnostics.md](docs/diagnostics.md).

```sh
cargo run -- check examples/hello
cargo run -- run examples/hello
cargo run -- grant examples/spec
```

`examples/hello` is the minimal first-run program: it writes `Hello, world!`. `examples/procedures` is the procedure/conversion acceptance test: it computes `11` and verifies it with `ASSERT`, exiting successfully with no output. `examples/modules` demonstrates `GRANT` / `SEIZE`, including a shared seized location, and `examples/spec` demonstrates a `SPEC MODULE` paired with its body.

## Repository Structure

```text
z200-chill/
├── AGENTS.md           # Guidelines and conventions for coding agents & contributors
├── Cargo.toml          # Rust package for z200c
├── README.md           # This file
├── src/                # CLI, lexer, parser, AST, mode checker, interpreter, manifest
├── tests/              # CLI integration tests
├── examples/
│   ├── hello/          # Minimal "Hello, world!" package
│   ├── procedures/     # Procedure, conversion, and ASSERT example
│   ├── modules/        # GRANT/SEIZE visibility example
│   ├── spec/           # SPEC MODULE + body example
│   ├── control/        # DO FOR / CASE / EXIT example
│   ├── multifile/      # Manifest `sources` example
│   ├── liblocal/       # Dependency package used by `app`
│   ├── app/            # Directory-dependency linking example
│   └── pipeline/       # End-to-end concurrent pipeline example
└── docs/               # subset.md, manifest.md, grant-file.md, diagnostics.md, ir.md,
                        # concurrency.md, moreta.md, contexts.md, piecewise.md,
                        # decisions.md
