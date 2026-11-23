# z200-chill moreta subset

Status: **intentionally narrow**. This document pins down the module-mode subset that
`z200c` implements today and the semantics that are project choices rather than
Z.200 requirements.

## Supported

- `NEWMODE Name = MODULE BODY ... END;`
- `NEWMODE Name = REGION SPEC/BODY ... END;` with per-object mutual exclusion on
  component method calls
- `NEWMODE Name = TASK SPEC/BODY ... END;` with a per-object task worker processing
  queued component procedure calls asynchronously; direct field access is serialized
  with the task worker, and `TERMINATE` stops an allocated task worker
- paired `MODULE SPEC` / `MODULE BODY` definitions with the same name,
- variable components,
- procedure components and static method calls `object.Method(...)`,
- value-returning methods in expressions and non-value methods as call actions,
- component `DCL ... INIT := ...` evaluation,
- `SELF` field access (`SELF.field`) and sibling method calls (`SELF.Method(...)`),
- constructors (`CONSTR`) and destructors (`DESTR`),
- constructor arguments in declarations (`DCL x M (args);`) and `ALLOCATE(M, (args))`,
- nested mode components (`NEWMODE`/`SYNMODE` inside a module body) accessed by
  simple name inside the body and as `Mode!Component` outside it,
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
- `SELF` refers to the current object inside a component procedure. Nested
  `SELF.Method(...)` calls operate on a temporary copy and copy component changes
  back before the outer method continues.

## Specs and inheritance

- A matching spec/body pair is merged during normalization. Body components override
  same-named spec components.
- Every spec procedure component must have a matching body procedure component.
- A spec without a body is abstract: declaring an object of that mode is rejected.
- `BASED_ON` inherits base components; derived same-named components override them.
- Base component initializers run before derived initializers.
- Spec and body invariants are combined with `AND`; inherited invariants are checked
  with derived invariants.

## Constructors and destructors

- A module mode may define at most one `CONSTR` and one `DESTR` component.
- Constructors may take `IN` parameters and must not return a value.
- Destructors take no parameters and must not return a value.
- Component initializers run in textual order, then the constructor, then invariants.
- Constructors run for declared objects and for `ALLOCATE` / `GETSTACK` calls with
  constructor arguments.
- Destructors run when `TERMINATE` ends an allocated module-mode location.
- Destructors for statically declared objects at the end of their reach or lifetime are
  not implemented yet.

## Invariants

- Invariants are checked after object initialization (including constructors) and
  before/after every method call.
- Failure raises the catchable `INVFAIL` exception.
- Invariants are also checked around constructor/destructor calls through the normal
  method-call path.

## Grant interfaces

- Moreta modes are encoded in grant files with their variable, procedure, and mode
  components.
- Exported variables and procedure signatures may reference moreta modes
  symbolically.
- Nested mode components have a limited grant encoding: the underlying modes are
  serialized as module modes, but preserving `Mode!Component` names across packages
  is not complete yet.

## Not implemented

- interface modes,
- static-object destructors at reach/lifetime end,
- `IMPLEMENTS`, assignability attributes, abstract/final attributes,
- public/private component visibility and `GRANT` inside module-mode specs,
- generic moreta templates,
- complete `Mode!Component` name preservation in grant files,
- grant-file encoding for named set modes.

These exclusions are deliberate. Adding them should be a separate design step rather
than an incidental extension.
