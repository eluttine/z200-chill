# Native C Backend

`z200c compile [path]` emits C and uses the system C compiler to produce a native
executable in `target/z200c/<package>`.

The interpreter remains the reference implementation. The native backend deliberately
accepts only a small, deterministic subset and reports a clear diagnostic for anything
else.

## Current native subset

- one or more non-spec modules,
- `DCL` variables of integer, boolean, character, and character-string modes,
- initializers,
- assignments to simple variables,
- integer/boolean arithmetic, comparisons, and logical operators,
- `ASSERT`,
- `IF` / `ELSE IF` / `ELSE`,
- procedures with `IN` parameters and optional integer/boolean/character/string results,
- procedure calls, `RETURN`, and `RESULT`,
- integer conversions in the generated C representation,
- one-dimensional scalar arrays with indexing and element assignment,
- `WRITE` and `WRITELINE` with one value argument.

Generated code is C11 with `int64_t` integers, `int` booleans/characters, and
`const char *` strings.

## Not supported yet

- non-`IN` procedure parameters,
- generics,
- moreta modes and objects,
- multidimensional arrays and array initializers,
- sets, powersets, references, text, and buffers,
- exceptions and handlers,
- processes, regions, tasks, signals, and timing,
- runtime range/overflow checks,
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
