M: MODULE
  DCL n INT INIT := 0;
  DCL refs ARRAY(1:2) REF INT;
  DCL ptrs ARRAY(1:1) PTR;
  DCL rows ARRAY(1:1) ROW CHARS(5);

  refs(1) := ->n;
  refs(1)-> := 4;
  ASSERT n = 4;

  ptrs(1) := NULL;
  ASSERT ptrs(1) = NULL;

  rows(1) := ALLOCATE(CHARS(5), "hello");
  ASSERT rows(1)-> = "hello";
END M;
