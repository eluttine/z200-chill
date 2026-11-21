M: MODULE
  DCL a ARRAY (1:3) INT;
  DCL b ARRAY (1:2) INT;
  DCL first INT INIT := 2;
  DCL upper INT INIT := 4;
  b := a(first:upper);
END M;
