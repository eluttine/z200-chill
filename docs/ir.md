# Typed Expression IR

`z200c` lowers checked expressions into a small typed IR before interpretation.

## Pipeline

```text
source -> tokens -> AST -> checker (types + resolutions) -> typed expression IR -> interpreter
```

- The checker records the checked mode of every expression and resolves each `Apply`
  node as either a mode conversion or a procedure call.
- `ir::lower` walks the checked AST and produces `ir::Expr` nodes. Every node carries its
  checked `Type` (procedure-call actions may carry `None` when no value is used).
- The interpreter evaluates `ir::Expr` nodes; it does not re-infer arithmetic result
  modes or conversion targets.

## Node kinds

`ir::Expr` covers:

- integer, boolean, character, and string literals;
- variable/name references with a checked mode;
- mode conversions with the resolved `IntKind`;
- procedure calls with resolved names and arguments;
- unary `-` / `NOT`;
- binary operators with the checked result mode.

`ExprTable` maps AST spans to `Rc<Expr>` roots, so an action can look up the lowered
expression for each of its expression fields without duplicating the AST.

## What remains AST-level

Statements/actions are still executed from the parsed AST:

- `IF`, `DO`, `CASE`, `BEGIN`, assignments, `RETURN`/`RESULT`, `EXIT`, `ASSERT`.

A statement-level IR and a code-generation backend are future work. The typed
expression layer is the reusable base for that backend: it already has resolved modes,
conversion targets, and call/expression kinds.

## Runtime checks

The IR does not remove runtime checks. Range/overflow checks, undefined-value checks,
division-by-zero checks, and `ASSERT` failures remain interpreter responsibilities.
