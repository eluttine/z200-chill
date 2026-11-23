# Text Locations and Access Sub-locations

This note records the `z200c` text-location model and the staged implementation of
Z.200 text sub-locations.

## Model

A `TEXT(n)` or `WTEXT(n)` location has:

- a **text record** — the current character/wide-character string;
- an **actual index** — the default cursor used by text transfer operations;
- an **access sub-location** — connection state shared with `ACCESS` locations.

The text record and actual index live in the interpreter variable. The access
sub-location is stored alongside the variable and uses the same `AccessState` as an
`ACCESS` location.

## Implemented phase 1

- `CONNECT(text, association [, usage [, where [, index]]])` connects the text
  location's access sub-location. Sequential text modes accept no index; text modes
  declared with an index mode accept the index expression.
- `DISCONNECT(text)` clears the connection.
- `GETUSAGE(text)`, `OUTOFFILE(text)`, and `GETASSOCIATION(text)` operate on the text
  access sub-location.
- While connected, `WRITETEXT` formats into the text record and writes the resulting
  record to the associated file at the current access position.
- While connected, `READTEXT` reads the next record from the file into the text record
  before applying format conversions. End of file sets `OUTOFFILE` and performs no
  transfer.
- Disconnected `READTEXT`/`WRITETEXT` keep the in-memory behavior documented in
  [subset.md](subset.md).
- Text connections honor `READONLY`/`WRITEONLY` usage restrictions and raise
  `READFAIL`/`WRITEFAIL` on the wrong transfer direction.
- `SETTEXTINDEX`/`GETTEXTINDEX` continue to manage the actual index; text transfers
  reset it to `0`.

## Implemented phase 2

A text location now owns shared state containing:

- a record cell (the text record sub-location),
- the actual index,
- an access cell (the access sub-location).

This makes the sub-locations reference-backed:

- `SETTEXTRECORD(text, string-location)` makes the text record alias the supplied
  bounded varying string location of the same width and length. Assignments through
  either name are visible through the other.
- `GETTEXTRECORD(text)` returns a `REF CHARS(n) VARYING` or
  `REF WCHARS(n) VARYING` to the record cell. Dereferencing it reads or assigns the
  text record, and the bounded reference mode enforces the text length.
- `SETTEXTACCESS(text, access-location)` makes the text access sub-location alias the
  supplied `ACCESS` location, including its connection state.
- `GETTEXTACCESS(text)` returns a `REF ACCESS` to the access cell. Dereferenced access
  references (`ref->`) can be used as `CONNECT`/`DISCONNECT` transfer locations and
  with `GETUSAGE`, `OUTOFFILE`, and `GETASSOCIATION`.

## Indexed text access

Text modes retain their optional index mode:

```chill
DCL t TEXT(20) 0:3;
DCL bounded TEXT(20) INT;
```

- `CONNECT(text, ..., index)` is accepted when the text mode has an index mode;
  otherwise the connection is sequential.
- For a connected text location, an explicit index argument to `READTEXT`/`WRITETEXT`
  selects the file record. Without it, the access position is used.
- Indexed reads/writes check the declared index range and raise `RANGEFAIL` outside it.
- `OUTOFFILE` applies to indexed reads as well; a later indexed read can still succeed.
- `DYNAMIC` remains accepted on the text mode and is effectively satisfied by the
  varying string record model.

- `SETTEXTRECORD` accepts either a simple string location or a dereferenced reference
  (`ref->`) to one.
- Text transfers and record-reference assignments reject values longer than the text
  mode's declared length.

These rules preserve the surface syntax documented in [subset.md](subset.md).
