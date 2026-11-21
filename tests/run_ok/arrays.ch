M: MODULE
  DCL a ARRAY (1:3) INT;
  DCL s ARRAY (0:1) CHARS;
  a(1) := 1;
  a(2) := 2;
  a(3) := a(1) + a(2);
  ASSERT a(3) = 3;
  s(0) := "x";
  ASSERT s(0) = "x";
END M;
