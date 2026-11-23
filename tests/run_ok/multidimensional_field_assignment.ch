M: MODULE
  NEWMODE P = STRUCT (x INT, y INT);
  DCL a ARRAY(1:2, 1:2) P;
  DCL b ARRAY(1:2) P;

  a(1, 1).x := 5;
  a(2, 2).y := 7;
  ASSERT a(1, 1).x = 5;
  ASSERT a(2, 2).y = 7;

  b(2).x := 3;
  ASSERT b(2).x = 3;
END M;
