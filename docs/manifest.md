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
```

## `[package]`

- `name` — non-empty package name.
- `version` — non-empty package version string.
- `edition` — `z200-11/99` (the current in-force edition) or `z200-1988`.
  `z200-1999` is accepted as an alias for `z200-11/99`.

## `[build]`

- `entry` — the single CHILL entry file. A package compiles one entry file; a
  `sources` list for multi-file packages is not implemented yet.
- `int_widths` — integer width profile, `SHORT_INT-INT-LONG_INT` or the legacy
  `INT-LONG_INT` form. Defaults to `16-32-64`.

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

Each key is a dependency name and each value is a path to that dependency's grant file.

```toml
[dependencies]
lib = "../lib/target/z200c/lib.grt.toml"
```

- Paths are resolved relative to the depending package's directory.
- The dependency's `edition` must match.
- The dependency's `int_widths` must parse to the same `WidthProfile` (equivalent
  spellings are accepted).
- Names exported by both the program and a dependency are an error.
- `z200c check` and `z200c grant` resolve `SEIZE` against dependency exports.
- `z200c run` does not link external dependencies; it rejects a run only when an
  external name is actually seized.

## Commands

```sh
z200c check <path>   # parse and mode-check
z200c run <path>     # parse, check, and interpret
z200c grant <path>   # parse, check, and write target/z200c/<package>.grt.toml
```

`z200c build` remains an alias for `z200c grant`.
