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

## 10.4 Grant-file / multi-module model (static model resolved; linking open)

v0 implements `SPEC MODULE` pairing, writes a TOML grant file from `z200c grant`, and
consumes dependency grant files for `SEIZE` resolution in `check` and `grant`. The
dependency's edition and width profile must match. Linking external dependencies for
`run`, symbol mangling, and executing dependency code are still open. Z.200 permits
several piecewise schemes (§10.10); the next step is a linking strategy while rejecting
unsupported schemes with clear diagnostics.

## 10.5 Range-check default (resolved for v0)

The interpreter always checks integer range/overflow conditions. Z.200 leaves these
checks optional unless a handler is present (§1.1, §1.8); a configurable flag can be
added with the native backend.

## 10.6 Width defaults (resolved)

Confirmed: `SHORT_INT` = 16, `INT` = 32, `LONG_INT` = 64 on Linux. The standard does
not constrain them.

---
