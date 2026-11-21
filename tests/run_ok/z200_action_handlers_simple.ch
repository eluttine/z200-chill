-- Handlers appended to assignment, assert, and call actions.
M: MODULE
  DCL log INT INIT := 0;

  Fail: PROC () EXCEPTIONS (proc_fail);
    CAUSE proc_fail;
  END Fail;

  DCL a SHORT_INT INIT := 32767;
  a +:= 1
  ON (OVERFLOW):
    log := 1;
  END;
  ASSERT log = 1;

  ASSERT 1 = 2
  ON (ASSERTFAIL):
    log := 2;
  END;
  ASSERT log = 2;

  Fail()
  ON (proc_fail):
    log := 3;
  END;
  ASSERT log = 3;
END M;
