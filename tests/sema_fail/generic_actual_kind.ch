M: MODULE
  AddN: GENERIC
    MODE T;
    SYN N = 1;
    PROC (x INT) RETURNS (INT);
      RETURN x + N;
    END AddN;
  Bad: PROC = NEW AddN (3, INT) END;
END M;
