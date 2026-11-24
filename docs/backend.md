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
- named set modes and set-element constants,
- powersets over integer, character, and boolean members, with literals and ranges,
  `IN`, union/difference/intersection, equality, subset ordering, structure fields,
  procedure parameters/results, and arrays of powersets,
- named structure modes with scalar/string or nested structure fields, field access,
  field assignment, whole-structure assignment, structure equality, and references to
  scalar fields,
- `READLINE` input,
- scalar `REF` modes, `NULL`, reference comparison, taking addresses of scalar
  variables and scalar array elements, dereference reads, and dereference assignment,
- `ASSERT`,
- `IF` / `ELSE IF` / `ELSE`,
- `BEGIN` blocks with scoped local declarations and shadowing,
- `CASE` with one or more selectors, typed labels, ranges, `(ELSE)`, `(*)`, and
  unparenthesized `ELSE` alternatives,
- `DO WHILE`, `DO FOR`, `DO FOR EVER`, labelled actions/blocks, and labelled or
  unlabelled `EXIT`,
- procedures with `IN`, `OUT`, `INOUT`, and `LOC` scalar and array parameters,
  including multidimensional arrays and arrays of structures, plus scalar and array
  results (including arrays of structures),
- procedure calls, `RETURN`, and `RESULT`,
- integer conversions in the generated C representation,
- scalar and structure arrays of up to four dimensions with indexing, element
  assignment, nested initializers, array-literal assignment, same-mode array copying,
  and same-mode array equality/inequality,
- `WRITE` and `WRITELINE` with one value argument,
- runtime checks for array bounds, division/modulo by zero, signed overflow in
  `+`/`-`/`*`/unary `-`, and `NULL` dereference,
- `CAUSE` and `ON` handlers on module, procedure, and action bodies, including
  catching `ASSERTFAIL`, `RANGEFAIL`, and `OVERFLOW`.

Generated code is C11 with `int64_t` integers, `int` booleans/characters, and
`const char *` strings. Structure equality is generated field by field rather than with
`memcmp`, avoiding dependence on C padding/layout.

The C compiler is selected from `Z200C_CC`, then `CC`, and defaults to `cc` on Unix and
`gcc` on Windows. The target executable uses the host executable suffix (`.exe` on
Windows).

## Not supported yet

- generics,
- moreta modes and objects,
- arrays of more complex composite element modes,
- structure initializers and structure literals,
- references to fields or composite locations, text, and buffers,
- exception propagation across separate packages,
- processes, regions, tasks, signals, and timing,
- string and powerset memory reclamation (generated concatenations and powerset
  operations allocate and are not freed),
- MSVC-specific compiler flags and discovery,
- package dependencies and grant linking,
- integer width enforcement beyond the C integer representation.

Exceptions use a generated `setjmp`/`longjmp` handler chain. Unhandled exceptions and
runtime failures print a CHILL-style code and message to stderr and exit with status 1;
they do not abort or produce a core dump. Handler
support uses C `setjmp` semantics: reading an automatic local variable in a handler is
only reliable if that variable was not modified after the protected action called
`setjmp`. Module-level state and heap-backed values are safe. A future runtime frame or
heap-allocated activation records will remove this C limitation.

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
archive yet. Exception handling is currently generated inline with `setjmp`/`longjmp`.
Runtime support will be added when native text, references, and concurrency are
implemented.
