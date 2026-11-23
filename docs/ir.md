# Typed IR

`z200c` lowers checked programs into a typed expression and statement IR before
interpretation.

## Pipeline

```text
source -> tokens -> AST -> checker (types + resolutions) -> typed IR -> interpreter
```

- The checker records the checked mode of every expression and resolves each `Apply`
  node as either a mode conversion or a procedure call.
- `ir::lower` walks the checked AST and produces `ir::Expr` and `ir::Stmt` nodes.
- `ExprTable` maps AST expression spans to `Rc<Expr>` roots.
- `BodyTable` maps module and procedure spans to their lowered `Stmt` bodies.
- The interpreter executes `ir::Stmt` and `ir::Expr`; it does not traverse or re-infer
  the parsed action tree.

## Expression nodes

`ir::Expr` covers:

- integer, float, boolean, character, string, wide-string, bit-string, and `NULL`
  literals;
- variable/name references with a checked mode;
- set-element values and powerset literals;
- mode conversions, including integer and float conversions;
- procedure, process, and moreta method calls with resolved names and arguments;
- `THIS`, process `START`, `SELF` values/fields, address-of, dereference, and
  `ALLOCATE`/`GETSTACK`;
- unary `-` / `NOT`;
- binary operators with the checked result mode;
- field selection, array indexing, constant and dynamic array slices;
- array literals, labelled array tuples, and powerset literals;
- association/access operations, text I/O, and text index attributes;
- other resolved built-ins (I/O, timing, duration, and termination).

## Statement nodes

`ir::Stmt` covers:

- assignment to variables, fields, array elements, and array slices;
- `ASSERT`, procedure-call actions, `RETURN`, `RESULT`, and `EXIT`;
- `IF`, `DO` (while/ever/for), `CASE` with one or more selectors, labelled actions,
  `BEGIN`, and empty actions;
- `CAUSE`, handled actions, and nested module actions;
- process actions (`START`, `STOP`), signal send/receive, `DELAY`/`DELAY CASE`,
  buffer operations, and timing actions.

All embedded expressions are `ir::Expr`. `BEGIN` keeps its declaration list as AST
declarations (`DCL` structure), but initializer expressions are lowered IR expressions.

## What remains AST-level

- Module/procedure declaration structure, parameter lists, modes, and visibility.
- `BEGIN` declaration lists (`DCL` statements);
- nested module bodies (`Stmt::Module`).

A native or C code-generation backend is future work. The typed expression/statement
IR is the intended input to that backend.

Structure values are represented as ordered field/value pairs. Field selection lowers to
`Expr::Field`; assignments lower to `AssignTarget::Field` with a root variable and field
path, so the interpreter mutates the variable in place. Assignment and `IN` argument
passing copy the whole value, including nested structures.

## Runtime checks

The IR does not remove runtime checks. Range/overflow checks, undefined-value checks,
division-by-zero checks, and `ASSERT` failures remain interpreter responsibilities.
