# z200-chill — Language Subset and Semantics Notes

Status: **draft for review**. This document records the CHILL subset that `z200c` intends
to implement, the rules taken directly from the standard, and the decisions that the
project still has to make. It is deliberately explicit about which statements are
normative Z.200 and which are z200-chill design choices.

Normative reference:

- **ITU-T Recommendation Z.200 (11/99)**, *CHILL — The ITU-T Programming Language*,
  approved 1999-11-19; identical text published as **ISO/IEC 9496:2003**.
  Freely available: <https://www.itu.int/rec/T-REC-Z.200-199911-I/en>
  (PDF mirror used while writing this note:
  <https://psc.informatik.uni-jena.de/publ/1999-T-REC-Z.200-199911-I!!PDF-E[1].pdf>).

Superseded editions (useful for historical compatibility work): Z.200 (1984), (1988),
(1992), (1993), (1996). The GNU CHILL compiler implemented mostly Z.200/1988 and was
removed from GCC in 2002 (GCC 3.0 omitted it; the source was deleted in April 2002).

Section references below are to Z.200 (11/99) unless stated otherwise. The example
compiler targets `z200-11/99`, the current in-force edition
(11/99); every construct used by the subset and the hello example is also valid in
earlier editions, but 1999 is the normative target.

---

## 1. Scope of the initial subset

The first milestone (`v0`) should cover:

- lexical structure, comments, integer/boolean/character/string literals;
- mode names and the integer modes used by the project's width profiles;
- `DCL` location declarations and `INIT :=` initialization;
- expressions: arithmetic, relational, boolean, mode conversion;
- actions: assignment, `IF`, `DO`, `CASE`, `EXIT`, `BEGIN … END`, `ASSERT`, `CAUSE`;
- procedure definitions and calls (`PROC`, `IN`/`OUT`/`INOUT`/`LOC`, `RETURNS`);
- `RESULT` / `RETURN`;
- modules and the `GRANT` / `SEIZE` visibility mechanism.

Explicitly **out of scope for v0** (keep the frontend/IR boundary clean for them):

- processes, `START`/`STOP`/`DELAY`, events, buffers, signals;
- regions and mutual exclusion;
- timers (`DURATION`, `TIME`, `TIMEOUT`);
- powersets, sets, bit strings, references, rows, `ALLOCATE`/`TERMINATE`;
- text I/O (`READTEXT`/`WRITETEXT`), associations/accesses;
- generics, moreta (module/region/task) modes, object orientation;
- `SPEC`/`CONTEXT`/`REMOTE` piecewise compilation beyond the simple two-module case
  described in §7.

### 1.1 Implementation status (v0)

Implemented in `z200c`:

- manifest loading and width-profile validation;
- lexer, parser, and AST for the constructs in the subset above;
- mode checking for `SHORT_INT` / `INT` / `LONG_INT` / `FLOAT` / `BOOL` / `CHAR` /
  `CHARS`, `CHARS(n)` / `CHARS(n) VARYING`, literals, arithmetic, and one-dimensional
  arrays of scalar or structure modes (`ARRAY (low:high) mode`),
  relational and boolean expressions, procedures, calls, assignments, `ASSERT`,
  `IF`, `DO WHILE` / `DO FOR EVER`, and `BEGIN … END`;
- `GRANT` / `SEIZE` visibility with module/package-qualified names (`Module!x`,
  `pkg!Module!x`), `GRANT ALL` / `SEIZE ALL`, `PREFIXED`/prefix-rename output, `TO`
  friend restrictions, and static structure-mode `FORBID`, including shared seized
  locations and procedures;
- `SPEC MODULE` interfaces paired with body modules, with variable and procedure
  signature matching;
- `CONTEXT ... FOR` lists on top-level and nested body modules, with quasi
  variable/procedure declarations checked against the enclosing reach; later contexts
  may `SEIZE` earlier context declarations or `GRANT` their own;
- nested body modules executed as actions, with `SEIZE` from the immediate parent
  reach and `GRANT` of their own, child, or grandchild variable declarations into that
  reach; siblings and parents can seize/re-grant those grants, action blocks (`BEGIN`,
  `IF`, `CASE`, `DO`) form their own reaches, and declarations are hoisted so runtime
  visibility is order-independent; nested modules may define modes and procedures and
  grant/re-grant them with `PREFIXED`/`TO`/`FORBID` (see
  [contexts.md](contexts.md));
- `z200c grant`, which writes a TOML grant file describing the package interface and
  (optionally) its source package directory;
- loading dependency grant files from `[dependencies]` for static `SEIZE` resolution in
  `check` and `grant`, and source-linking them for `run` when `package_dir` is present;
- `DO FOR` (ascending, descending, with `BY`), `CASE`, and labelled `EXIT`;
- a typed expression and statement IR (checked modes, resolved `Apply` nodes, lowered
  actions) consumed by the interpreter instead of re-inferring or walking the AST;
- multi-file `[build] sources`, package-directory dependency linking, and
  package-qualified module identity for `run`;
- action-level handlers on bracketed actions, assignment, assert, and call actions,
  including handlers that see `BEGIN` block declarations;
- static exception-set flow checking for `CAUSE`, `ASSERT`, and procedure calls, with
  `EXCEPTIONS` lists enforced and matched between spec and body signatures;
- source-map diagnostics with stable codes, multi-error checker reporting, a parser
  nesting limit, and fixture-based parser/sema/run tests;
- a tree-walking interpreter, so `z200c run examples/hello` executes the example and
  verifies its `ASSERT`.

Not implemented yet (rejected or deferred with a diagnostic):

- interface-only external dependencies for `run`;
- multiple `CASE` selectors and non-integer `CASE` labels;
- `OUT` / `INOUT` / `LOC` parameter passing;
- standard `READTEXT`/`WRITETEXT` and associations (z200c instead provides the
  implementation-defined `WRITE`, `WRITELINE`, and `READLINE` below);
- region concurrency/mutual exclusion;
- processes, timers, powersets, references, and generics.

The interpreter is a bootstrap for validating semantics; native code generation is a
later milestone.

---

## 2. Lexical rules (verified)

- CHILL is **case sensitive**: `Status` and `status` are different name strings
  (Z.200 §2.2).
- Reserved words are uppercase (Appendix III); the full list includes `DCL`, `PROC`,
  `MODULE`, `SPEC`, `GRANT`, `SEIZE`, `IF`, `THEN`, `ELSE`, `ELSIF`, `FI`, `DO`, `OD`,
  `CASE`, `OF`, `ESAC`, `RETURN`, `RESULT`, `EXIT`, `BEGIN`, `END`, `RETURNS`,
  `EXCEPTIONS`, `IN`, `OUT`, `INOUT`, `LOC`, `INT`, `BOOL`, `CHAR`, etc.
- Comments (Z.200 §2.4):
  - bracketed: `/* … */` (terminated by the first `*/`);
  - line-end: `-- …` (the `--` operator).
  A comment may appear wherever a delimiter (space) is allowed.
- Relational "not equal" is `/=`. **`<>` is not inequality**; it delimits compiler
  directives (`<> … <>`) (Appendix II).
- Assignment is `:=`; compound assignment is operator + `:=` (e.g. `+:=`, `-:=`) (§6.2).
- String concatenation is `//`. Mode/location conversion uses `#` (expression conversion)
  and `mode(...)` (representation conversion) (§5.2.11–5.2.12).

---

## 3. Modes and classes

CHILL separates **modes** (types, attached to locations and strong values) from
**classes** (used for compatibility checking). The distinction matters for how integer
literals and conversions are typed.

### 3.1 What the standard defines

- A mode defines a set of values and the operations allowed on them (§3.3).
- The only **predefined integer mode name** is `INT` (§3.4.2).
- `INT`'s bounds are **implementation defined**. The standard does not require any
  particular width, and does not define `SHORT INT`, `LONG INT`, or `LONG` (§3.4.2,
  §13.2).
- An implementation *may* define additional integer modes, e.g. short/long/unsigned
  integers. They **must be denoted by implementation-defined integer mode names** and
  are treated as if introduced by an unspecific `NEWMODE` (§1.13, §13.2). The standard
  suggests names such as `LONG_INT`, `SHORT_INT`, `UNSIGNED_INT` — these are identifiers,
  **not multi-word keywords**.
- The virtual mode `&INT` contains all values of all predefined integer modes
  (§3.4.2). It is not a mode that can be named in a declaration.
- An integer literal has class **`&INT`-derived**, not a fixed mode (§5.2.4.2). A literal
  is compatible with any integer mode whose value set contains it. This is why `2` can
  be used where a `LONG_INT` is expected.

### 3.2 Mode definitions

- `SYNMODE name = mode;` — a synonym; same mode, no new identity.
- `NEWMODE name = mode;` — a new, distinct mode (novelty). This is how a project would
  introduce `SHORT_INT` / `LONG_INT` as distinct modes (§3.2).
- `NEWMODE name = STRUCT(field mode, ...);` — a structure mode with scalar/nested fields.
  Field selection uses `.` and can appear in expressions and assignment targets.
  `GRANT S FORBID (field, ...)` / `FORBID ALL` prevents field selection outside the
  defining module (see [grant-file.md](grant-file.md)).

Structure values are fully interpreted: field selection and assignment (including nested
fields), whole-value assignment with copy semantics, `IN` parameters, results, and
initialization from another structure value. Fields start undefined and reading one
before assignment is an error. Equality and inequality are defined recursively for
values of the same structure mode (§5.3.5); ordering comparisons remain undefined for
structures. There are no structure literals in the v0 subset.

For `v0`, assign the implementation-defined integer mode names directly rather than
requiring the user to write `NEWMODE` declarations.

### 3.3 Conversions

- **Expression conversion** `mode # (expr)` attaches a mode without changing the
  representation; the source and target must have the same size (§5.2.11).
- **Representation conversion** `mode (expr)` attaches a mode and may change the
  representation. For discrete-to-discrete conversions the numeric value is preserved:
  `NUM(mode(expr)) = NUM(expr)` (§5.2.12). This is the form used by `LONG_INT(b)` in
  the example.
- Representation conversion overrides normal mode checking, so `LONG_INT(b)` is legal
  even though the source mode differs. A narrowing conversion that cannot represent the
  value is an error/range failure; widening is always safe.

> **Resolved:** standard CHILL mode names are single identifiers. The example now uses
> `SHORT_INT` / `INT` / `LONG_INT` consistently, both in declarations and in the
> conversion `LONG_INT(b)`. `INT` is the only integer mode predefined by Z.200; the
> other two are implementation-defined integer mode names (§13.2).

---

## 4. Integer width rules

### 4.1 What Z.200 requires

Z.200 requires essentially only this:

- each integer mode has implementation-defined lower/upper bounds (§3.4.2);
- `&INT` contains the values of every predefined integer mode (§3.4.2);
- arithmetic on integer values follows the mode/class rules of §5.3, with an
  `OVERFLOW` exception when a non-constant operation produces a value outside the
  result's root mode (§5.3.6–5.3.7).

Z.200 does **not** require `SHORT_INT` to be narrower than `INT`, nor `INT` narrower than
`LONG_INT`, and it does not prescribe two's-complement representation.

### 4.2 z200-chill profile (project rule)

`z200c` fixes the Linux profile:

| Mode        | Width *n* | Range |
|-------------|-----------|-------|
| `SHORT_INT` | *s* = 16  | −2^(s−1) … 2^(s−1)−1 |
| `INT`       | *i* = 32  | −2^(i−1) … 2^(i−1)−1 |
| `LONG_INT`  | *l* = 64  | −2^(l−1) … 2^(l−1)−1 |

Integer widths are a target/toolchain property, not a package manifest setting. The
invariant `width(SHORT_INT) <= width(INT) <= width(LONG_INT)` holds by construction.

### 4.3 Arrays

`ARRAY (low:high, ...) element-mode` declares an array with one or more constant
integer index ranges. Up to four dimensions are supported. The element mode may be
scalar (`SHORT_INT`/`INT`/`LONG_INT`, `FLOAT`, `BOOL`, `CHAR`, `CHARS`, or bounded
character strings) or a structure mode.

- `a(i)` selects an element from a one-dimensional array; `a(i, j, ...)` selects an
  element from a multidimensional array. Both may be read or used as assignment
  targets.
- Structure elements support field selection and field assignment:
  `a(i).field`, including nested fields.
- Bounds are checked at runtime.
- Whole-array assignment copies the value.
- Equality and inequality are defined for arrays of the same mode.
- As with whole structure assignment, reading a structure element field before it is
  assigned raises the undefined-value runtime error.
- Array element modes cannot themselves be arrays.
- Array tuples are accepted where a one-dimensional target array mode is known:
  `[1, 2, 3]` or `(: 1, 2, 3 :)` (unlabelled), and
  `[ (1, 2): 5, (3:4): 7 ]` (labelled). Unlabelled tuples must have exactly as many
  elements as the target; labelled tuples use integer labels, ranges, `ELSE`, and
  `(*)`, must cover every index exactly once, and must stay within the target bounds.
  Tuple values must be assignable to the element mode. This works for assignment,
  `INIT :=`, procedure arguments, `RETURN`, `RESULT`, and array equality.
- Slices are available as values and as assignment targets:
  `a(lower:upper)` and `a(first UP size)`. The slice keeps the original array's lower
  bound; its upper bound is the lower bound plus the slice length minus one.
- Constant slice bounds are checked at compile time. If a bound is not a constant
  expression, the slice has a dynamic mode and its bounds are checked when it is read
  or assigned.
- A dynamic slice may be assigned to a fixed array (or passed to a fixed array
  parameter) when the lower bound and element mode agree; the runtime checks that the
  slice length matches the target. It can be compared with a fixed array of the same
  lower bound and element mode.
- Slice assignment copies elements into the source range at runtime.
- Whole-array assignment and equality work for multidimensional arrays with the same
  shape and element mode.
- Multidimensional array elements can be read through field selection
  (`a(i, j).field`); assigning through a field of a multidimensional element is not
  implemented yet.
- Array slices currently apply to one-dimensional arrays only.
- Array literals currently apply to one-dimensional arrays only.


### 4.3.1 Floating-point mode

`FLOAT` is the implementation-defined floating-point mode, represented as an IEEE-754
double. Floating-point literals use a decimal point and/or exponent (`1.5`, `1e3`).
Arithmetic supports `+`, `-`, `*`, and `/`; comparisons and equality are defined for
matching `FLOAT` operands. `FLOAT(expr)` converts an integer or floating-point value.
Floating-point values may be array elements and procedure parameters/results.

### 4.3.2 Powersets

`POWERSET member-mode` declares a powerset whose members are discrete scalar values
`INT`/`SHORT_INT`/`LONG_INT`, `BOOL`, or `CHAR`.

- Tuple literals supply members and inclusive ranges:
  `['A':'C', 'x']`, `[1, 3:5]`, `[TRUE]`, or `[]`.
- `x IN s` tests membership.
- `+`, `-`, and `*` perform union, difference, and intersection.
- `=`, `/=`, `<=`, `>=`, `<`, and `>` compare powersets by value/subset ordering.
- Powerset literals take their member mode from context, like array tuples.

Named set modes are declared through `NEWMODE`/`SYNMODE`:

```chill
NEWMODE status = SET(occupied, free);
NEWMODE code = SET(one = 1, three = 3);
```

Unnumbered elements receive representation values 0, 1, ...; numbered elements use the
explicit non-negative values. Set element names are visible in the defining reach and
may be assigned and compared. Named set modes are module-local for now: they are not
included in grant interfaces, and powersets currently accept only `INT`/`BOOL`/`CHAR`
members.

### 4.4 Runtime checks

- Assignment checks that the value lies in the target mode's value set. For plain
  integer modes this is the assignment condition; for range modes a violation raises
  `RANGEFAIL` (§6.2).
- Non-constant arithmetic that leaves the root mode raises `OVERFLOW` (§5.3.6–5.3.7).
  Whether checks are emitted is implementation-defined unless a handler is present
  (§1.1, §1.8); z200-chill should emit them by default (project choice).

---

## 5. Declarations and statements

### 5.1 Location declarations (§4.1.2)

Syntax (simplified):

```text
DCL defining-occurrence-list mode [ STATIC ] [ := value | INIT := constant ] ;
```

- `DCL a, b SHORT_INT;`
- `DCL answer LONG_INT INIT := 0;`
- `INIT :=` is a lifetime-bound initialization (once, at the start of the location's
  lifetime). `:=` is reach-bound (each time the reach is entered).
- A module-level declaration has the lifetime of the enclosing block; modules restrict
  visibility but do **not** change lifetime (§10.6, §10.9).

### 5.2 Assignment (§6.2)

```text
location := value ;
location +:= expression ;      -- and -:=, *:=, /:=
```

The modes of all target locations must be equivalent and writable; each must be
compatible with the value's class. The value must lie in the mode's value set.

### 5.3 Control flow (§6.3–6.5)

```chill
IF cond THEN
  ...
ELSIF cond THEN
  ...
ELSE
  ...
FI
```

```chill
DO WHILE cond;
  ...
OD

DO FOR i := 1 TO n;
  ...
OD

DO FOR i := 1 BY 2 TO n;
  ...
OD

DO FOR i := 10 BY 1 DOWN TO 1;
  ...
OD

DO FOR EVER;
  ...
OD
```

- `BY` is optional (default step 1) and must be positive; `DOWN` iterates downward.
- The loop counter is implicitly declared in the loop's own reach (it shadows an outer
  name of the same spelling). The loop terminates before an overflowing next value.
- The counter's mode follows the resulting class of the start/step/end expressions; in
  v0 a purely literal counter behaves as an `&INT`-derived value.

```chill
CASE selector OF
  (1): ...
  (2, 3): ...
  (4:6): ...
  (ELSE): ...
  ELSE
    ...
ESAC
```

- v0 supports one integer selector, integer labels, literal ranges, `(ELSE)`, `(*)`,
  and an optional unparenthesized `ELSE` branch.
- `(ELSE)` matches selector values not explicitly listed by any alternative; the
  unparenthesized `ELSE` branch runs when no alternative matches.
- Multiple selectors and non-integer label values are not implemented yet.

Actions can be labelled and left with `EXIT`:

```chill
Outer: DO FOR i := 1 TO 3;
  DO FOR j := 1 TO 3;
    IF done THEN
      EXIT Outer;
    FI;
  OD;
OD;
```

- `EXIT label;` leaves the matching labelled action or named module (§6.6).
- `EXIT;` leaves the innermost `DO`, `BEGIN`, or module.
- A label is visible only inside the labelled action; unknown labels are errors.

`ASSERT boolean-expression;` raises the `ASSERTFAIL` exception if the expression is
`FALSE` (§6.10). `CAUSE name;` raises a named exception (§6.12).

Handlers may be appended to a module, procedure, or action statement (Z.200 §8.2):

```text
END
ON (exception-list): actions
   (other-exceptions): actions
ELSE actions
END
```

For action statements, the handler follows the action and precedes the terminating
semicolon, for example:

```chill
BEGIN
  ...
END
ON (OVERFLOW):
  ...
END;

a +:= 1
ON (OVERFLOW):
  ...
END;
```

- A matching `CAUSE` name runs the corresponding alternative; `ELSE` runs when no
  alternative matches.
- `ASSERTFAIL` and `OVERFLOW` also behave as catchable exceptions.
- Unhandled exceptions terminate the program with `E0509`.
- A handler appended to a `BEGIN` block runs in that block's reach and can see its
  declarations. Handlers on other action statements run in the enclosing reach.
- Exception-set flow checking tracks explicit `CAUSE` names, `ASSERTFAIL` from
  `ASSERT`, and the declared exception sets of called procedures. A handler removes
  the exceptions it catches (or all exceptions with `ELSE`); exceptions raised by
  handler actions still escape. A procedure with an `EXCEPTIONS` list may only let
  those exceptions escape; an absent list means an empty set. Spec and body procedure
  signatures must declare the same exception list.
- Implicit runtime checks (`OVERFLOW`, range failures, array bounds, text failures)
  are not inferred into the static exception set yet. Procedures loaded from grant
  files have an unknown exception set and do not contribute to the caller's set.

---

### 5.5 Text I/O (implementation-defined)

Z.200 permits implementation-defined built-in routine names. `z200c` provides:

- `WRITE(chars);` writes to stdout without a newline.
- `WRITELINE();` / `WRITELINE(chars);` writes a line to stdout.
- `line := READLINE();` reads one stdin line as `CHARS`, without the terminator.

`READTEXT`/`WRITETEXT` are also supported for **`CHARS` locations with literal format
text**:

- `WRITETEXT(text, format)` writes at index 0.
- `WRITETEXT(text, index, format)` writes at an integer index, padding with spaces if
  the index is beyond the current length.
- `READTEXT(text, format)` / `READTEXT(text, index, format)` check that the text matches
  the literal format and raise the `TEXTFAIL` exception otherwise.
- Format text must be a literal in the current subset.
- Supported format elements:
  - conversions `%B` (binary), `%O` (octal), `%H` (hexadecimal), `%C` (default), and
    `%F` (scientific floating point);
  - conversion qualifiers `L` (left justification), `E` (overflow `*`), and
    `P<char>` (padding character);
  - fixed clause widths such as `%C5`, variable widths `%CV` / `%XV`, fractional widths
    such as `%C.2` / `%F.2`, exponent widths such as `%F:3`, and repetition factors
    such as `%3C`;
  - editing clauses `%X` (space), `%>` (skip right), `%<` (skip left), and `%T`
    (tabulate), with optional widths;
  - I/O control clauses `%/` (next record), `%+` (next page), `%-` (current line),
    `%?` (prompt), `%!` (emit), and `%=` (end page);
  - `%%` for a literal percent and ordinary format text;
  - io lists after the format argument. Each conversion takes one value/location
    (or a width value first when its width is variable).
- `READTEXT` io list elements are locations; `WRITETEXT` io list elements are values.
- Parenthesized clauses are supported, e.g. `%2(%C:%C %)`.
- Variable widths are taken from the io list in format order; for example
  `WRITETEXT(s, "%CV", 5, 42)` uses width 5 and value 42.
- For in-memory strings, record/page controls are mapped to implementation-defined
  separators: `%/` and `%?` use newline, `%+` uses form feed, and `%-`, `%!`, `%=`
  have no textual effect.
- Association `MODIFY` and indexed access are not implemented yet.
- Text locations must have a varying string mode (`CHARS` or `CHARS(n) VARYING`).

### 5.6 Text modes

`CHARS(n)` is a fixed string mode: every value must contain exactly `n` characters.
`CHARS(n) VARYING` is a bounded varying mode: values may contain between 0 and `n`
characters. Bare `CHARS` remains an unbounded varying string mode.

- Fixed and bounded varying strings are valid scalar array elements.
- Assignment checks the destination string mode's length/limit at runtime.
- String concatenation and comparison accept any mix of `CHARS`, `CHARS(n)`, and
  `CHARS(n) VARYING`; concatenation delivers an unbounded `CHARS` value.
- String literals and `READLINE` deliver unbounded `CHARS` values, so assigning them to
  a bounded string is checked at runtime.
- `WCHARS` and wide-character text are not implemented.

### 5.7 Associations and access modes (simplified)

`ASSOCIATION` is an opaque association-mode location. `ACCESS CHARS` is an opaque
sequential access-mode location for character-string line records. The current
implementation is an intentional simplification of Z.200 file handling:

- `ASSOCIATE(association, "path")` attaches an implementation-defined host-file path.
- `ISASSOCIATED(association)` reports whether an association is present.
- `EXISTING`, `READABLE`, `WRITEABLE`, `INDEXABLE`, `SEQUENCIBLE`, and `VARIABLE`
  report association attributes. In the current text-file model, readable/writeable
  follow `existing`, sequencible is always true, and indexable/variable are false.
- `DISSOCIATE(association)` clears the association.
- `CREATE(association)` creates/truncates the associated file.
- `DELETE(association)` removes the associated file.
- `CONNECT(access, association)` connects an access location and resets its record
  position to the beginning. Indexed access accepts an optional integer index:
  `CONNECT(access, association, index)`.
- `DISCONNECT(access)` clears the connection.
- `WRITERECORD(access, text)` appends one line-based record.
- `READRECORD(access)` returns the next line; `READRECORD(access, target)` stores it in
  a `CHARS` location.
- Indexed access also supports `READRECORD(access, index [, target])` and
  `WRITERECORD(access, index, text)`. Indexed writes replace the selected line.
- Out-of-range indexed transfers raise `RANGEFAIL`.
- `MODIFY(association)` validates the association and currently has no modifiable
  parameters; passing parameters is rejected.
- There is no random access outside an indexed `ACCESS` mode.

The built-in routines raise implementation-defined `ASSOCIATEFAIL`, `NOTASSOCIATED`,
`CREATEFAIL`, `DELETEFAIL`, `CONNECTFAIL`, `NOTCONNECTED`, `READFAIL`, and `WRITEFAIL`
exceptions. These implicit exceptions are not included in the static exception-flow
analysis yet.

## 6. Procedures

Syntax (§10.4), simplified:

```text
name: PROC ( param-list ) [ RETURNS ( mode ) ] [ EXCEPTIONS ( names ) ] ;
  body
END name ;
```

- Parameters: `name mode [IN|OUT|INOUT|LOC]`. The default is pass-by-value (`IN`).
  `OUT` does not copy in; `INOUT` copies in and back; `LOC` passes a location.
- `RETURNS (mode)` makes a value procedure; without it the procedure delivers no result.
- `RESULT value;` establishes the result without returning; `RETURN [value];` returns.
  `RETURN value` is derived syntax for `DO RESULT value; RETURN; OD` (§6.8).
- A value procedure's call is an expression and may appear on the right-hand side of an
  assignment.

### 6.1 Result semantics (§6.8)

- The latest executed `RESULT` determines the delivered result.
- If no `RESULT` executed, the call delivers an undefined value/location.
- With `RETURNS (mode)` and no `LOC`, the result must be a value whose class is
  compatible with `mode`.
- Reaching `END` of the procedure body is equivalent to returning.

---

### 6.2 Generic procedure templates

Procedures may be templated over modes:

```chill
Identity: GENERIC
  MODE T;
  PROC (x T) RETURNS (T);
    RETURN x;
  END Identity;

IdentityInt: PROC = NEW Identity (INT) END;
```

- `GENERIC MODE T;` declares formal mode parameters.
- Template bodies are checked after each instantiation, with formal modes substituted.
- Local declarations, parameters, results, and nested procedure signatures are substituted.
- Instantiation syntax is `name: PROC = NEW template (actual-modes) END;`.
- The current subset supports `MODE` parameters only; generic `SYN` and `PROC`
  parameters are not implemented yet.

Generic module templates use the same mechanism:

```chill
Pair: GENERIC
  MODE T;
  MODULE
    DCL first T;
    DCL second T;
  END Pair;

PairInt: MODULE = NEW Pair (INT) END;
```

- Instantiated modules are cloned with formal modes substituted through declarations,
  contexts, nested modules, actions, and handlers.
- Generic region and process templates are not implemented yet.

### 6.3 Minimal moreta module modes

The detailed supported subset and intentional exclusions are documented in
[moreta.md](moreta.md).

`NEWMODE`/`SYNMODE` may define a module-body mode with variable components:

```chill
NEWMODE Point = MODULE BODY
  DCL x INT;
  DCL y INT;
END Point;

DCL p Point;
p.x := 1;
ASSERT p.x = 1;
```

- Each object has its own variable components.
- Variable components are accessed and assigned with `.`.
- Procedure components may be defined and called as methods:
  ```chill
  ASSERT counter.Inc(2) = 3;
  counter.Reset();
  ```
- Both value-returning and non-value method actions are supported.
- Method bodies see the object's components; mutations are written back to the object.
- Component `DCL ... INIT := ...` initializers run when an object is declared and may
  reference earlier components.
- `MODULE SPEC` declares the interface components (variables and procedure
  signatures). A matching `MODULE BODY` in the same reach supplies implementations;
  every interface procedure must have a body component.
- `BASED_ON Base` derives a module mode from another module mode. Base variable and
  procedure components are inherited; same-named components in the derived body
  override the base. Base component `INIT` declarations run before derived
  initializers.
- `INVARIANT boolean-expression` at the end of a module mode body is checked before and
  after every component method call. A false invariant raises `INVFAIL`.
- Component modes/signatures are resolved when the module mode is defined.
- Whole-object assignment uses value-copy semantics.
- Module-mode types are currently module-local: exporting one through a grant
  interface is rejected with a clear error.

Not implemented yet: constructors/destructors and task/region object modes.

## 7. Modules and visibility

### 7.1 Syntax (§10.6)

```text
[name :] MODULE [BODY] module-body END [handler] [name] ;
```

The name before `:` defines a module name and a label name. The optional trailing name
must match it. A module may contain declarations, `GRANT`/`SEIZE`, and actions.

The module heading itself takes no semicolon: the form is `Hello: MODULE` followed
directly by the body. (`Hello: MODULE;` would also parse, because the `;` is an empty
action, but it is non-idiomatic and has been removed from the example.)

### 7.2 What GRANT and SEIZE do (§12.2.3.4–12.2.3.5)

- **`GRANT`** extends the visibility of names from a module's reach to the **directly
  enclosing** reach. It is the export mechanism.
- **`SEIZE`** extends the visibility of names from the enclosing reach into the
  **directly enclosed** module. It is the import mechanism.
- Neither is a runtime operation; both are compile-time visibility statements.
- Standard checks:
  - you may only `GRANT` names that are visible and grantable by the current module;
  - you may not `SEIZE` a name that is not visible/granted in the enclosing reach;
  - referring to a granted name without seizing it is an error.
- z200c also supports prefixes (`module!name`), grant windows (`ALL`, `FORBID`),
  friend clauses (`TO`), spec modules, and top-level contexts. Remote pieces and nested
  modulions are not implemented.

### 7.2.1 Implemented model (v0)

The `GRANT`/`SEIZE` model, `SPEC MODULE` pairing, grant-file schema, and dependency
rules are documented in [grant-file.md](grant-file.md). Context lists and their current
subset are documented in [contexts.md](contexts.md).

### 7.4 Complete programs (§10.8)

A complete CHILL program is a list of program units (modules, regions, spec modules,
generic declarations) considered to be enclosed in an **imaginary outermost process**.
Module-level actions execute when that reach is entered. There is no `main` function in
the language.

---

## 8. Exact semantics of `examples/hello/src/main.ch`

Source:

```chill
Hello: MODULE
  AddScaled: PROC (a SHORT_INT, b SHORT_INT) RETURNS (LONG_INT);
    DCL scale LONG_INT;
    DCL result LONG_INT;
    scale := LONG_INT(b) * 2;
    result := LONG_INT(a) + scale;
    RETURN (result);
  END AddScaled;

  DCL answer LONG_INT;
  answer := AddScaled(3, 4);
  ASSERT answer = 11;
END Hello;
```

The standard semantics is:

1. `3` and `4` are integer literals of class `&INT`-derived, constant (§5.2.4.2).
2. `AddScaled` is called with pass-by-value parameters (`IN` is the default). The
   assignment conditions require `3` and `4` to lie in `SHORT_INT`; they do.
3. Inside, `b` holds 4. `LONG_INT(b)` is a representation conversion; for discrete modes
   it preserves the numeric value, so it yields the `LONG_INT` value 4 (§5.2.12).
4. `LONG_INT(b) * 2`: the literal `2` is `&INT`-derived and compatible with the
   `LONG_INT` mode. The resulting class of a `LONG_INT` value and an `&INT`-derived
   class is the `LONG_INT` value class (§12.1.1.7), so the product is the `LONG_INT`
   value 8.
5. `scale := 8`.
6. `LONG_INT(a) + scale` = 3 + 8 = 11, of the `LONG_INT` value class.
7. `RETURN (result)` is `RETURN` with a result, derived from
   `DO RESULT result; RETURN; OD` (§6.8); the call delivers 11.
8. `answer := 11`.
9. `ASSERT answer = 11;` is `TRUE`, so no `ASSERTFAIL` exception occurs (§6.10).

Observable behaviour: the module performs no I/O, so a conforming run writes **nothing**
to stdout and terminates normally. The `ASSERT` is the self-check: if the computed value
were not 11, an unhandled `ASSERTFAIL` would make the run fail. This gives `run_ok`
something to test without requiring I/O:

- expected stdout: empty;
- expected exit status: success (0);
- an `ASSERTFAIL` (or any other unhandled exception) means failure.

If textual output is desired later, Z.200 provides `WRITETEXT` (§7.5.3). Z.200 does not
predefine `STDOUT`; the target text location is implementation-defined (GNU CHILL added
`STDOUT`). That belongs to the runtime/I-O milestone, not v0.

---

## 9. Relationship to the project's stated design

| `AGENTS.md` principle | Z.200 basis | Status |
|---|---|---|
| Separate frontend / IR / backend | Standard has context-free syntax + separate semantics (§2.1) | compatible |
| Model `MODULE`/`SPEC`/`GRANT`/`SEIZE` | §10.6, §10.10, §12.2.3 | compatible; v0 simplified |
| `SHORT_INT`/`INT`/`LONG_INT` widths | Implementation-defined integer modes (§13.2); only `INT` is predefined | resolved: single-identifier mode names |
| Width containment invariant | Not required by Z.200 | project rule; document it |
| Profile in build-cache key | Not a language issue | project rule |
| Isolate runtime semantics | Exceptions/processes/signals are language-level (§1.8, §1.11) | compatible |

---

## 10. Decisions

Resolved and open project decisions are tracked in [decisions.md](decisions.md).

## 11. References

Primary:

- ITU-T Z.200 (11/99), *CHILL — The ITU-T Programming Language* / ISO/IEC 9496:2003.
  <https://www.itu.int/rec/T-REC-Z.200-199911-I/en>
- ITU-T Z.200 (11/88), (11/93), (10/96) — superseded editions.
- CCITT, *CHILL User's Manual*, Geneva 1986 (more readable than the standard).

Secondary / prior art:

- GNU CHILL front end (Z.200/1988 based), shipped through GCC 2.95.3 and removed in
  2002. Manual: <https://gcc.gnu.org/onlinedocs/gcc-2.95.3/chill.html>
- F. S. University Jena, CHILL homepage and CHILL2000 tutorial:
  <https://psc.informatik.uni-jena.de/languages/chill/chill.htm>
- J. F. H. Winkler, *CHILL 2000*, Telektronikk 4/2000.
- K. Rekdal, *CHILL — the international standard language for telecommunications
  programming*, Telektronikk 1993.

Useful section map for the subset:

| Topic | Z.200 (11/99) |
|---|---|
| Implementation options (extra integer modes) | §1.13 |
| Lexical elements, comments | §2.2–2.4 |
| Mode definitions / classification | §3.2–3.3 |
| Integer modes | §3.4.2 |
| Procedure modes | §3.7 |
| Location declarations | §4.1.2 |
| Literals / integer literals | §5.2.4.1–5.2.4.2 |
| Expression / representation conversion | §5.2.11–5.2.12 |
| Arithmetic classes and overflow | §5.3.6–5.3.7 |
| Assignment | §6.2 |
| `IF` / `CASE` / `DO` | §6.3–6.5 |
| `ASSERT` / `CAUSE` | §6.10, §6.12 |
| `RESULT` / `RETURN` | §6.8 |
| Text I/O | §7.5.3 |
| Procedure definitions | §10.4 |
| Modules / program / lifetime | §10.6–10.9 |
| Piecewise programming (spec/remote) | §10.10 |
| Resulting class | §12.1.1.7 |
| `GRANT` / `SEIZE` | §12.2.3.4–12.2.3.5 |
| Implementation-defined integer modes | §13.2 |
| Reserved words / special symbols | Appendices II–III |
