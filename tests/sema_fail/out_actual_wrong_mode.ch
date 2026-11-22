M: MODULE
  Assign: PROC (x LONG_INT OUT);
    x := 1;
  END Assign;

  DCL a INT;
  Assign(a);
END M;
