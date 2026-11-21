-- Direct Z.200 control-flow examples.
M: MODULE
  DCL total INT INIT := 0;
  DCL i INT;

  DO FOR i := 1 TO 5;
    IF i MOD 2 = 0 THEN
      total +:= i;
    ELSIF i = 5 THEN
      total +:= 100;
    ELSE
      total +:= 0;
    FI;
  OD;
  ASSERT total = 106;

  DO FOR i := 3 BY 1 DOWN TO 1;
    total -:= i;
  OD;
  ASSERT total = 100;

  DO WHILE total > 95;
    total -:= 1;
  OD;
  ASSERT total = 95;

  CASE total OF
    (0:10): i := 0;
    (11:90): i := 1;
    (91:99): i := 2;
    (100): i := 3;
    (ELSE): i := 4;
  ESAC;
  ASSERT i = 2;

  Outer: DO FOR i := 1 TO 3;
    DO FOR total := 1 TO 3;
      IF total = 2 THEN
        EXIT Outer;
      FI;
    OD;
  OD;
  ASSERT total = 2;
END M;
