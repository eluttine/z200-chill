# Diagnostics and Source Locations

Every diagnostic carries a stable code, an optional source span, and a message. Codes are
assigned explicitly where diagnostics are constructed; message substrings are not used
to infer codes. The CLI renders them as:

```text
src/main.ch:4:3: error[E0404]: cannot assign LONG_INT to SHORT_INT
```

## Source locations

`Span` carries a `SourceId`; `SourceMap` maps ids to files and renders line/column
locations. Lexer, parser, checker, and interpreter errors all use this path, so
multi-file diagnostics do not depend on the entry file.

## Code ranges

| Range | Phase |
|---|---|
| `E01xx` | lexer |
| `E02xx` | parser |
| `E03xx` | manifest / package configuration |
| `E04xx` | checker (static semantics) |
| `E05xx` | interpreter (runtime) |
| `E06xx` | grant files / dependency compatibility |
| `E07xx` | piecewise compilation / remote pieces |

### Lexer

| Code | Meaning |
|---|---|
| `E0100` | generic lexical error |
| `E0101` | unterminated bracketed comment |
| `E0102` | based integer literal |
| `E0103` | character literal control sequence |
| `E0104` | character literal |
| `E0105` | unterminated character string |
| `E0106` | character string control sequence |
| `E0107` | unexpected character |
| `E0108` | integer literal |

### Parser

| Code | Meaning |
|---|---|
| `E0201` | syntax error / expected token |
| `E0202` | module or procedure end-name mismatch |
| `E0203` | nesting limit exceeded |
| `E0204` | spec-module syntax restriction |
| `E0205` | visibility placement or unsupported `GRANT ALL` |
| `E0206` | `CASE` syntax/label restriction |

### Checker

| Code | Meaning |
|---|---|
| `E0400` | generic semantic error |
| `E0401` | unknown mode |
| `E0402` | duplicate declaration/parameter/label |
| `E0403` | unknown name |
| `E0404` | mode/type/assignment mismatch |
| `E0405` | range or case-label out of range |
| `E0406` | visibility, spec, or interface problem |
| `E0407` | `CASE` / `DO FOR` / `EXIT` / loop-counter restriction |
| `E0408` | `RETURN` / `RESULT` misuse |
| `E0409` | unimplemented feature |
| `E0410` | exception-flow violation |

### Interpreter

| Code | Meaning |
|---|---|
| `E0500` | generic runtime error |
| `E0501` | `ASSERTFAIL` |
| `E0502` | `OVERFLOW` |
| `E0503` | invalid arithmetic (division/MOD/exponent) |
| `E0504` | use of an undefined value |
| `E0505` | unknown variable/name |
| `E0506` | call error (arity, depth, missing result) |
| `E0507` | runtime value/mode error |
| `E0508` | control flow or external-link error |

### Piecewise compilation

| Code | Meaning |
|---|---|
| `E0700` | generic piece/piece-designator error |
| `E0701` | remote piece or context is not expanded yet |

### Manifest / grant files

| Code | Meaning |
|---|---|
| `E0300` | missing/invalid manifest or package path |
| `E0600` | grant-file parse/compatibility/output error |

## Multiple diagnostics

The checker records multiple errors across sibling actions, initializers, procedures,
and modules. `z200c check` prints all of them. The parser and lexer also collect all
lexical errors; parsing stops at the first syntax error.

## Checked annotations

`z200c` records a checked annotation table:

- `types`: the checked mode of every expression, keyed by span;
- `resolutions`: whether each `Apply` is a mode conversion or a procedure call.

`ir::lower` turns these annotations into a typed expression IR; the interpreter consumes
the IR instead of re-inferring arithmetic result modes and conversion targets. See
[ir.md](ir.md). Range and overflow checks remain runtime conditions.

## Fixtures

Negative and positive cases live under:

- `tests/parse_ok`, `tests/parse_fail`
- `tests/sema_fail`
- `tests/run_ok`

A `.ch` fixture may have a sibling `.error` file with a substring expected in the
diagnostic message.
