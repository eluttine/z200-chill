# AGENTS.md — z200-chill

## Project Overview
`z200-chill` is an open-source compiler and lightweight package/build tool (`z200c`) for the **CHILL** (**ITU-T Z.200**) programming language targeting **Linux**.

Prioritize clarity, deterministic behavior, and small, verifiable changes over cleverness or broad rewrites.

## Repository Structure (Target Layout)
- `src/` — Compiler and CLI (`z200c`) source code (lexer, parser, AST, symbol table/mode checker, lowering/codegen, CLI, manifest loader).
- `runtime/` — Linux runtime support library (`libz200rt` / `libchillrt`) for processes, signals, timers, and exceptions.
- `tests/` — Unit tests, parser/semantic fixtures, and end-to-end `.ch` test programs.
- `examples/` — Small sample CHILL programs and multi-module packages.
- `docs/` — Language subset notes, manifest specification, and design decisions.

*(Update these paths if the repository uses a different layout.)*

## Core Architectural Principles
1. **Separate frontend, IR/lowering, and backend:** Keep lexing, parsing, AST construction, and mode/type checking decoupled from target code generation or runtime details.
2. **Respect CHILL's module & visibility model:** Model `MODULE`, `SPEC`, `GRANT`, and `SEIZE` explicitly in the AST/symbol table; mangle or namespace symbols internally so multi-module and multi-package builds do not collide.
3. **Configurable integer widths (`SHORT INT` / `LONG INT`):** Default to `32` and `64` bits on Linux, with support for configurable profiles (such as `16-32` or `8-16`) while enforcing `width(SHORT INT) <= width(INT) <= width(LONG INT)` and including the active profile in build cache keys.
4. **Incremental, explicit scope:** Support a well-defined CHILL subset first (modes, declarations, expressions, control flow, procedures, modules) before adding full concurrency, timers, or exotic Z.200 features.
5. **Isolate runtime & target semantics:** Keep CHILL-specific semantics (processes, signals, timers, exceptions, range/overflow checks) in clean runtime helpers and explicit lowering rules rather than ad-hoc hacks.

## Coding & Contribution Rules for Agents
- **Stay scoped to the task:** Do not refactor unrelated modules, rename public CLI/manifest fields, or change formatting across untouched files unless requested.
- **Add or update tests with behavior changes:** Every new syntax rule, mode/type rule, manifest behavior, width profile rule, or codegen/runtime fix should include a corresponding test case.
- **Never invent language or runtime semantics:** When ITU-T Z.200 rules, dialect behavior, integer width/overflow behavior, or runtime semantics are ambiguous or underspecified, state the assumption clearly in code comments/docs or ask before guessing.
- **Keep `AGENTS.md` concise:** Only add durable project rules, build commands, and gotchas that save future sessions mistakes.

## Build, Lint, and Test Commands
Replace these placeholders with the project's actual commands once initialized:
- Build compiler & CLI (`z200c`): `<add build command, e.g. cargo build>`
- Run unit & integration tests: `<add test command, e.g. cargo test>`
- Run a sample `.ch` program: `<add run command, e.g. z200c run examples/hello>`
- Lint / typecheck: `<add lint/check command, e.g. cargo clippy>`

## Working Style
- Before editing large files or changing architecture, summarize the plan briefly.
- After making changes, report what was modified, what tests were run, and any remaining limitations or assumptions.

