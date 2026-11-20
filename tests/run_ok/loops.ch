M: MODULE
  DCL sum INT INIT := 0;
  DO FOR i := 1 TO 4;
    sum := sum + i;
  OD;
  ASSERT sum = 10;
END M;
