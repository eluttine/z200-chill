# Contexts (design)

Status: **partially implemented.** Context lists work on top-level modules and on nested
body modules; nested `SEIZE` resolves from the parent reach. Context visibility
statements, mode definitions in contexts, remote pieces, and the full reach/export model
remain future work.

## Normative model

References are to ITU-T Z.200 (11/99), identical to ISO/IEC 9496.

### Syntax (§10.10.2)

```text
<context list> ::= <context> { <context> }*
                 | <remote context>

<context> ::= CONTEXT <context body> FOR

<context body> ::=
    { <quasi data statement> | <visibility statement>
    | <spec module> | <spec region> }*
```

A context list may precede a module, region, spec module or spec region (§10.6, §10.7,
§10.10.2). `CONTEXT MODULE REMOTE <piece designator> ;` is derived syntax for
`CONTEXT REMOTE <piece designator> FOR MODULE SEIZE ALL; END;`.

### Semantics (§10.2, §10.10.2)

- A context is a **quasi reach**: it has no runtime existence and no locations.
- The reach of a context is directly enclosed in the context in front of it, if any;
  otherwise it has no directly enclosing group.
- A context list describes the surrounding reaches of a piece of a program that is
  developed piecewisely. Its quasi declarations state the static properties of names
  that the enclosed module/region seizes from those surrounding reaches.
- A context with no directly enclosing group may not contain visibility statements.
- Virtual grants: for each name string `OP ! NS` visible in a spec module/region and
  linked to a quasi defining occurrence, and granted into a real reach as `NP ! NS`, a
  virtual grant with old name `OP ! NS` and new name `NP ! NS` is introduced in the
  corresponding module/region body.
- Contexts are redundant in a complete program; their purpose is separate compilation.
  `REMOTE` pieces are the mechanism that supplies the missing text.

## Implemented subset

`z200c` now supports a context list before a **top-level** module or spec module:

```chill
CONTEXT
  DCL x INT;
  F: PROC (a INT) RETURNS (INT) END;
FOR
Client: MODULE
  SEIZE x, F;
  ASSERT F(2) = 3;
END Client;
```

- Quasi declarations are `DCL` (without initializers) and procedure signatures.
- Each quasi variable/procedure is checked against the grants that enter the
  imaginary outermost reach from modules in the same program:
  - the name must be granted exactly once and be accessible to the module (including
    `TO` restrictions),
  - the declared mode or procedure signature must match the grant,
  - unprefixed grants only (a context simple name denotes an unprefixed name string).
- Contexts have no runtime effect; they are static assertions about the enclosing reach.
- Multiple context lists are parsed in order, but visibility statements (`GRANT`/`SEIZE`)
  in a context are rejected for now. The standard forbids them in a context with no
  directly enclosing group; the rest needs nested context reaches.
- Mode definitions in contexts are rejected for now; declarations may use the built-in
  scalar modes.
- Contexts work on top-level modules and on **nested body modules**.
- A nested module can `SEIZE` names declared in its enclosing reach (the parent module,
  a `BEGIN` block, or a procedure body). `SEIZE ALL` imports the whole visible reach.
- Contexts on a nested module are checked against the parent reach.
- Names supplied by external grant files are not covered yet.
- `REMOTE` pieces are not implemented.

## Nested-module subset

Z.200 modules are actions, so a module may appear in another module's action list:

```chill
Outer: MODULE
  DCL x INT INIT := 1;

  Helper: MODULE
    SEIZE x;
    x := x + 1;
  END Helper;

  ASSERT x = 2;
END Outer;
```

Implemented:

- nested body modules, named or unnamed, executed in action order;
- `SEIZE`/`SEIZE ALL` from the immediate enclosing reach, including variables,
  procedures, and modes;
- seized variables alias the parent location; parent procedures remain callable;
- `module_floor` scoping: parent names are hidden unless seized, and nested locals do
  not escape;
- `GRANT` of a nested module's own variable declarations into its parent reach;
- sibling modules can `SEIZE` those grants (visibility is checked independently of
  textual order; duplicate names are ambiguous at the `SEIZE`);
- the parent reach itself can use a child's grant directly, and a nested parent can use
  a grandchild's grant;
- contexts on nested modules, checked against the parent reach, including sibling
  grants;
- `EXIT <module name>` from inside the module.

Grants are preloaded from modules directly in a module body. Modules inside action
blocks (`BEGIN`, `IF`, `DO`) use that block's reach, but static preloading for those
block reaches is not implemented yet.

Not implemented yet:

- procedure or mode definitions inside a nested module (modes may be seized from the
  parent);
- **re-granting a child's grant from the parent** (the parent can use the name directly,
  but its own `GRANT` still resolves only against its local declarations);
- prefix clauses, `TO`, or `FORBID` in a nested `GRANT`;
- prefix clauses or qualified names in a nested `SEIZE`;
- Z.200 lifetime: nested module locations are recreated each time the module action is
  executed instead of living to the end of the closest surrounding block, so at runtime
  the grant must be registered before a sibling or parent uses it;
- nested spec modules and regions.

## Why the full feature needs more

Top-level modules now have a meaningful enclosing reach (the imaginary outermost
process), and nested body modules have their parent reach. The remaining gap is the
full reach/export model:

1. **Nested `GRANT` and sibling visibility.** A child's grants should enter the parent
   reach and be seizable by siblings; top-level grants should propagate outward only.
   The current export table is still flat for top-level modules.
2. **Piecewise/remote compilation.** A context supplies the enclosing reach from a
   separate compilation unit, with `REMOTE` replacing text.

## Remaining staged plan

### Stage 1 — nested modulions and reaches (partially done)

Done: `Action::Module`, parser/AST support, nested execution, parent-reach `SEIZE`,
`module_floor` hiding, nested contexts, nested `GRANT` of own declarations, sibling
grants entering the parent reach, and direct use of child grants in the parent reach.

Still to do: parent re-grant of child grants, block-reach preloading, nested
procedures/modes, nested spec modules/regions, runtime reach hoisting (Z.200 location
lifetime), and the remaining nested `GRANT` clauses.

### Stage 2 — finish quasi declarations

- Allow mode definitions (and structure modes) in contexts, with a context-local mode
  table.
- Allow context visibility statements once nested context reaches exist.
- Support contexts before nested modules/regions and spec regions.

### Stage 3 — checking contexts

- Resolve each quasi declaration against the context's enclosing reach:
  - the name must be visible there,
  - the declared mode/signature must match the reach's defining occurrence.
- Context visibility statements operate inside the context reach, following the normal
  `GRANT`/`SEIZE` rules.
- Enforce “no visibility statements in a context with no enclosing group”.
- Implement virtual grants for names granted from a spec/context reach into a real reach.

### Stage 4 — runtime

Contexts emit no code. A nested module's execution enters its reach; quasi declarations
do not allocate or initialize anything.

### Stage 5 — remote pieces

- Manifest/CLI mapping from piece designators to source files.
- `REMOTE <piece designator>` for modules/specs/program units and
  `CONTEXT REMOTE <piece designator> FOR`.
- Text replacement semantics, including context-body insertion after the last context
  body of the referenced context list.

## Test fixtures for the implementation

- A context declares a name granted by the parent reach; a nested module seizes it.
- A context declaration mismatching the parent grant produces a diagnostic.
- A context `GRANT` exports a nested module's name into its reach.
- A context with no enclosing group and a visibility statement fails.
- A remote context supplies a context list and is merged by the context-body insertion
  rule.

## Non-goals

- Contexts as runtime objects or storage.
- Accepting `CONTEXT` syntax without the reach model to give it meaning.
- Piecewise compilation without `REMOTE` designators and a file-mapping rule.
