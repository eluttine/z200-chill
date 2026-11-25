# TODO

This file tracks remaining work and the boundary between the portable C backend and the
future LLVM backend. Keep items small, testable, and ordered by risk.

## Status snapshot

- The interpreter is the reference implementation for the supported subset.
- `z200c compile` provides a portable C11 backend for a broad sequential/static subset.
- Native tests cover language structure, runtime failures, text/files, grants, and
  reference-count cleanup.
- Native ownership is reference-counted and unwind-safe for normal scopes, loops,
  labelled exits, and caught exceptions.
- Full native concurrency, generics, moreta, and optimized code generation are not C
  backend goals.
- The current platform target is Linux only. Windows and macOS support are out of scope
  until explicitly revisited.

Verification gate before any commit:

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all
```

## 1. Finish the C backend's meaningful language scope

These items are worth doing in the C backend because they complete the sequential/static
subset and exercise the same typed information the LLVM backend will need.

- [ ] Complete text sub-location references:
  - `GETTEXTACCESS`, `SETTEXTACCESS`,
  - `GETTEXTRECORD`, `SETTEXTRECORD`,
  - `GETASSOCIATION` reference results,
  - dereferenced text/access references as transfer locations,
  - aliasing between text record cells and bounded varying string locations.
- [ ] Add dynamic multidimensional slice bounds.
  Start in the checker/interpreter type model; the C backend should follow the shared
  representation rather than invent a native-only shape.
- [ ] Add wide text/bit-string execution only if the C backend must pass broad
  interpreter fixtures. Otherwise leave `WCHAR`/`WTEXT` and `BOOLS` execution to LLVM.
- [ ] Native module linking for grant-file-only dependencies without source metadata.
- [ ] Extract the generated runtime into `runtime/`:
  - refcounted payload allocation,
  - string/powerset/text operations,
  - cleanup stack and exception frames,
  - association/access file records,
  - text formatting helpers.
  This is the shared runtime ABI seed for LLVM.

## 2. C backend stop line

The C backend should **not** grow into the following. These belong to the LLVM/full-native
track:

- processes, signals, timers, buffers, regions, tasks, and schedulers,
- task/region moreta object execution,
- moreta lifecycle, inheritance, and component dispatch,
- generic procedure/module/region/process lowering,
- preemptive or coroutine-based concurrency,
- sophisticated optimization, debug info, and source maps,
- full cross-package ABI and piecewise linking,
- a second full implementation of dynamic-shape or sub-location semantics just for C.

The C backend remains a bootstrap/fallback target for the sequential/static subset. The
interpreter remains authoritative for semantics not implemented natively.

## 3. LLVM handoff criteria

Start the LLVM backend when these are true, rather than waiting for every C feature:

1. **Typed IR is the lowering input**
   - Sequential declarations, expressions, calls, aggregates, text, references,
     exceptions, and module symbols are represented in typed IR.
   - The LLVM backend does not duplicate the C backend's AST-walking logic.

2. **Runtime ABI is defined**
   - Value layout for scalars, strings, text records, powersets, structures, arrays,
     references, associations, and access locations.
   - Parameter passing (`IN`, `OUT`, `INOUT`, `LOC`) and array/result wrappers.
   - Refcount retain/release/copy/drop and unwind cleanup entry points.
   - Exception raise/handler ABI and handler cleanup markers.
   - Module symbol/visibility conventions for `GRANT`/`SEIZE`.

3. **C backend scope is frozen**
   - Text sub-locations and dynamic slices are either implemented or explicitly
     declared unsupported in both native backends.
   - No active semantic changes are pending only in the C code generator.

4. **Conformance gate**
   - All `tests/run_ok` fixtures that do not require deferred LLVM-only features pass
     natively.
   - Native `run_fail` behavior is stable, including `live-payloads: 0` under
     `Z200C_RC_STATS=1`.

5. **Diagnostics are stable**
   - Unsupported LLVM-first features are rejected with stable codes and clear messages.
   - The interpreter and native backends agree on mode errors and runtime checks.

## 4. LLVM roadmap after handoff

- [ ] LLVM target skeleton and C ABI shim.
- [ ] Scalar modes, variables, expressions, calls, `IF`/`DO`/`CASE`.
- [ ] Arrays, slices, structures, strings, powersets, references, text.
- [ ] Module linking, grants, and package ABI.
- [ ] Exception frames and runtime range/overflow checks.
- [ ] Moreta object layout, constructors/destructors, methods, and visibility.
- [ ] Generic instantiation/lowering.
- [ ] Cooperative processes, signals, buffers, timers, regions, and tasks.
- [ ] Optimization, debug info, and source maps.

## 5. Interpreter and conformance backlog

The interpreter remains the reference while native work proceeds.

- [ ] Full coroutine-based main-process termination semantics.
- [ ] Arrays of references, nested references, and remaining reference-to-sub-location
      cases that are still deferred.
- [ ] Moreta task/region object modes and generic region/process templates.
- [ ] Keep `tests/run_ok`, `tests/run_fail`, and `tests/sema_fail` growing with every
      language change.
- [ ] Add native fixture mirrors for new sequential features where practical.

## 6. Tooling and documentation

- [ ] Update `docs/decisions.md` §10.9: code generation is no longer future work; record
      the C bootstrap scope and LLVM handoff policy.
- [ ] Keep `docs/backend.md` as the current C-backend contract and limitation list.
- [ ] Document the runtime payload/cleanup/exception ABI before LLVM lowering starts.
- [ ] Decide and test an MSRV, or document stable-only policy.
- [ ] Add a stable/nightly CI matrix when there is a concrete reason.
- [ ] Add native runtime allocation/ownership stress tests to CI.

## 7. Completed milestones (summary)

The following areas are implemented and no longer need backlog entries:

- shared reach/grant model and virtual-grant normalization,
- conformance fixtures and diagnostic-code assertions,
- cooperative interpreter concurrency, moreta objects, generics, associations, and text,
- C backend sequential/static coverage:
  - variables, control flow, procedures and parameters,
  - arrays, multidimensional arrays, slices, structures,
  - strings, floats, powersets, references,
  - text formatting, text files, associations, and access records,
  - exceptions, runtime checks, and integer width enforcement,
  - cross-module grants and seized modes,
  - reference-counted string/powerset/text payloads,
  - unwind-safe cleanup for scopes, loops, labelled exits, and caught exceptions,
  - configurable C compiler selection and CRLF-safe `READLINE`.
