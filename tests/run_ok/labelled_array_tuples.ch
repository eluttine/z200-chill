-- Labelled array tuples.
M: MODULE
  DCL a ARRAY (1:4) INT;
  DCL b ARRAY (0:3) INT;
  DCL c ARRAY (1:3) INT;
  DCL d ARRAY (1:4) INT;

  a := (: (1, 2): 5, (3): 7, (ELSE): 9 :);
  ASSERT a(1) = 5;
  ASSERT a(2) = 5;
  ASSERT a(3) = 7;
  ASSERT a(4) = 9;

  b := [ (0:1): 10, (2:3): 20 ];
  ASSERT b(0) = 10;
  ASSERT b(1) = 10;
  ASSERT b(2) = 20;
  ASSERT b(3) = 20;

  c := [ (*): 3 ];
  ASSERT c(1) = 3;
  ASSERT c(2) = 3;
  ASSERT c(3) = 3;

  d := (: (1): 1, (2:3): 2, (4): 4 :);
  ASSERT d = [1, 2, 2, 4];
END M;
