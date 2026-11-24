# Native C Backend

`z200c compile [path]` emits C and uses the system C compiler to produce a native
executable in `target/z200c/<package>`.

The interpreter remains the reference implementation. The native backend deliberately
accepts only a small, deterministic subset and reports a clear diagnostic for anything
else.

## Backend strategy

This project uses a two-tier native strategy:

- **C backend** — a portable bootstrap/fallback backend aimed at a broad
  sequential/static subset: procedures, arrays, text, references, exceptions, runtime
  checks, and module linking.
- **LLVM backend** — the eventual full-native target for optimized code, a dedicated
  CHILL runtime ABI, moreta lowering, and concurrency.

The C backend is not intended to become the sole full-language backend. Full CHILL
concurrency is unlikely to be implemented in generated C. The interpreter remains the
reference implementation for the complete supported language.

## Current native subset

- one or more non-spec modules,
- `DCL` variables of integer, boolean, character, and character-string modes,
- initializers,
- assignments to simple variables,
- integer/boolean arithmetic, comparisons, and logical operators,
- character-string equality, inequality, ordering comparisons, concatenation, and
  `//:=` concatenation assignment,
- `ASSERT`,
- `IF` / `ELSE IF` / `ELSE`,
- `BEGIN` blocks with scoped local declarations and shadowing,
- `CASE` with one or more selectors, typed labels, ranges, `(ELSE)`, `(*)`, and
  unparenthesized `ELSE` alternatives,
- `DO WHILE`, `DO FOR`, `DO FOR EVER`, and unlabelled `EXIT` inside loops,
- procedures with `IN` parameters and optional integer/boolean/character/string results,
- procedure calls, `RETURN`, and `RESULT`,
- integer conversions in the generated C representation,
- scalar arrays of up to four dimensions with indexing, element assignment,
  nested initializers, array-literal assignment, same-mode array copying, and
  same-mode array equality/inequality,
- `WRITE` and `WRITELINE` with one value argument.

Generated code is C11 with `int64_t` integers, `int` booleans/characters, and
`const char *` strings.

## Not supported yet

- non-`IN` procedure parameters,
- generics,
- moreta modes and objects,
- arrays of structures or other composite element modes,
- sets, powersets, references, text, and buffers,
- exceptions and handlers,
- processes, regions, tasks, signals, and timing,
- runtime range/overflow checks,
- string memory reclamation (generated concatenations allocate and are not freed),
- package dependencies and grant linking,
- integer width enforcement beyond the C integer representation.

Unsupported constructs return an `E0601` diagnostic and do not produce a binary.

## Compilation pipeline

```text
source -> lexer/parser -> checker -> minimal C generator -> cc -> native executable
```

The generator currently walks checked AST declarations/actions. A later milestone will
move code generation onto the typed IR so that mode conversions and resolved calls are
shared with the interpreter.

## Runtime ABI

The first slice uses only the C standard library. There is no separate CHILL runtime
archive yet. Runtime support will be added when native text, references, exceptions, and
concurrency are implemented.
