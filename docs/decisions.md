# Decisions

Resolved and open project decisions. The language rules live in [subset.md](subset.md),
the manifest in [manifest.md](manifest.md), and grant files in
[grant-file.md](grant-file.md).

## 10.1 Integer mode spelling (resolved)

Z.200 defines only `INT`; extra integer modes are single-identifier names such as
`LONG_INT`. The project standardizes on `SHORT_INT`, `INT`, and `LONG_INT` everywhere,
including conversions (`LONG_INT(b)`). The non-standard two-word spellings and the bare
`LONG` conversion have been removed from the example and from `AGENTS.md`.

## 10.2 Integer widths (resolved, revised)

Integer widths are fixed by the compiler (`SHORT_INT` = 16, `INT` = 32,
`LONG_INT` = 64 on Linux), not configured per package. The old `[build] int_widths`
manifest field and the width profile recorded in grant files were removed: width is a
target/ABI property, so every package in a build shares it. See §4.2 of
[subset.md](subset.md).

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
that records `package_dir` is also source-linked for `run`. Dependencies share the compiler's fixed language revision and integer widths. Modules are identified by package plus simple name,
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

## 10.15 Shared reach tree (stage 1)

`program::ReachTree` now holds the static module reach hierarchy and each module's
declaration interface (variables, procedures, modes). `sema` and `interp` read module
mode/declaration data from this tree instead of maintaining separate span-keyed maps.
The tree also records child reaches, whether each nested module is directly hoisted
from a module body, and the module-reach export metadata (owner, kind, prefix/friends).
The interpreter's module-body hoisting follows that plan, and both the checker and
interpreter seed module-scope exports from the tree instead of recomputing module-body
grants. Top-level child-export collection for `GRANT` re-exports now also reads the tree.
Block reaches (`BEGIN`/`IF`/`DO`/`CASE`) are tree nodes keyed by block/branch/
alternative spans; checker and interpreter block scopes are seeded from them instead of
rescanning child modules. Nested `GRANT` validation and `FORBID` application happen once
while building the tree, so the checker no longer collects nested grant sets and the
interpreter no longer registers grants at module execution time. The interpreter hoists
both module-body and block modules from the same tree-driven routine; it no longer scans
IR statements to find nested module declarations.

## 10.14 Text I/O (implementation-defined, resolved for now)

`WRITE`, `WRITELINE`, and `READLINE` provide practical text I/O as
implementation-defined built-in routines, which Z.200 permits. `READTEXT`/`WRITETEXT`
support literal format text, conversion/editing codes, variable widths, io lists, and
I/O control clauses. Bounded text modes (`CHARS(n)`, `CHARS(n) VARYING`) and a
simplified text-file association/access model are also implemented. `WCHARS`, full
association attributes, and arbitrary file models remain future work.

## 10.13 Exceptions (partial, resolved for now)

`CAUSE`, module/procedure handlers (`ON ... ELSE ... END`), action-level handlers, and
catching `ASSERTFAIL`/`OVERFLOW` are implemented. Static exception-set flow checking
enforces `EXCEPTIONS` lists. Unhandled exceptions fail with `E0509`; the
lists are parsed but not enforced.

## 10.12 Manifest minimalism (resolved)

`Chill.toml` is a non-normative build artifact. It is kept small: `package.name` is
optional and defaults to the directory name, `build.entry` defaults to
`src/main.ch`, and `build.sources` auto-discovers other `.ch` files in the entry
directory when omitted. Package version, edition, and integer widths are not manifest
configuration. `[dependencies]` and `[pieces]` remain because they are genuine build
graph/piece lookup data.

## 10.11 Piecewise compilation (design)

Remote pieces, piece designators, nested spec modules, and regions are a separate
"piecewise compilation" milestone. Z.200 leaves designator-to-text mapping
implementation-defined, so the manifest will carry a `[pieces]` text-reference table and
literal designators resolve package-relative. Phases 0–4 are implemented for the current subset: `[pieces]` manifest lookups, remote
syntax, designator resolution with cycle detection, expansion of remote
modules/specs/regions/program units, remote contexts/context-list merging,
`CONTEXT MODULE REMOTE` desugaring, recursive nested spec modules, virtual grants (applied by `program::normalize` for
every semantic entry point),
top-level regions, and spec regions. Region concurrency/mutual exclusion and nested
regions remain. See [piecewise.md](piecewise.md).

## 10.9 Code generation (open)

A native or C code-generation backend remains future work; the typed IR is the intended
input. Declarations/procedure structure and `BEGIN` declaration lists remain AST-level.

## 10.10 Contexts and nested modulions (partially resolved)

Top-level `CONTEXT ... FOR` lists are parsed and checked. Quasi `DCL`/procedure
signatures are validated against the grants that enter the imaginary outermost reach
(existence, uniqueness, mode/signature match, `TO` restrictions), and contexts have no
runtime effect. Nested body modules are supported as actions: they execute in order,
`SEIZE` from the immediate parent reach (variables, procedures, modes), and accept
contexts checked against that reach. `module_floor` hides parent names unless seized.
Nested modules can `GRANT` their own, child, and grandchild variable declarations into
the parent reach; siblings can `SEIZE` them, and the parent can re-grant them outward.
Duplicate sibling grants are ambiguous at the `SEIZE`. The parent reach can use a
child's grant directly. Nested module declarations are hoisted when the enclosing
module scope is created, so grants are registered and re-granted before module bodies
execute; initializers still run when the module action is entered. Top-level re-grants
appear in the package interface and grant files. Block reaches are modelled for
`BEGIN`, `IF`, `CASE`, and `DO`: modules inside a block grant into that block, siblings
can seize, entries are preloaded/hoisted at block entry, and grants do not escape.
Nested modules can define and grant modes and procedures; their grants can be
re-granted outward. Nested `GRANT` supports `PREFIXED`, `TO`, and `FORBID`. Still open: re-granting an
advanced nested grant upward, advanced context visibility clauses, remote pieces, and
nested spec modules/regions.
See [contexts.md](contexts.md).

---

## 10.16 First-release concurrency policy (resolved)

The first release targets **language coverage and static semantics**, not faithful
concurrent execution. `z200c` remains a single-threaded, non-preemptive interpreter:

- regions do not implement mutual exclusion or waiting queues;
- processes, signals, events, delays, and timers are not executed faithfully;
- no resumable continuation or scheduler is required for the first release.

Faithful CHILL concurrency needs an explicit execution-state machine or async runtime
and is deferred to a post-first-release milestone. This also defers moreta task/region
object modes.
