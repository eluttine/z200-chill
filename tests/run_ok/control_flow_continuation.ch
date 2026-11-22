-- Actions after IF/DO/CASE/BEGIN must continue executing.
M: MODULE
  DCL x INT INIT := 0;
  DCL i INT;
  DCL y INT INIT := 0;

  IF TRUE THEN
    x := 1;
  FI;
  ASSERT x = 1;

  DO FOR i := 1 TO 2;
    x := x + 1;
  OD;
  ASSERT x = 3;

  CASE x OF
    (3): y := 4;
    ELSE y := 9;
  ESAC;
  ASSERT y = 4;

  BEGIN
    DCL z INT INIT := 5;
    y := z;
  END;
  ASSERT y = 5;
END M;
