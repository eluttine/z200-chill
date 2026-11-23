M: MODULE
  DCL a ARRAY(1:3, 1:3) INT;
  DCL b ARRAY(2:3, 2:3) INT;
  DCL c ARRAY(1:2, 2:3) INT;

  a := [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
  b := a(2:3, 2:3);
  ASSERT b(2, 2) = 5;
  ASSERT b(2, 3) = 6;
  ASSERT b(3, 2) = 8;
  ASSERT b(3, 3) = 9;

  c := a(1:2, 2 UP 2);
  ASSERT c(1, 2) = 2;
  ASSERT c(2, 3) = 6;
END M;
