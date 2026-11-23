M: MODULE
  DCL a ARRAY(1:2, 1:2) INT;
  DCL b ARRAY(1:2, 1:2) INT;

  a := [[1, 2], [3, 4]];
  ASSERT a(1, 1) = 1;
  ASSERT a(1, 2) = 2;
  ASSERT a(2, 1) = 3;
  ASSERT a(2, 2) = 4;

  b := a;
  ASSERT b(2, 2) = 4;
END M;
