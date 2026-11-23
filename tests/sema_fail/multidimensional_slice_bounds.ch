M: MODULE
  DCL a ARRAY(1:2, 1:2) INT;
  DCL b ARRAY(1:1, 1:1) INT;
  b := a(1:3, 1:1);
END M;
