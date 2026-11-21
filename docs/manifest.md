# `Chill.toml` Manifest

A z200-chill package is described by a `Chill.toml` next to its sources.

```toml
[package]
name = "hello"
version = "0.1.0"
edition = "z200-11/99"

[build]
entry = "src/main.ch"
int_widths = "16-32-64"

[dependencies]
lib = "../lib/target/z200c/lib.grt.toml"

[pieces]
counter = "pieces/counter.ch"
```

## `[package]`

- `name` — non-empty package name.
- `version` — non-empty package version string.
- `edition` — `z200-11/99` (the current in-force edition) or `z200-1988`.
  `z200-1999` is accepted as an alias for `z200-11/99`.

## `[build]`

- `entry` — the CHILL entry file. It is compiled last, after `sources`.
- `sources` — additional CHILL source files, compiled in listed order before the
  entry. Modules from all files are siblings in the program, so `GRANT`/`SEIZE` works
  across them.
- `int_widths` — integer width profile, `SHORT_INT-INT-LONG_INT` or the legacy
  `INT-LONG_INT` form. Defaults to `16-32-64`.

```toml
[build]
entry = "src/main.ch"
sources = ["src/library.ch"]
```

## `[pieces]`

Text-reference names for remote piece designators (`REMOTE counter;`). Values are
package-relative paths. Literal designators (`REMOTE "pieces/a.ch";`) do not need an
entry; empty designators resolve to `pieces/<unit-name>.ch`. See
[piecewise.md](piecewise.md).

## Integer width profiles

The full rules live in [subset.md](subset.md), but the manifest-relevant parts are:

### Profile

The project wants configurable widths with this invariant:

```text
width(SHORT_INT) <= width(INT) <= width(LONG_INT)
```

and signed two's-complement ranges:

| Mode        | Width *n* | Range |
|-------------|-----------|-------|
| `SHORT_INT` | *s*       | −2^(s−1) … 2^(s−1)−1 |
| `INT`       | *i*       | −2^(i−1) … 2^(i−1)−1 |
| `LONG_INT`  | *l*       | −2^(l−1) … 2^(l−1)−1 |

Default Linux profile: `SHORT_INT` = 16, `INT` = 32, `LONG_INT` = 64.
The active profile must be part of the build-cache key (per `AGENTS.md`).

### Notation

Chosen format (A): a three-number form

```toml
int_widths = "16-32-64"   # SHORT_INT-INT-LONG_INT
```

The two-number legacy form `"32-64"` is read as `INT-LONG_INT`, with `SHORT_INT`
defaulting to `INT`; this is documented, never silent. Profiles must satisfy the
containment invariant, and any violation is a manifest error.


## `[dependencies]`

Each key is a dependency name and each value is either a package directory or a
compiled grant file.

```toml
[dependencies]
liblocal = "../liblocal"                         # source-linked dependency
lib = "../lib/target/z200c/lib.grt.toml"         # interface-only dependency
```

- Paths are resolved relative to the depending package's directory.
- A **directory** dependency (or a path to its `Chill.toml`) is loaded recursively and
  its modules are linked into the program. Dependency modules run before the depending
  package's `sources` and `entry`.
- A **grant-file** dependency contributes its exported interface for static checking.
  A grant file produced by `z200c grant` also records its source package directory; in
  that case `run` links the source package. Interface-only grant files remain
  check/build-only, and `run` rejects a seized external name from them.
- Directory dependencies are loaded once; cycles and duplicate package loads are
  detected.
- The dependency's `edition` must match, and its `int_widths` must parse to the same
  `WidthProfile` (equivalent spellings are accepted).
- Exported simple names may repeat across modules/packages. An unqualified `SEIZE name`
  is then ambiguous; use `Module!name` or `package!Module!name`. `GRANT ... TO ...`
  restricts visibility to named friend modules.
- Grant files record each export's module/package names as well as its simple name, so
  duplicate simple names can be emitted and consumed with qualified `SEIZE`.
- Modules are identified by package plus name, so different packages may contain a
  module with the same simple name. An `EXIT ModuleName` still targets the enclosing
  module.
- `z200c check`, `z200c run`, and `z200c grant` all see source-linked dependencies.
  `check`/`grant` additionally resolve `SEIZE` against grant-file dependencies.
- `z200c run` links directory dependencies and grant files with source metadata; it
  rejects a run only when an interface-only external name is actually seized.

## Commands

```sh
z200c check <path>   # parse and mode-check
z200c run <path>     # parse, check, and interpret
z200c grant <path>   # parse, check, and write target/z200c/<package>.grt.toml
```

`z200c build` remains an alias for `z200c grant`.
