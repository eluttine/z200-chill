# z200-chill

`z200-chill` is an open-source compiler and lightweight package/build tool (`z200c`) for the **CHILL** (**ITU-T Z.200**) programming language targeting **Linux**.

CHILL was designed for large, long-lived, high-reliability systems (such as telecommunications switching and event-driven control). `z200-chill` aims to make a practical subset of CHILL approachable, buildable, and testable on modern Linux without requiring legacy proprietary workstations or closed hardware.

## Project Status

Early development. `z200c` currently provides:

- **Packages and CLI** — `Chill.toml` loading, multi-file packages, package-directory and grant-file dependencies, and single-file `.ch` scripts without a manifest.
- **Frontend** — lexer, parser, AST, and a typed expression/statement IR for the supported CHILL subset.
- **Native backend** — `z200c compile` emits C for a sequential subset, including procedures, arrays, structures, powersets, scalar references, runtime checks, and exception handlers, and builds a native executable; the interpreter remains the reference for unsupported features.
- **Modes and checks** — mode checking, static exception-flow checks, and stable diagnostic codes with multiple errors reported together; see [docs/diagnostics.md](docs/diagnostics.md).
- **Modules and interfaces** — `GRANT`/`SEIZE`, qualified names, `GRANT ALL`/`SEIZE ALL`, `PREFIXED`, `TO` friends, structure-mode `FORBID`, `SPEC MODULE` body validation, and TOML grant files.
- **Interpreter** — procedures, arrays, structures, sets, powersets, text, references, generics, multiple `CASE` selectors, `DO FOR`, labelled `EXIT`, and handlers.
- **Concurrency and objects** — cooperative processes, signals, buffers, timing, regions, and module/region/task object modes.
- **Grant files and scripts** — `check`/`grant` resolve interface-only grant files; `run` links them when source metadata is present.

```sh
cargo run -- check examples/hello
cargo run -- run examples/hello
cargo run -- compile examples/hello
cargo run -- grant examples/spec
```

Example programs:

- `examples/hello` — minimal first-run program that writes `Hello, world!`.
- `examples/procedures` — procedure/conversion acceptance test; computes `11` and verifies it with `ASSERT`.
- `examples/modules` — `GRANT`/`SEIZE` with a shared seized location.
- `examples/spec` — a `SPEC MODULE` paired with its body.
- `examples/pipeline` — end-to-end cooperative concurrency example.
- `examples/showcase` — integration example covering module modes, references, arrays, slices, and `CASE`.

## Repository Structure

```text
z200-chill/
├── AGENTS.md           # Guidelines and conventions for coding agents & contributors
├── Cargo.toml          # Rust package for z200c
├── README.md           # This file
├── src/                # CLI, lexer, parser, AST, mode checker, interpreter, manifest
├── tests/              # CLI integration tests and parser/sema/run fixtures
├── examples/
│   ├── hello/          # Minimal "Hello, world!" package
│   ├── procedures/     # Procedure, conversion, and ASSERT example
│   ├── modules/        # GRANT/SEIZE visibility example
│   ├── spec/           # SPEC MODULE + body example
│   ├── control/        # DO FOR / CASE / EXIT example
│   ├── multifile/      # Manifest `sources` example
│   ├── liblocal/       # Dependency package used by `app`
│   ├── app/            # Directory-dependency linking example
│   ├── pipeline/       # End-to-end concurrent pipeline example
│   └── showcase/       # Objects, references, arrays, slices, and CASE
└── docs/               # subset.md, manifest.md, grant-file.md, diagnostics.md, ir.md,
                        # concurrency.md, moreta.md, text.md, backend.md,
                        # contexts.md, piecewise.md, decisions.md
