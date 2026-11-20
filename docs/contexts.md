# Contexts (design)

Status: **design only, not implemented.** Contexts depend on nested modulions, which
`z200c` does not support yet.

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

## Why this is blocked today

The current compiler has a flat module list (`Program.modules: Vec<Module>`). Every
module's enclosing reach is the imaginary outermost process, which contains only
predefined names. In that world a context can only describe predefined names, so there
is nothing meaningful to check or implement.

Contexts become useful only when one of these exists:

1. **Nested modulions.** Modules are actions in Z.200 (§10.6), so a module can sit
   inside another module's reach. A context list then describes the parent reach.
2. **Piecewise/remote compilation.** A context supplies the enclosing reach from a
   separate compilation unit, with `REMOTE` replacing text.

`z200c` currently supports neither. Implementing contexts first would mean inventing
reach semantics or accepting syntax with no effect, which this project avoids.

## Staged plan

### Stage 1 — nested modulions and reaches

- AST: allow modules/regions in action position (`Action::Module`).
- Analysis: replace the flat module list with a module tree or parent links.
- Visibility: `GRANT` exports into the directly enclosing reach; `SEIZE` searches that
  reach. Module identity remains package + module name for separate compilation, but a
  nested module's reach must be tracked separately from its name.
- Interpreter: executing a nested module enters/leaves its reach; declarations and
  actions behave like a top-level module.

This stage is the prerequisite for everything below.

### Stage 2 — context syntax and quasi declarations

- Parse `<context list>` before modules/regions/specs and represent it in the AST.
- Context bodies reuse declarations/visibility with a `quasi` flag:
  - no initializers,
  - no actions,
  - no runtime allocation,
  - procedure entries are signatures only.
- Keep contexts in syntax trees even before full checking so diagnostics are precise.

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
