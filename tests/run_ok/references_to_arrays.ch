M: MODULE
  DCL a ARRAY(1:2) INT;
  DCL r REF ARRAY(1:2) INT;

  a(1) := 1;
  a(2) := 2;
  r := ->a;
  ASSERT r->(2) = 2;
  r->(1) := 7;
  ASSERT a(1) = 7;
END M;
