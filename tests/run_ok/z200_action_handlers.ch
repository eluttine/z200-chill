-- Z.200 8.2: handlers appended to actions.
M: MODULE
  DCL log INT INIT := 0;
  DCL i INT;

  BEGIN
    DCL a SHORT_INT INIT := 32767;
    a +:= 1;
    log := 100;
  END
  ON (OVERFLOW):
    log := 1;
  END;
  ASSERT log = 1;

  BEGIN
    ASSERT 1 = 2;
  END
  ON (ASSERTFAIL):
    log := 2;
  END;
  ASSERT log = 2;

  BEGIN
    CAUSE inner;
  END
  ON (inner):
    log := 3;
  END;
  ASSERT log = 3;

  BEGIN
    DCL outer INT INIT := 4;
    BEGIN
      DCL inner INT INIT := 2;
      CAUSE nested;
    END
    ON (nested):
      log := outer + inner;
    END;
  END;
  ASSERT log = 6;

  BEGIN
    log := 7;
  END
  ON (OVERFLOW):
    log := 99;
  END;
  ASSERT log = 7;

  IF FALSE THEN
    CAUSE branch;
  ELSE
    CAUSE branch;
  FI
  ON (branch):
    log := 8;
  END;
  ASSERT log = 8;

  CASE log OF
    (8): CAUSE alt;
    (ELSE): CAUSE alt;
  ESAC
  ON (alt):
    log := 9;
  END;
  ASSERT log = 9;

  DO FOR i := 1 TO 3;
    IF i = 2 THEN
      CAUSE loop;
    FI;
  OD
  ON (loop):
    log := 10;
  END;
  ASSERT log = 10;

  BEGIN
    DCL a SHORT_INT INIT := 32767;
    a +:= 1;
  END
  ON (OVERFLOW):
    log := 11;
  END;
  ASSERT log = 11;
END M;
