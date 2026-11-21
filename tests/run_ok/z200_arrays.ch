-- Direct Z.200 array examples.
M: MODULE
  DCL a ARRAY (-2:2) INT;
  DCL b ARRAY (-2:2) INT;
  DCL i INT;

  DO FOR i := -2 TO 2;
    a(i) := i * i;
  OD;

  b := a;
  ASSERT b(-2) = 4;
  ASSERT b(0) = 0;
  ASSERT b(2) = 4;

  a(2) := 10;
  ASSERT a(2) = 10;
  ASSERT b(2) = 4;
END M;
