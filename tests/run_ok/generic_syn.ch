-- Generic SYN parameters in procedure and module templates.
M: MODULE
  AddN: GENERIC
    MODE T;
    SYN N = 1;
    PROC (x INT) RETURNS (INT);
      RETURN x + N;
    END AddN;

  Add3: PROC = NEW AddN (INT, 3) END;
  Add10: PROC = NEW AddN (INT, 10) END;

  ASSERT Add3(4) = 7;
  ASSERT Add10(4) = 14;
END M;

Pair: GENERIC
  MODE T;
  SYN N = 1;
  MODULE
    DCL x INT INIT := N;
    ASSERT x = N;
  END Pair;

Pair5: MODULE = NEW Pair (INT, 5) END;
