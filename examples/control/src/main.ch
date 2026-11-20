Control: MODULE
  DCL sum INT INIT := 0;
  DCL score INT INIT := 0;

  DO FOR i := 1 TO 4;
    sum := sum + i;
  OD;
  ASSERT sum = 10;

  DO FOR i := 4 BY 1 DOWN TO 1;
    sum := sum + i;
  OD;
  ASSERT sum = 20;

  CASE score OF
    (0): score := 5;
    (1:3): score := 10;
    (ELSE): score := 99;
  ESAC;
  ASSERT score = 5;

Outer: DO FOR i := 1 TO 3;
    DO FOR j := 1 TO 3;
      sum := sum + 1;
      IF sum = 24 THEN
        EXIT Outer;
      FI;
    OD;
  OD;
  ASSERT sum = 24;
END Control;
