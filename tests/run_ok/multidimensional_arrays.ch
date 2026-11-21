-- Two-dimensional arrays and flat multidimensional indexing.
M: MODULE
  DCL m ARRAY (1:2, 1:3) INT;
  DCL n ARRAY (1:2, 1:3) INT;
  DCL r ARRAY (0:1, 2:3) INT;
  NEWMODE Point = STRUCT(x INT, y INT);
  DCL points ARRAY (1:2, 1:2) Point;
  DCL p Point;

  Sum: PROC (v ARRAY (1:2, 1:3) INT) RETURNS (INT);
    RETURN v(1, 1) + v(1, 2) + v(1, 3) + v(2, 1) + v(2, 2) + v(2, 3);
  END Sum;

  m(1, 1) := 1;
  m(1, 2) := 2;
  m(1, 3) := 3;
  m(2, 1) := 4;
  m(2, 2) := 5;
  m(2, 3) := 6;
  ASSERT m(2, 3) = 6;
  ASSERT Sum(m) = 21;

  n := m;
  ASSERT n = m;
  ASSERT n(1, 2) = 2;
  n(2, 3) := 9;
  ASSERT n(2, 3) = 9;
  ASSERT m(2, 3) = 6;

  r(0, 2) := 10;
  r(1, 3) := 40;
  ASSERT r(0, 2) = 10;
  ASSERT r(1, 3) = 40;

  p.x := 7;
  p.y := 8;
  points(1, 1) := p;
  ASSERT points(1, 1).x = 7;
  ASSERT points(1, 1).y = 8;
END M;
