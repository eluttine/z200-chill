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

## Deferred phase 2

- Indexed/dynamic text access. The current text mode does not retain an index mode, so
  `CONNECT(text, ..., index)` is rejected and connected text transfers are sequential.
- `GETTEXTACCESS` / `SETTEXTACCESS`, which need a program-visible reference to the
  access sub-location.
- `GETTEXTRECORD` / `SETTEXTRECORD`, which need a reference to text record storage
  rather than copying a string value.
- Assigning text sub-location references or retaining them beyond the text location's
  lifetime.

These are deliberate limits while text values remain string-valued in the
interpreter. A future phase can introduce reference-backed text storage without
changing the surface syntax documented here.
