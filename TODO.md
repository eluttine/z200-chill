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
- [ ] Remaining: share the reach tree itself (declarations, modes, hoisting, virtual
      grants) between `program`, `sema`, and `interp`.

Acceptance for completion: no duplicate reach-matching or reach-building logic remains;
all existing tests pass with no new special cases.

## 2. Move virtual grants into analysis

- [ ] Replace the loader-side `apply_virtual_grants` mutation with a normalization step
      in program analysis.
- [ ] Ensure `run_source`/direct `check`/`grant` see the same virtual grants as the CLI
      loader path.
- [ ] Add a regression test that checks the same nested-spec program through every entry
      point.

## 3. Conformance and negative fixtures

- [ ] Build a fixture set from Z.200 examples for the implemented subset.
- [ ] Add negative fixtures for reach, context, nested-spec, region, and remote errors.
- [ ] Add CLI golden tests for grant-file output and diagnostics.

## 4. Decide product direction

- [ ] Choose interpreter-first educational compiler vs native/C backend.
- [ ] If backend: define declaration/procedure lowering, symbol mangling, and runtime
      ABI before writing code.
- [ ] If language-first: prioritize arrays, strings, exceptions/handlers, and I/O.

## 5. CI

- [ ] Add CI for `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
      `cargo test --all`.
- [ ] Pin/declare the minimum supported Rust version.
- [ ] Optionally run tests on Linux with a stable toolchain matrix.

## 6. Region concurrency

- [ ] Design region mutual exclusion and intra-regional lifetimes.
- [ ] Implement the runtime semantics or explicitly keep regions single-threaded.
- [ ] Support nested regions inside module bodies.

## 7. Core language coverage

- [ ] Arrays and array slices.
- [ ] Full character-string modes.
- [ ] Exceptions and handlers.
- [ ] Processes, signals, and timers.
- [ ] Sets/powersets, moreta modes, templates.

## 8. Cleanup

- [ ] Replace message-based error classification with explicit codes.
- [ ] Reduce repeated temp-dir setup in CLI tests.
- [ ] Reconcile docs status matrices with the implementation.
