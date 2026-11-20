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
manifest declares `edition = "z200-11/99"`, matching the current in-force edition
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
- mode checking for `SHORT_INT` / `INT` / `LONG_INT` / `BOOL`, literals, arithmetic,
  relational and boolean expressions, procedures, calls, assignments, `ASSERT`,
  `IF`, `DO WHILE` / `DO FOR EVER`, and `BEGIN … END`;
- `GRANT` / `SEIZE` visibility between modules in a single entry file, including
  shared seized locations and procedures;
- `SPEC MODULE` interfaces paired with body modules, with variable and procedure
  signature matching;
- `z200c grant`, which writes a TOML grant file describing the package interface;
- loading dependency grant files from `[dependencies]` for static `SEIZE` resolution in
  `check` and `grant`;
- `DO FOR` (ascending, descending, with `BY`), `CASE`, and labelled `EXIT`;
- a tree-walking interpreter, so `z200c run examples/hello` executes the example and
  verifies its `ASSERT`.

Not implemented yet (rejected or deferred with a diagnostic):

- linking external dependencies for `run`;
- contexts and remote pieces;
- multiple `CASE` selectors and non-integer `CASE` labels;
- `OUT` / `INOUT` / `LOC` parameter passing;
- exceptions (`EXCEPTIONS`, `CAUSE`, handlers) and text I/O;
- processes, regions, timers, powersets, references, and generics.

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

### 4.3 Manifest notation

Chosen format (A): a three-number form

```toml
int_widths = "16-32-64"   # SHORT_INT-INT-LONG_INT
```

The two-number legacy form `"32-64"` is read as `INT-LONG_INT`, with `SHORT_INT`
defaulting to `INT`; this is documented, never silent. Profiles must satisfy the
containment invariant, and any violation is a manifest error.

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

---

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
- Full Z.200 also supports prefixes (`module!name`), grant windows (`ALL`, `FORBID`),
  friend clauses (`TO`), contexts, spec modules, and remote pieces.

### 7.2.1 Implemented model (v0, single entry file)

The program units in one entry file are treated as siblings in the imaginary outermost
process. Visibility is static and independent of textual order:

- `GRANT a, b;` may only name variables or procedures **declared in the same module**.
  Granted names become candidates for `SEIZE` by other modules.
- `SEIZE a, b;` imports each name from the unique other module that granted it.
  Seizing an ungranted name is an error; a name granted by more than one module is an
  error (v0 has no prefixes to disambiguate).
- Names are private unless granted and seized. A module cannot see another module's
  private names.
- A seized variable aliases the granting module's location, so assignments are shared.
- A seized procedure executes in its **defining** module's scope; it does not see the
  caller's module or local names.
- A module cannot seize a name it grants itself.
- `GRANT ALL`, `FORBID`, `TO`, prefixes, contexts, remote pieces, and cross-package
  grant-file consumption are not implemented yet.

### 7.3 SPEC modules (§10.10.2)

A spec module declares the static interface of a module body:

```text
name: SPEC MODULE
  ... declarations and procedure signatures ...
  GRANT ... ;
END name;

name: MODULE SPEC          -- alternative spelling in Z.200 (11/99)
  ...
END name;
```

Implemented v0 rules:

- A spec must be named and must have a body module with the same name. Module names
  must be unique within their kind, so one spec and one body may share a name.
- A spec body may contain `DCL` declarations, procedure signatures
  (`P: PROC (...) [RETURNS (...)] END;`), `GRANT`, and `SEIZE`.
- Spec variables must not have initializers, and spec modules cannot contain actions.
- The body must declare every spec variable with the same mode and every spec procedure
  with a matching parameter/result signature. Extra private declarations are allowed.
- If a spec exists, `GRANT` must appear in the spec rather than the body. The spec's
  grants are the module's public interface.
- At runtime, spec grants resolve to the paired body's variables and procedures.

`z200c grant` writes the compiled interface to
`target/z200c/<package>.grt.toml`:

```toml
schema = 1
package = "spec"
edition = "z200-11/99"
int_widths = "16-32-64"

[[exports]]
name = "Bump"
kind = "procedure"

[[exports]]
name = "value"
kind = "variable"
mode = "INT"
```

A package can consume another package's interface through `[dependencies]`:

```toml
[dependencies]
lib = "../lib/target/z200c/lib.grt.toml"
```

- Paths are resolved relative to the depending package's directory.
- The dependency's `edition` and `int_widths` must match the consuming package.
- Names exported by both the program and a dependency are an error.
- `z200c check` and `z200c grant` resolve `SEIZE` against these external exports.
- `z200c run` does not link external dependencies yet; it rejects a run only when an
  external name is actually seized.

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

### 10.1 Integer mode spelling (resolved)

Z.200 defines only `INT`; extra integer modes are single-identifier names such as
`LONG_INT`. The project standardizes on `SHORT_INT`, `INT`, and `LONG_INT` everywhere,
including conversions (`LONG_INT(b)`). The non-standard two-word spellings and the bare
`LONG` conversion have been removed from the example and from `AGENTS.md`.

### 10.2 `int_widths` notation (resolved)

Three-number form `"16-32-64"` = `SHORT_INT-INT-LONG_INT`. The legacy two-number form
`"32-64"` is read as `INT-LONG_INT`, with `SHORT_INT` = `INT`. See §4.3.

### 10.3 Harness output convention (resolved)

The hello example verifies itself with the standard `ASSERT` action, so `run_ok` does not
need I/O:

- expected stdout: empty;
- expected exit status: success;
- unhandled `ASSERTFAIL`/exception: failure.

### 10.4 Grant-file / multi-module model (static model resolved; linking open)

v0 implements `SPEC MODULE` pairing, writes a TOML grant file from `z200c grant`, and
consumes dependency grant files for `SEIZE` resolution in `check` and `grant`. The
dependency's edition and width profile must match. Linking external dependencies for
`run`, symbol mangling, and executing dependency code are still open. Z.200 permits
several piecewise schemes (§10.10); the next step is a linking strategy while rejecting
unsupported schemes with clear diagnostics.

### 10.5 Range-check default (resolved for v0)

The interpreter always checks integer range/overflow conditions. Z.200 leaves these
checks optional unless a handler is present (§1.1, §1.8); a configurable flag can be
added with the native backend.

### 10.6 Width defaults (resolved)

Confirmed: `SHORT_INT` = 16, `INT` = 32, `LONG_INT` = 64 on Linux. The standard does
not constrain them.

---

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
