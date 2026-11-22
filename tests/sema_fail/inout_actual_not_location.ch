M: MODULE
  Inc: PROC (x INT INOUT);
    x := x + 1;
  END Inc;

  DCL a INT INIT := 1;
  Inc(a + 1);
END M;
