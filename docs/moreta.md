# z200-chill moreta subset

Status: **intentionally narrow**. This document pins down the module-mode subset that
`z200c` implements today and the semantics that are project choices rather than
Z.200 requirements.

## Supported

- `NEWMODE Name = MODULE BODY ... END;`
- paired `MODULE SPEC` / `MODULE BODY` definitions with the same name,
- variable components,
- procedure components and static method calls `object.Method(...)`,
- value-returning methods in expressions and non-value methods as call actions,
- component `DCL ... INIT := ...` evaluation,
- `BASED_ON` single or multiple module-mode inheritance,
- `INVARIANT boolean-expression` at the end of a mode body.

## Object semantics

- A declared module-mode object owns independent component values.
- Module-mode objects are **not assignable**. Whole-object assignment and passing an
  object where a value of module-mode type is required are rejected. This matches the
  standard default `NOT_ASSIGNABLE` behavior more closely than value copying.
- Module-mode types are module-local and cannot appear in grant interfaces yet.
- Objects are initialized when their declaration is executed.
- Initialization order is base components first, then derived components, in
  declaration order.
- Method calls copy component values into a temporary method scope, execute, and write
  mutated component values back to the object on success.
- Method calls are statically dispatched by the object's declared module mode.

## Specs and inheritance

- A matching spec/body pair is merged during normalization. Body components override
  same-named spec components.
- Every spec procedure component must have a matching body procedure component.
- A spec without a body is abstract: declaring an object of that mode is rejected.
- `BASED_ON` inherits base components; derived same-named components override them.
- Base component initializers run before derived initializers.
- Spec and body invariants are combined with `AND`; inherited invariants are checked
  with derived invariants.

## Invariants

- Invariants are checked after object initialization and before/after every method
  call.
- Failure raises the catchable `INVFAIL` exception.
- Invariants cannot yet be checked around process or constructor/destructor calls
  because those are not implemented.

## Not implemented

- constructors/destructors (`CONSTR`, `DESTR`),
- task modes, region modes, interface modes, and object references,
- `SELF` and unqualified sibling method calls,
- mode components and `Mode!Component` access,
- `IMPLEMENTS`, assignability attributes, abstract/final attributes,
- public/private component visibility and `GRANT` inside module-mode specs,
- generic moreta templates,
- grant-file encoding for set or module-mode types.

These exclusions are deliberate. Adding them should be a separate design step rather
than an incidental extension.
