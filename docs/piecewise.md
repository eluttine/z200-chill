# Piecewise compilation (design)

Status: **Phases 0–4 implemented for the current region subset.** Remote syntax, piece
designators, manifest `[pieces]`, the resolver, expansion of remote modules/spec
modules/program units, remote contexts, `CONTEXT MODULE REMOTE` desugaring, nested spec
modules (including inside contexts and recursively), virtual grants, regions, and spec
regions exist. Region concurrency/mutual exclusion and nested regions are not
implemented.

## Normative model

References are to ITU-T Z.200 (11/99), identical to ISO/IEC 9496.

### Program units and pieces (§10.8, §10.10)

A complete program is a list of program units — modules, spec modules, regions, spec
regions, moreta declarations, and templates — surrounded by the imaginary outermost
process. Modules and regions are the elementary **pieces** of a piecewise-developed
program. The text of a piece is indicated by remote constructs.

### Remote constructs (§10.10.1)

```text
<remote modulion> ::= [ <simple name string> : ] REMOTE <piece designator> ;
<remote spec>     ::= [ <simple name string> : ] SPEC REMOTE <piece designator> ;
<remote context>  ::= CONTEXT REMOTE <piece designator>
                      [ <context body> ] FOR
<context module>  ::= CONTEXT MODULE REMOTE <piece designator> ;
<remote program unit> ::= [ <simple name string> : ] REMOTE <piece designator> ;
```

`CONTEXT MODULE REMOTE <piece designator> ;` is derived syntax for

```text
CONTEXT REMOTE <piece designator> FOR
  MODULE SEIZE ALL; END;
```

### Piece designators (§10.10.1)

```text
<piece designator> ::= <character string literal> | <text reference name> | <empty>
```

The mapping from a designator to source text is **implementation defined**:

- an empty designator retrieves text from a place determined by the program structure;
- a character string literal is used to retrieve the text;
- a text reference name is interpreted implementation-definedly.

This is the same kind of freedom as the grant-file format: the artifact and lookup rule
are ours, while the replacement semantics are normative.

### Replacement semantics

- A program with a remote modulion/spec/program unit is equivalent to the program with
  that construct replaced by the referenced piece of CHILL text.
- A program with remote contexts is equivalent to the program with each remote context
  replaced by the referenced context list, with the local context body virtually
  inserted immediately after the last context body of that list.
- `REMOTE` does not imply textual substitution; it only defines equivalence.

### Static conditions

- The designator of a remote modulion must refer to a module or region that is not
  itself a remote modulion.
- A remote spec must refer to a spec module or spec region that is not a remote spec.
- A remote context or context module must refer to a context list that is not a remote
  context.
- A remote program unit must refer to a program unit that is not remote.
- A context that has no directly enclosing group may not contain visibility statements.
- A spec module/region must have a corresponding body with the same simple name, and the
  body must match the spec's variables and procedure signatures.

### Spec modules, spec regions, and contexts (§10.10.2)

- Spec modules/regions are quasi reaches. They may contain quasi declarations,
  visibility statements, nested spec modules, and spec regions; they contain no actions.
- Contexts are quasi reaches describing the **surrounding reaches** of a piece.
- A module or region is developed piecewisely iff it has a context list.
- Virtual grants: for each name `OP ! NS` visible in a spec/context reach, linked to a
  quasi defining occurrence, and granted into a real reach as `NP ! NS`, a virtual grant
  with the same old/new names is introduced in the corresponding body.

## Current implementation

Implemented today:

- flat top-level modules and spec modules;
- nested body modules and block reaches;
- `GRANT`/`SEIZE`, prefixes, `TO`, `FORBID`, and re-grants through nested reaches;
- top-level `CONTEXT ... FOR` lists with quasi declarations and limited visibility;
- grant files and package dependencies.

Missing:

- regions and spec regions (regions do not exist in the compiler at all);
- remote constructs and piece designators;
- nested spec modules (inside contexts/spec modules);
- virtual grants from spec/context reaches;
- a manifest rule for piece lookup.

## Proposed architecture

### 1. Program-unit tree

Replace the flat top-level `Program.modules` with a unit tree while keeping the existing
`Module` type as the body representation:

```rust
pub struct Program {
    pub units: Vec<ProgramUnit>,
}

pub struct ProgramUnit {
    pub kind: UnitKind,          // Module | SpecModule | Region | SpecRegion
    pub name: Option<String>,
    pub contexts: Vec<Context>,
    pub module: Option<Module>,
    pub region: Option<Region>,
    pub origin: PieceOrigin,     // file/piece provenance for diagnostics
}

pub struct PieceOrigin {
    pub source: SourceId,
    pub piece: Option<String>,
}
```

Remote units can be represented directly in the AST (`UnitBody::Remote(designator)`) or
resolved before building `Program`. Prefer a separate **expansion pass** that turns a
syntax program with remotes into a complete syntax program, so the existing checker and
interpreter never see remote nodes.

### 2. Piece designators and resolution

Add:

```rust
pub enum PieceDesignator {
    Empty,
    Literal(String),
    TextRef(String),
}
```

Resolution is a `PieceResolver` owned by the loader:

- **Literal**: a path relative to the containing package directory. Reject paths that
  escape the package root unless an explicit `allow_absolute_pieces` setting exists.
- **TextRef**: looked up in the package manifest.
- **Empty**: implementation-defined convention. Proposal: the unit's simple name plus
  `.ch`, looked up in the package's `pieces/` directory; if the unit is unnamed, error.
  This keeps behavior deterministic and file-oriented.
- Canonicalize paths, track `SourceId`s, and detect cycles by canonical path.

Manifest extension (non-normative artifact):

```toml
[pieces]
counter_body = "pieces/counter.ch"
```

The key is a text reference name; the value is a package-relative path.

### 3. Expansion pass

Order:

1. Parse each source file into a **syntax program** containing modules, specs, regions,
   contexts, and remote nodes.
2. Resolve remote units recursively. For a remote modulion/spec/program unit, parse the
   referenced file and substitute the resulting unit. For a remote context, parse the
   referenced context list and append the local context body after the last context body.
3. Desugar `CONTEXT MODULE REMOTE` before or during expansion.
4. Verify kind compatibility and expansion-kind restrictions.
5. Run the existing semantic analysis on the expanded program.

Eager expansion is preferred: it gives deterministic diagnostics and keeps one AST shape
for the checker, interpreter, and grant emitter. Provenance is retained on each expanded
unit so diagnostics point at the piece file, not only the referencing file.

### 4. Spec modules and virtual grants

- Keep spec/body pairing by package + simple name (already used for top-level specs).
- Extend pairing to nested specs once regions/contexts make them reachable.
- After expansion, compute quasi reaches for spec modules/contexts and synthesize
  virtual grants before the normal `GRANT`/`SEIZE` analysis:
  - names declared in a spec reach and granted into a real reach produce grants in the
    corresponding body;
  - the old/new prefix pair is preserved, so prefix-aware reach exports are reused.
- Context lists on piecewise modules are checked against the expanded program's
  enclosing reach. In a complete program, the declarations must match real reach names;
  in a piecewise build they may also be satisfied by another piece's spec/context.

### 5. Regions

Regions are a prerequisite for remote regions and spec regions, and are a larger
feature (concurrent access, intra-regional lifetimes). This design reserves the AST and
unit-kind slots but does not define region execution.

Proposed region milestone:

1. Parse `REGION ... END` and `SPEC REGION`/`REGION SPEC` with the same body restrictions
   as modules.
2. Reuse module reach/visibility rules for regions.
3. Add the minimal runtime semantics needed by tests: a region is a modulion with its own
   reach; concurrency/locking is a later step.
4. Then allow remote modulions to refer to regions and add spec-region pairing.

### 6. Diagnostics

Add a piecewise/remote code range, e.g. `E07xx`:

- missing piece / unreadable file;
- unknown text reference;
- designator refers to the wrong unit kind;
- remote cycle;
- context-body insertion failure;
- nested spec without a body / body mismatch;
- region used before region support.

Diagnostics should carry both the referencing span and the resolved piece path.

## Phased implementation

### Phase 0 — Manifest and resolver (implemented)

- `[pieces]` text-reference table in `Chill.toml`.
- AST `PieceDesignator`/`RemoteUnit`; parse `REMOTE`, `SPEC REMOTE`, `CONTEXT REMOTE`,
  `CONTEXT MODULE REMOTE`.
- `src/pieces.rs` resolves literal, text-reference, and empty designators; rejects
  absolute/escaping paths; detects cycles via `resolve_checked`.
- Loader validates that referenced pieces exist (`E0700`).
- No expansion yet; remote nodes produce `E0701` “remote pieces are not expanded yet”.

### Phase 1 — Remote modules, specs, and program units (implemented)

- The loader expands remote modulions/specs/program units before semantic analysis.
- Kind compatibility is enforced (module vs spec module vs any unit).
- Optional names must match the referenced unit’s module name.
- A referenced piece must contain exactly one non-remote program unit.
- Cycle detection uses the resolver’s `visiting` set.
- Expanded modules get the referencing package identity.
- Tests: literal/text-ref/empty resolution, missing piece, wrong kind, named remote
  module, remote spec module, and execution of expanded modules.
- Remote contexts and `CONTEXT MODULE REMOTE` still report `E0701`.

### Phase 2 — Remote contexts and context lists (implemented)

- `parser::parse_context_piece` parses a standalone context-list piece.
- `CONTEXT REMOTE ... FOR` resolves, parses, and recursively expands the referenced
  context list, then appends the local context body to the last context in that list.
- Parser first-context visibility rejection is deferred for remote contexts; the
  post-merge checker enforces the final context positions.
- `CONTEXT MODULE REMOTE` desugars to the referenced context list plus an anonymous
  `MODULE SEIZE ALL; END;` unit.
- Context pieces must be context lists, not modules or remote units.
- Tests: body insertion into a later context, context-module desugaring, wrong-kind
  context piece, and execution with merged contexts.

### Phase 3 — Nested spec modules and virtual grants (partial)

Implemented:

- `Declaration::SpecModule` parses `SPEC MODULE`/`MODULE SPEC` inside spec module
  bodies and context bodies, including a context list.
- Nested specs are collected from a module’s declarations **and its contexts**, then
  paired by name with directly nested body modules.
- Interfaces are checked with the normal spec/body rules, recursively through the nested
  spec/body tree.
- `program::normalize` applies virtual grants (nested-spec `GRANT`/`SEIZE` copied into
  the matching body) before semantic analysis, recursively, so they export into the
  parent reach and can be re-granted upward. The checker, interpreter, and grant
  emitter all normalize, so behavior does not depend on the loader path.
- Nested bodies with their own visibility statements are rejected when a nested spec
  exists.
- Tests: nested spec parsing, context nested specs, recursive spec/body trees,
  virtual-grant execution, and nested interface mismatch.

Still to do: spec regions (after regions exist).

### Phase 4 — Regions and spec regions (current subset implemented)

- `REGION`, `REGION BODY`, `REGION SPEC`, and `SPEC REGION` parse as top-level
  program units.
- Region bodies may contain declarations and visibility statements but no actions.
- Regions reuse the module reach/visibility/export rules and the interpreter’s module
  scope; a region is entered once and its declarations are initialized.
- Spec regions pair with regions by package + name, with interface checking and
  spec-side grants (virtual grants).
- Remote modulion designators accept modules or regions; remote spec designators accept
  spec modules or spec regions.
- Tests: region parsing/action rejection, spec-region pairing and execution, remote
  region expansion.

Still to do: region concurrency/mutual exclusion, nested regions inside module bodies,
and spec-region nesting inside spec regions.

### Phase 5 — Cross-file provenance and grant files

- Ensure source maps and diagnostics span referenced pieces.
- Decide whether grant files record piece provenance (proposal: no; they record the
  expanded interface, with `package_dir` as today).

## Test fixtures to add

- `pieces/remote_module.ch` referenced by literal path and by text reference.
- Remote spec module paired with a body.
- Remote context merged with a local context body.
- `CONTEXT MODULE REMOTE` expansion.
- Missing/cyclic/wrong-kind pieces.
- Nested spec module with a virtual grant.
- Region/spec-region fixtures once regions exist.

## Non-goals for this design

- Defining region concurrency or storage semantics beyond what tests need.
- Changing the grant-file schema for piece provenance.
- Cross-package piece references (pieces are package-local).
- Textual substitution semantics or macro-like expansion; only Z.200-equivalent
  replacement.
