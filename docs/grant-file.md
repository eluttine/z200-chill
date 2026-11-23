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
- Prefix clauses follow §12.2.3.3:
  - `GRANT x PREFIXED P;` exports `P!x`;
  - `GRANT ALL PREFIXED;` uses the granting module name as the prefix;
  - `GRANT (old -> new) ! x;` links the source `old!x` to the exported
    name `new!x`. An empty old prefix is the common form; a non-empty old prefix
    requires a prefixed source name in the granting module's reach, which is supported
    only for the documented nested-grant subset.
  - `SEIZE x PREFIXED P;` imports `P!x` as the local simple name `x`.
  - `SEIZE ALL PREFIXED P;` imports every visible `P!name` as `name`.
  - `SEIZE (old -> new) ! x;` follows the same link rule. A non-empty new prefix
    would create a locally prefixed name, which the subset does not support yet.
  Prefixed exports are not addressable by their bare simple name, and `SEIZE ALL`
  without a prefix does not import prefixed exports.
- `SEIZE name;` imports the unique visible export with that simple name. If more than one
  module exports it, the seize is ambiguous and must be qualified.
- `SEIZE A!x;` / `SEIZE pkg!A!x;` select a module/package-qualified export.
- `GRANT ALL;` grants every variable/procedure declared in the module.
- `SEIZE ALL;` imports every visible export with a unique simple name; a duplicate simple
  name makes the whole `SEIZE ALL` an error.
- `GRANT name TO Module1, package!Module2;` restricts who may seize the grant. Friend
  names are module names, optionally package-qualified.
- Structure, set, and module modes can be granted and seized:
  ```chill
  NEWMODE S = STRUCT(x INT, hidden INT);
  GRANT S FORBID (hidden);
  ```
  `FORBID (a, b)` / `FORBID ALL` allows field selection only inside the defining
  module. `SEIZE S;` makes the mode name usable in the seizing module. Set modes keep
  their element names and representation values across the grant boundary.
- Names are private unless granted and seized. A module cannot see another module's
  private names.
- A seized variable aliases the granting module's location, so assignments are shared.
- A seized procedure executes in its **defining** module's scope; it does not see the
  caller's module or local names.
- A module cannot seize a name it grants itself.
- A `SEIZE` with a non-empty new prefix is rejected because locally prefixed names are
  not supported.
- Contexts and remote pieces are implemented for their documented subset.

`z200c grant` records each export's simple name, module/package names, and optional
prefix, so duplicate simple names and prefixed outputs can be represented. Consumers can
use the corresponding qualified name when seizing from the grant file.

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
`target/z200c/<package>.grt.toml`. The grant file is a `z200c` artifact, not a Z.200
construct: the standard defines `GRANT`/`SEIZE` semantics (§12.2.3.4–12.2.3.5), not an
interchange format. The file must preserve those semantics, but its encoding is ours.

```toml
schema = 4
package = "spec"
package_dir = "/absolute/path/to/spec"

[[modes]]
name = "S"
module = "Counter"
package = "spec"
fields = [
  { name = "x", mode = "INT" },
  { name = "hidden", mode = "BOOL" },
]

[[exports]]
name = "S"
module = "Counter"
package = "spec"
kind = "mode"
mode = "spec!Counter!S"
forbid = ["hidden"]
friends = ["spec!Driver"]

[[exports]]
name = "Bump"
module = "Counter"
package = "spec"
prefix = "P"
kind = "procedure"

[[exports]]
name = "value"
module = "Counter"
package = "spec"
kind = "variable"
mode = "INT"
```

Named `SET (...)` modes are recorded with `kind = "set"` and an `[[modes.elements]]`
entry per element:

```toml
[[modes]]
name = "S"
module = "Counter"
package = "spec"
kind = "set"

[[modes.elements]]
name = "low"
value = "0"

[[modes.elements]]
name = "high"
value = "5"
```

Set representation values are stored as strings so values outside the TOML integer range
survive a round trip. Module, region, task, and interface modes are recorded with their
corresponding `kind` and component list.

Text types with an index mode are encoded as `TEXT(n) INT` or `TEXT(n) low:high`
(and likewise for `WTEXT`). Reference, pointer, row, and buffer types are encoded as
`REF <mode>`, `PTR`, `ROW <mode>`, and `BUFFER[(length)] <element-mode>`. The element of a buffer may be any
referable mode, including a symbolic structure-mode reference. Arrays of references,
nested `REF`/`ROW` targets, and references to fields or indexes remain unsupported.

Array types are encoded as `ARRAY(low:high)` for one dimension or
`ARRAY(low:high, low:high, ...)` for multidimensional arrays. The element mode may be
scalar or a symbolic structure-mode reference (`package!Module!Mode`). Up to four
dimensions are supported.
Structure modes use the symbolic identity `package!Module!Mode`; local `ModeId`s are never
written. All structure modes declared by named modules are recorded so field types can be
resolved, even if a nested mode is not granted itself. `forbid` is per grant, matching
`GRANT ... FORBID ...`.

Schema changes are breaking while the project is early: `z200c` accepts exactly
`schema = 4` and does not read older layouts.

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
- The language revision and integer widths are fixed by the compiler, so no
  per-dependency compatibility check is needed.
- Exported simple names may repeat; use `Module!name`, `package!Module!name`, or a
  granted prefix to disambiguate.
- `z200c check` and `z200c grant` resolve `SEIZE` against both forms.
- `z200c run` links directory dependencies and grant files with `package_dir`; it
  rejects a run only when an interface-only external name is actually seized.

## Current limits

- Interface-only grant files are static: `check`, `grant`, and structure-mode use work,
  but `run` cannot execute external variables/procedures without a source package.
- `SEIZE` with a non-empty new prefix (locally prefixed names) is not implemented.
- Structure modes travel through grant files: an external package can `SEIZE` a granted
  structure mode and use its fields statically. Grant files still contain interfaces,
  not bodies, so `run` needs source linking before external variables or procedures can
  be executed.
- Contexts, remote pieces, and symbol mangling for native code generation are not
  implemented.
