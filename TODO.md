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
- [ ] Expand fixtures with more direct Z.200 examples and, if useful, full golden
      snapshots.

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

## 6. Region concurrency

- [ ] Design region mutual exclusion and intra-regional lifetimes.
- [ ] Implement the runtime semantics or explicitly keep regions single-threaded.
- [ ] Support nested regions inside module bodies.

## 7. Core language coverage

- [x] One-dimensional scalar arrays (`ARRAY (low:high) mode`) with indexing, bounds
      checks, and whole-array assignment.
- [ ] Array slices, nested/multidimensional arrays, arrays of structures, and array
      literals.
- [x] `CHAR` and unbounded `CHARS` modes (`CHARS(n)`, `VARYING`, and `WCHARS` remain).
- [ ] Exceptions and handlers.
- [ ] Processes, signals, and timers.
- [ ] Sets/powersets, moreta modes, templates.

## 8. Cleanup

- [ ] Replace message-based error classification with explicit codes.
- [ ] Reduce repeated temp-dir setup in CLI tests.
- [ ] Reconcile docs status matrices with the implementation.
