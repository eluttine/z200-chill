# Grant Files and SPEC Modules

This document covers `GRANT` / `SEIZE`, `SPEC MODULE` pairing, and the compiled
interface artifact. See [subset.md](subset.md) for the language rules and
[manifest.md](manifest.md) for dependency configuration.

## GRANT/SEIZE model (v0)

Visibility is static and independent of textual order:

- `GRANT a, b;` may only name variables or procedures **declared in the same module**.
  Granted names become candidates for `SEIZE` by other modules.
- Every granted name is addressable as:
  - `name` (simple),
  - `Module!name`,
  - `package!Module!name`.
- `SEIZE name;` imports the unique visible export with that simple name. If more than one
  module exports it, the seize is ambiguous and must be qualified.
- `SEIZE A!x;` / `SEIZE pkg!A!x;` select a module/package-qualified export.
- `GRANT ALL;` grants every variable/procedure declared in the module.
- `SEIZE ALL;` imports every visible export with a unique simple name; a duplicate simple
  name makes the whole `SEIZE ALL` an error.
- `GRANT name TO Module1, package!Module2;` restricts who may seize the grant. Friend
  names are module names, optionally package-qualified.
- Names are private unless granted and seized. A module cannot see another module's
  private names.
- A seized variable aliases the granting module's location, so assignments are shared.
- A seized procedure executes in its **defining** module's scope; it does not see the
  caller's module or local names.
- A module cannot seize a name it grants itself.
- `GRANT` prefix renaming, `PREFIXED`, `FORBID`, contexts, and remote pieces are not
  implemented yet.

`z200c grant` emits simple names only, so a package whose modules export duplicate
simple names cannot produce a grant file (the emitter reports the duplicate).

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
package_dir = "/absolute/path/to/spec"

[[exports]]
name = "Bump"
kind = "procedure"

[[exports]]
name = "value"
kind = "variable"
mode = "INT"
```

`package_dir` is optional. `z200c grant` records the source package directory so `run`
can link the dependency when the source is available; interface-only grant files omit it
or are used only by `check`/`grant`.

A package can consume another package's interface through `[dependencies]`:

```toml
[dependencies]
lib = "../lib/target/z200c/lib.grt.toml"
```

- Paths are resolved relative to the depending package's directory.
- A grant-file dependency contributes its interface for `check`/`grant`. If the grant
  file records `package_dir`, `run` links that source package; otherwise it is
  interface-only.
- Directory dependencies are loaded recursively, their modules run first, and their
  exports are available to `SEIZE` without grant files.
- The dependency's `edition` and `int_widths` must match the consuming package.
- Exported names must be unique across the program and its dependencies (prefix
  qualification is not implemented yet).
- `z200c check` and `z200c grant` resolve `SEIZE` against both forms.
- `z200c run` links directory dependencies and grant files with `package_dir`; it
  rejects a run only when an interface-only external name is actually seized.

## Current limits

- Interface-only grant files are static: `check` and `grant` use their interfaces, but
  `run` cannot link them without a source package.
- Exported names are globally simple names; prefixes (`module!name`) and export
  namespacing are not implemented.
- `GRANT ALL`, `FORBID`, `TO`, contexts, and remote pieces are not implemented.
