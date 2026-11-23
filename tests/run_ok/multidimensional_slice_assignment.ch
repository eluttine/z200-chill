M: MODULE
  DCL a ARRAY(1:3, 1:3) INT;

  a := [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
  a(2:3, 2:3) := [[20, 21], [22, 23]];
  ASSERT a(2, 2) = 20;
  ASSERT a(2, 3) = 21;
  ASSERT a(3, 2) = 22;
  ASSERT a(3, 3) = 23;
  ASSERT a(1, 1) = 1;
END M;
