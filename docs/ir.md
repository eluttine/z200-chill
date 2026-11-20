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

- integer, boolean, character, and string literals;
- variable/name references with a checked mode;
- mode conversions with the resolved `IntKind`;
- procedure calls with resolved names and arguments;
- unary `-` / `NOT`;
- binary operators with the checked result mode.

## Statement nodes

`ir::Stmt` covers:

- assignment (the v0 target is always a name);
- `ASSERT`, procedure-call actions, `RETURN`, `RESULT`, `EXIT`;
- `IF`, `DO` (while/ever/for), `CASE`, labelled actions, `BEGIN`, empty actions.

All embedded expressions are `ir::Expr`. `BEGIN` keeps its declaration list as AST
declarations (`DCL` structure), but initializer expressions are lowered IR expressions.

## What remains AST-level

- Module/procedure declaration structure, parameter lists, modes, and visibility.
- `BEGIN` declaration lists (`DCL` statements).

A native or C code-generation backend is future work. The typed expression/statement
IR is the intended input to that backend.

## Runtime checks

The IR does not remove runtime checks. Range/overflow checks, undefined-value checks,
division-by-zero checks, and `ASSERT` failures remain interpreter responsibilities.
