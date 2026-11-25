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

- one or more non-spec modules, including `SEIZE`/`GRANT` access to variables,
  procedures, named modes, and their set-element constants from other modules and
  source package dependencies; seized modes work in arrays and nested structure
  definitions, and module order does not matter,
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
- `REF` modes including scalar variables, structure fields, whole structures, whole
  arrays, multidimensional arrays, and array elements; `NULL`, reference comparison,
  dereference reads and assignment, field access through structure references, indexed
  dereferences, and passing dereferenced arrays to array parameters,
- `ASSERT`,
- `IF` / `ELSE IF` / `ELSE`,
- `BEGIN` blocks with scoped local declarations and shadowing,
- `CASE` with one or more selectors, typed labels, ranges, `(ELSE)`, `(*)`, and
  unparenthesized `ELSE` alternatives,
- `DO WHILE`, `DO FOR`, `DO FOR EVER`, labelled actions/blocks, and labelled or
  unlabelled `EXIT`,
- procedures with `IN`, `OUT`, `INOUT`, and `LOC` scalar and array parameters,
  including multidimensional arrays, arrays of structures, and array literals passed
  directly to `IN` parameters, plus scalar and array results (including arrays of
  structures),
- procedure calls, `RETURN`, and `RESULT`,
- integer conversions in the generated C representation,
- scalar and structure arrays of up to four dimensions with indexing, element
  assignment, nested initializers, array-literal assignment, same-mode array copying,
  and same-mode array equality/inequality,
- one-dimensional array slices with constant or runtime bounds, `UP` sizes, slice
  reads, slice assignment, slice arguments, and slice equality. Runtime slices carry a
  data pointer and element count and check length when converted to fixed array modes,
- constant-bound multidimensional slices, including strided reads, slice assignment,
  slice arguments, and slice equality. Materialized multidimensional slices use
  row-major temporary arrays,
- narrow sequential `TEXT(n)` records, including assignment, equality, direct
  procedure parameters, and structure fields,
- `SETTEXTINDEX` / `GETTEXTINDEX`,
- disconnected `WRITETEXT` / `READTEXT` on narrow sequential `TEXT(n)` and varying
  `CHARS`/`CHARS(n)` locations, with literal text, conversions, fixed and variable
  widths, left alignment, padding, overflow markers, editing clauses (`X`, `>`, `<`,
  `T`), and io-control clauses (`%/`, `%+`, `%-`, `%?`, `%!`, `%=`). Writes support
  `%C`, `%B`, `%O`, and `%H` for `INT`, `BOOL`, `CHAR`, and strings. Reads support
  `%C`, `%B`, `%O`, and `%H` for `INT`, `BOOL`, `CHAR`, and strings, including widths
  and variable widths. Explicit text indices, bounded-string length checks, and
  `TEXTFAIL` checks are supported,
- `ASSOCIATION` values and narrow `ACCESS` locations over line-record files, including
  `ASSOCIATE`, `DISSOCIATE`, `ISASSOCIATED`, `CREATE`, `DELETE`, `MODIFY`, `CONNECT`,
  `DISCONNECT`, `GETUSAGE`, `OUTOFFILE`, `READRECORD`, `WRITERECORD`, and the
  `EXISTING`/`READABLE`/`WRITEABLE`/`INDEXABLE`/`SEQUENCIBLE`/`VARIABLE` attributes,
- connected narrow `TEXT(n)` locations, including indexed text modes, file-backed
  `WRITETEXT`/`READTEXT`, sequential and indexed records, and usage restrictions,
- `WRITE` and `WRITELINE` with one value argument,
- runtime checks for array bounds, division/modulo by zero, signed overflow in
  `+`/`-`/`*`/unary `-`, and `NULL` dereference,
- signed range checks for `SHORT_INT`, `INT`, and `LONG_INT` on literals, assignments,
  initializers, conversions, procedure results, array elements, and arithmetic. Values
  still use an `int64_t` C representation, with the default 16/32/64 width profile,
- `CAUSE` and `ON` handlers on module, procedure, and action bodies, including
  catching `ASSERTFAIL`, `RANGEFAIL`, and `OVERFLOW`.

Generated code is C11 with `int64_t` integers, `int` booleans/characters, and
`const char *` strings. Structure equality is generated field by field rather than with
`memcmp`, avoiding dependence on C padding/layout.

The current target platform is Linux. The C compiler is selected from `Z200C_CC`, then
`CC`, and defaults to `cc`. Windows and macOS support are out of scope for now.

## Not supported yet

- generics,
- moreta modes and objects,
- arrays of more complex composite element modes,
- structure initializers and structure literals,
- references to array slices, text, and buffers,
- dynamic multidimensional slice bounds and slice-valued references,
- wide or array-valued text modes; `GETASSOCIATION` reference results; text
  sub-location references (`GETTEXTACCESS`, `SETTEXTACCESS`, `GETTEXTRECORD`,
  `SETTEXTRECORD`); scientific floating-point text conversions,
- exception propagation across separate packages,
- processes, regions, tasks, signals, and timing,
- grant-file-only dependencies without available source,

Strings, powersets, and text records use reference-counted heap payloads. Copying a
value retains its payload, assignment releases the previous payload, and arrays,
structures, procedure parameters, procedure results, and scopes recursively copy or
drop owned elements. Module globals are released after all module bodies run. Setting
`Z200C_RC_STATS=1` prints the number of live payloads at process exit.

Scope, loop, labelled-action, early `EXIT`, and exception-unwinding paths run the
generated cleanup stack before leaving their scopes, so refcounted payloads are
reclaimed on these paths as well.

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
archive yet. Exception handling is generated inline with `setjmp`/`longjmp`, and
reference-counted payload ownership is generated inline with a small allocation
registry. A future runtime archive will expose these facilities as a stable ABI for
the LLVM backend.
