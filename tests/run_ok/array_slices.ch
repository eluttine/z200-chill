-- Array slices with constant bounds.
M: MODULE
  DCL a ARRAY (1:5) INT;
  DCL b ARRAY (1:3) INT;
  DCL c ARRAY (1:2) INT;
  DCL p ARRAY (1:2) INT INIT := [9, 10];
  DCL s ARRAY (1:3) CHARS;

  a := [10, 20, 30, 40, 50];

  b := a(2:4);
  ASSERT b(1) = 20;
  ASSERT b(2) = 30;
  ASSERT b(3) = 40;

  ASSERT a(2:3) = [20, 30];

  c := a(2 UP 2);
  ASSERT c(1) = 20;
  ASSERT c(2) = 30;

  a(2:3) := p;
  ASSERT a(2) = 9;
  ASSERT a(3) = 10;
  ASSERT a(4) = 40;

  a(4 UP 2) := p;
  ASSERT a(4) = 9;
  ASSERT a(5) = 10;

  s := ["a", "b", "c"];
  s(2:3) := ["x", "y"];
  ASSERT s(1) = "a";
  ASSERT s(2) = "x";
  ASSERT s(3) = "y";
END M;
