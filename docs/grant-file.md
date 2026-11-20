# Grant Files and SPEC Modules

This document covers `GRANT` / `SEIZE`, `SPEC MODULE` pairing, and the compiled
interface artifact. See [subset.md](subset.md) for the language rules and
[manifest.md](manifest.md) for dependency configuration.

## GRANT/SEIZE model (v0, single entry file)

The program units in one entry file are treated as siblings in the imaginary outermost
process. Visibility is static and independent of textual order:

- `GRANT a, b;` may only name variables or procedures **declared in the same module**.
  Granted names become candidates for `SEIZE` by other modules.
- `SEIZE a, b;` imports each name from the unique other module that granted it.
  Seizing an ungranted name is an error; a name granted by more than one module is an
  error (v0 has no prefixes to disambiguate).
- Names are private unless granted and seized. A module cannot see another module's
  private names.
- A seized variable aliases the granting module's location, so assignments are shared.
- A seized procedure executes in its **defining** module's scope; it does not see the
  caller's module or local names.
- A module cannot seize a name it grants itself.
- `GRANT ALL`, `FORBID`, `TO`, prefixes, contexts, and remote pieces are not
  implemented yet.

## SPEC modules

A spec module declares the static interface of a module body:

```text
name: SPEC MODULE
  ... declarations and procedure signatures ...
  GRANT ... ;
END name;

name: MODULE SPEC          -- alternative spelling in Z.200 (11/99)
  ...
END name;
```

Implemented v0 rules:

- A spec must be named and must have a body module with the same name. Module names
  must be unique within their kind, so one spec and one body may share a name.
- A spec body may contain `DCL` declarations, procedure signatures
  (`P: PROC (...) [RETURNS (...)] END;`), `GRANT`, and `SEIZE`.
- Spec variables must not have initializers, and spec modules cannot contain actions.
- The body must declare every spec variable with the same mode and every spec procedure
  with a matching parameter/result signature. Extra private declarations are allowed.
- If a spec exists, `GRANT` must appear in the spec rather than the body. The spec's
  grants are the module's public interface.
- At runtime, spec grants resolve to the paired body's variables and procedures.

`z200c grant` writes the compiled interface to
`target/z200c/<package>.grt.toml`:

```toml
schema = 1
package = "spec"
edition = "z200-11/99"
int_widths = "16-32-64"

[[exports]]
name = "Bump"
kind = "procedure"

[[exports]]
name = "value"
kind = "variable"
mode = "INT"
```

A package can consume another package's interface through `[dependencies]`:

```toml
[dependencies]
lib = "../lib/target/z200c/lib.grt.toml"
```

- Paths are resolved relative to the depending package's directory.
- The dependency's `edition` and `int_widths` must match the consuming package.
- Names exported by both the program and a dependency are an error.
- `z200c check` and `z200c grant` resolve `SEIZE` against these external exports.
- `z200c run` does not link external dependencies yet; it rejects a run only when an
  external name is actually seized.

## Current limits

- Grant-file consumption is static: `check` and `grant` use dependency interfaces, but
  `run` has no linker for external code.
- Prefixes (`module!name`), `GRANT ALL`, `FORBID`, `TO`, contexts, and remote pieces are
  not implemented.
- Symbol mangling and a linked multi-package build are still open.
