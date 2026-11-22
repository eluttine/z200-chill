-- References, PTR, ROW, ALLOCATE/GETSTACK, and TERMINATE.
M: MODULE
  NEWMODE Cell = STRUCT(value INT, next REF Cell);

  DCL p REF Cell;
  DCL q REF Cell;
  DCL n INT INIT := 5;
  DCL r REF INT;
  DCL ptr PTR;
  DCL raw ROW CHARS(5);
  DCL stack_ref REF INT;

  r := ->n;
  ASSERT r-> = 5;
  r-> := 7;
  ASSERT n = 7;

  p := ALLOCATE(Cell);
  p->.value := 1;
  p->.next := NULL;
  ASSERT p->.next = NULL;

  q := ALLOCATE(Cell);
  q->.value := 2;
  q->.next := NULL;
  p->.next := q;
  ASSERT p->.next = q;
  ASSERT p->.next->.value = 2;

  ASSERT p /= NULL;
  ASSERT NULL = NULL;
  ptr := p;
  ASSERT ptr->Cell.value = 1;
  TERMINATE(ptr);

  raw := ALLOCATE(CHARS(5), "hello");
  ASSERT raw-> = "hello";
  raw-> := "world";
  ASSERT raw-> = "world";
  TERMINATE(raw);

  stack_ref := GETSTACK(INT, 3);
  ASSERT stack_ref-> = 3;
  TERMINATE(stack_ref);
END M;
