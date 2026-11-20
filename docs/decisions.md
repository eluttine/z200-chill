# Decisions

Resolved and open project decisions. The language rules live in [subset.md](subset.md),
the manifest in [manifest.md](manifest.md), and grant files in
[grant-file.md](grant-file.md).

## 10.1 Integer mode spelling (resolved)

Z.200 defines only `INT`; extra integer modes are single-identifier names such as
`LONG_INT`. The project standardizes on `SHORT_INT`, `INT`, and `LONG_INT` everywhere,
including conversions (`LONG_INT(b)`). The non-standard two-word spellings and the bare
`LONG` conversion have been removed from the example and from `AGENTS.md`.

## 10.2 `int_widths` notation (resolved)

Three-number form `"16-32-64"` = `SHORT_INT-INT-LONG_INT`. The legacy two-number form
`"32-64"` is read as `INT-LONG_INT`, with `SHORT_INT` = `INT`. See §4.3.

## 10.3 Harness output convention (resolved)

The hello example verifies itself with the standard `ASSERT` action, so `run_ok` does not
need I/O:

- expected stdout: empty;
- expected exit status: success;
- unhandled `ASSERTFAIL`/exception: failure.

## 10.4 Grant-file / multi-module model (qualified seize resolved; grant renaming open)

v0 implements `SPEC MODULE` pairing, writes a TOML grant file from `z200c grant`, and
consumes dependency grant files for `SEIZE` resolution in `check` and `grant`.
Package-directory dependencies are loaded recursively and linked for `run`; a grant file
that records `package_dir` is also source-linked for `run`. Dependencies and the root
must share edition and width profile. Modules are identified by package plus simple name,
so different packages may contain a module with the same name. `SEIZE` supports
`Module!name` and `package!Module!name`, plus `GRANT ALL`/`SEIZE ALL` and `TO` friend
restrictions. Emitted grant files record each export's module/package names, so
qualified external seizes work. `GRANT ... PREFIXED ...`, `SEIZE ... PREFIXED ...`, and
prefix-rename clauses (including a non-empty old prefix) are implemented. Structure
modes and static `FORBID (fields)` / `FORBID ALL` are implemented for internal
`GRANT`/`SEIZE`.
Structure values are interpreted with field assignment, nested fields, and copy
semantics. Grant-file schema 2 records structure modes under symbolic names
(`package!Module!Mode`), per-grant `FORBID`, and `TO` friends, so external packages can
seize modes and enforce restrictions. The format is a `z200c` artifact; it is not defined
by Z.200, and schema changes are deliberately breaking while the project is early.
Still open: `SEIZE` with a non-empty new prefix (locally prefixed names), and symbol
mangling for a code-generation backend. Z.200 permits several piecewise schemes (§10.10).

## 10.7 Multi-file packages (resolved)

`[build] sources` lists additional files compiled in order before the entry. Modules from
all files are siblings, so `GRANT`/`SEIZE` works across them. Execution order is
dependency modules, then `sources`, then `entry`.

## 10.5 Range-check default (resolved for v0)

The interpreter always checks integer range/overflow conditions. Z.200 leaves these
checks optional unless a handler is present (§1.1, §1.8); a configurable flag can be
added with the native backend.

## 10.6 Width defaults (resolved)

Confirmed: `SHORT_INT` = 16, `INT` = 32, `LONG_INT` = 64 on Linux. The standard does
not constrain them.

## 10.8 Typed IR (resolved)

The checker records expression modes and `Apply` resolutions; `ir::lower` produces typed
expression and statement nodes (`ir::Expr`, `ir::Stmt`) that the interpreter consumes.
This removes expression-level re-inference and AST action traversal from the interpreter.
See [ir.md](ir.md).

## 10.9 Code generation (open)

A native or C code-generation backend remains future work; the typed IR is the intended
input. Declarations/procedure structure and `BEGIN` declaration lists remain AST-level.

---
