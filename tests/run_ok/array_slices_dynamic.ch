-- Array slices whose bounds are computed at runtime.
M: MODULE
  DCL a ARRAY (1:5) INT;
  DCL b ARRAY (1:3) INT;
  DCL c ARRAY (1:2) INT;
  DCL p ARRAY (1:3) INT INIT := [7, 8, 9];
  DCL first INT INIT := 2;
  DCL upper INT INIT := 4;
  DCL size INT INIT := 2;

  Sum: PROC (v ARRAY (1:3) INT) RETURNS (INT);
    RETURN v(1) + v(2) + v(3);
  END Sum;

  a := [10, 20, 30, 40, 50];

  b := a(first:upper);
  ASSERT b(1) = 20;
  ASSERT b(2) = 30;
  ASSERT b(3) = 40;

  c := a(first UP size);
  ASSERT c(1) = 20;
  ASSERT c(2) = 30;

  ASSERT Sum(a(first:upper)) = 90;
  ASSERT a(first:upper) = b;

  a(first:upper) := p;
  ASSERT a(2) = 7;
  ASSERT a(3) = 8;
  ASSERT a(4) = 9;

  a(first UP size) := p(2:3);
  ASSERT a(2) = 8;
  ASSERT a(3) = 9;

  first := 1;
  upper := 2;
  a(first:upper) := p(1:2);
  ASSERT a(1) = 7;
  ASSERT a(2) = 8;
END M;
