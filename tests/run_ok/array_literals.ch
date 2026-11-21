-- Unlabelled array tuples in array-typed contexts.
M: MODULE
  DCL a ARRAY (1:3) INT;
  DCL b ARRAY (0:2) INT;
  DCL c ARRAY (1:2) INT INIT := [7, 8];
  DCL d ARRAY (1:2) INT;
  DCL s ARRAY (1:2) CHARS;

  Sum: PROC (v ARRAY (1:2) INT) RETURNS (INT);
    RETURN v(1) + v(2);
  END Sum;

  Pair: PROC () RETURNS (ARRAY (1:2) INT);
    RETURN [11, 12];
  END Pair;

  a := [1, 2, 3];
  ASSERT a(1) = 1;
  ASSERT a(3) = 3;
  ASSERT a = [1, 2, 3];

  b := (: 4, 5, 6 :);
  ASSERT b(0) = 4;
  ASSERT b(2) = 6;

  ASSERT c(2) = 8;
  ASSERT Sum([9, 10]) = 19;

  d := Pair();
  ASSERT d(1) = 11;
  ASSERT d(2) = 12;

  s := ["a", "b"];
  ASSERT s(1) = "a";
  ASSERT s(2) = "b";
END M;
