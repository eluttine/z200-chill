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
  location's access sub-location. The text location is sequential-only for now.
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
  `CHARS`/`WCHARS` location. Assignments through either name are visible through the
  other.
- `GETTEXTRECORD(text)` returns a `REF CHARS` or `REF WCHARS` to the record cell.
  Dereferencing it reads or assigns the text record.
- `SETTEXTACCESS(text, access-location)` makes the text access sub-location alias the
  supplied `ACCESS` location, including its connection state.
- `GETTEXTACCESS(text)` returns a `REF ACCESS` to the access cell. It can be compared
  with `NULL`; using such a reference as a `CONNECT`/`DISCONNECT` transfer location is
  still deferred because transfer arguments are currently simple locations.

## Deferred

- Indexed/dynamic text access. The current text mode does not retain an index mode, so
  `CONNECT(text, ..., index)` is rejected and connected text transfers are sequential.
- Dereferencing a `GETTEXTACCESS` reference in a transfer-location position.
- General reference-to-record arguments in `SETTEXTRECORD`; the current form accepts a
  simple string location.
- Enforcing the text mode's length on assignments through a record reference.

These are deliberate limits. Reference-backed record/access sub-locations preserve the
surface syntax documented in [subset.md](subset.md).
