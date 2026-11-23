M: MODULE
  NEWMODE A = STRUCT(next REF B);
  NEWMODE B = STRUCT(next REF A);
  DCL a A;
  DCL b B;
  a.next := NULL;
  b.next := NULL;
  ASSERT a.next = NULL;
  ASSERT b.next = NULL;
END M;
