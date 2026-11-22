# TODO

Prioritized work after the reach/context/piecewise work. Items are ordered; each should
land as a small, green change.

## 1. Unify the reach model

- [x] Introduce a shared export/reach representation (`src/reach.rs`) used by `sema`
      and `interp` (selection rules, not yet the full reach tree).
- [x] Move prefix matching, `TO` friend checks, and ambiguity rules into shared helpers.
- [x] Replace the duplicated `ReachExport` types in `sema.rs` and `interp.rs` with the
      shared metadata plus checker/interpreter-specific payloads.
- [x] Keep one `SEIZE`/`SEIZE ALL` selection path so checker and interpreter cannot
      diverge.
- [x] Add focused tests for prefix/friend/ambiguity cases.
- [x] Shared static `ReachTree` for module declarations and modes, consumed by
      `program`, `sema`, and `interp`.
- [x] Shared child/hoisting plan (`children`, `hoisted`), used by the interpreter's
      module-body hoisting.
- [x] Shared module-reach exports (`exports` with owner, kind, prefix/friend metadata),
      used to seed checker and interpreter module scopes.
- [x] Top-level `GRANT` child-export collection now uses the tree; removed the separate
      `collect_child_grants`/`collect_module_exports` path.
- [x] Block-level reach exports (`BEGIN`/`IF`/`DO`/`CASE`) are tree nodes keyed by
      block/branch/alternative spans and seed both checker and interpreter scopes.
- [x] Nested `GRANT` validation (duplicate/unknown names, unsupported old prefixes) and
      `FORBID` application moved into reach-tree construction; removed the checker's
      `collect_module_grants` and the interpreter's runtime grant registration.
- [x] Interpreter hoisting uses one tree-driven path for module bodies and block reaches;
      removed IR statement scanning for module declarations.

Acceptance for completion: no duplicate reach-matching or reach-building logic remains;
all existing tests pass with no new special cases.

## 2. Move virtual grants into analysis

- [x] Replace the loader-side `apply_virtual_grants` mutation with a normalization step
      (`program::normalize`) used by semantic entry points.
- [x] Ensure `run_source`/direct `check`/`grant` see the same virtual grants as the CLI
      loader path.
- [x] Add regression tests for direct entry points and normalization idempotence.
- [x] Remove the loader-side mutation.

## 3. Conformance and negative fixtures

- [x] Build a starter fixture set for the implemented subset (units, contexts,
      nested specs, remote syntax, nested/block reaches, spec regions, virtual grants).
- [x] Add negative fixtures for region actions, first-context visibility, unnamed
      nested specs, nested grant ambiguity, nested `FORBID`, `TO` restrictions, and
      nested/region interface mismatches.
- [x] Add CLI structural assertions for grant-file output (`schema = 4`, no
      `int_widths`/`edition`) and existing diagnostic-code assertions.
- [x] Reject `SET` and module-mode types at grant-interface boundaries instead of
      emitting unloadable grant files.
- [x] Expand fixtures with direct Z.200 examples (expressions, control flow,
      procedures, modes/structures, visibility, arrays, handlers) and a `run_fail`
      suite for runtime checks (`ASSERTFAIL`, `OVERFLOW`, array bounds, undefined
      values, `TEXTFAIL`, unhandled exceptions). Full stdout snapshots were not added;
      the interpreter writes directly to stdout, and the `ASSERT`-based fixtures already
      assert observable behavior.
- [x] Extend regression coverage for newer features: generic actual-kind mismatch,
      powerset member mismatch, float division by zero, negative variable widths, and
      a golden stdout snapshot for `WRITE`/`WRITELINE` through the CLI.

## 4. Decide product direction (resolved: interpreter-first)

- [x] Choose interpreter-first educational compiler vs native/C backend.
      Decision: interpreter-first educational compiler.
- [-] Native/C backend: deferred; no declaration lowering, symbol mangling, or ABI work
      for now.
- [x] Direction: prioritize language coverage/conformance; start with strings, then
      arrays, exceptions/handlers, and I/O.

## 5. CI

- [x] CI workflow runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
      and `cargo test --all` on stable Linux.
- [x] `rust-toolchain.toml` pins the stable channel with rustfmt/clippy.
- [ ] Decide whether to declare and test an MSRV; current policy is stable-only.
- [ ] Optionally add a stable/nightly matrix once there is a reason.

## 6. Concurrency (post-first-release)

First release policy: **language coverage, not faithful concurrency**. Regions and
processes are single-threaded/non-preemptive for now; faithful CHILL concurrency is
deferred.

- [x] Decide first-release policy: keep execution single-threaded.
- [ ] Design region mutual exclusion and intra-regional lifetimes.
- [ ] Implement a resumable scheduler/VM for processes, signals, and timers.
- [ ] Support nested regions inside module bodies.

## 7. Core language coverage

- [x] One-dimensional scalar arrays (`ARRAY (low:high) mode`) with indexing, bounds
      checks, and whole-array assignment.
- [x] Arrays of structures, including field selection and field assignment through
      array elements.
- [x] Unlabelled array literals/tuples (`(: v, ... :)` and `[v, ...]`) in contexts
      with a known array mode.
- [x] Array slices (`a(lower:upper)` and `a(first UP size)`), usable as array values
      and assignment targets, with constant or runtime bounds.
- [x] Multidimensional arrays up to four dimensions, with multi-index access,
      whole-array assignment/equality, and scalar or structure elements.
- [x] Labelled array tuples with literal/range/`ELSE`/`(*)` labels, completeness and
      overlap checking.
- [x] `CHAR`, unbounded `CHARS`, fixed `CHARS(n)`, and bounded
      `CHARS(n) VARYING` modes (`WCHARS` remains).
- [x] `CAUSE` and module/procedure handlers, including catching `ASSERTFAIL`/`OVERFLOW`.
- [x] Action-level handlers, including handlers on bracketed actions, assignment,
      assert, and call actions.
- [x] Exception-set flow checking for `CAUSE`, `ASSERT`, and called procedures;
      `EXCEPTIONS` lists are enforced and compared across spec/body signatures.
      Implicit runtime checks such as `OVERFLOW`, range, and text failures are not
      statically inferred yet.
- [x] Implementation-defined text I/O (`WRITE`, `WRITELINE`, `READLINE`).
- [x] `READTEXT`/`WRITETEXT` for `CHARS` locations with literal format text.
- [x] Bounded text modes (`CHARS(n)` and `CHARS(n) VARYING`), including arrays of
      bounded strings.
- [x] `READTEXT`/`WRITETEXT` format conversion and editing codes with io lists
      (`%B`, `%O`, `%H`, `%C`, widths, `L`/`E`/`P` qualifiers, repetition, `X`, `>`,
      `<`, `T`, and `%%`).
- [x] Basic `ASSOCIATION` and `ACCESS CHARS` modes with `ASSOCIATE`, `DISSOCIATE`,
      `ISASSOCIATED`, `CREATE`, `DELETE`, `CONNECT`, `DISCONNECT`, `READRECORD`, and
      `WRITERECORD` over sequential line records.
- [x] Parenthesized text-format clauses with repetition.
- [x] Variable format widths (`V`) for conversions and editing clauses.
- [x] Association attributes `EXISTING`, `READABLE`, `WRITEABLE`, `INDEXABLE`,
      `SEQUENCIBLE`, and `VARIABLE`.
- [x] Floating-point mode `FLOAT`, literals, arithmetic/comparisons, `FLOAT(...)`
      conversion, float arrays, fractional/exponent format widths, and `%F`.
- [x] I/O control format clauses (`%/`, `%+`, `%-`, `%?`, `%!`, `%=`).
- [x] Association `MODIFY` (no parameters) and indexed `ACCESS (range) CHARS`,
      indexed `CONNECT`/`READRECORD`/`WRITERECORD`, record-range checks, and
      `INDEXABLE` reporting.
- [-] Processes, signals, and timers: deferred to post-first-release concurrency work.
- [x] `POWERSET` of `INT` / `BOOL` / `CHAR` with literals, ranges, union,
      intersection, difference, equality/ordering, and `IN`.
- [x] Named `SET (...)` modes with unnumbered and numbered elements.
- [x] Generic procedure templates parameterized by `MODE`, with
      `name: PROC = NEW template (modes) END;` instantiation.
- [x] Generic module templates parameterized by `MODE`, with
      `name: MODULE = NEW template (modes) END;` instantiation.
- [x] Moreta module-body objects with variable/procedure components, value-returning
      methods, non-value method actions, component `INIT` evaluation (including base
      initializers), `BASED_ON` inheritance/override, paired `MODULE SPEC`
      interfaces, and invariants.
- [x] Consolidate the moreta subset: document semantics/limits, reject object
      assignment, reject abstract-mode instantiation, check invariants after
      initialization, merge spec/body invariants, and key moreta metadata by stable
      mode ID.
- [x] Generic `SYN` value parameters in procedure and module templates.
- [ ] Moreta constructors/destructors, `SELF`, task/region object modes; generic
      `PROC` parameters and region/process templates.
- [ ] Faithful processes/signals/timers and region mutual exclusion: deferred pending
      a resumable/concurrent runtime (post-first-release).

## 8. Cleanup

- [x] Replace message-based error classification with explicit codes at checker and
      interpreter error-construction sites.
- [x] Reduce repeated temp-dir setup in CLI tests with a `TempDir` fixture that
      creates sources and cleans up on drop.
- [x] Reconcile docs status matrices with the implementation (subset scope,
      grant/contexts/piecewise notes, text I/O and exception decisions).
