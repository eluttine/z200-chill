M: MODULE
  Wrong: PROC (x CHAR) RETURNS (INT);
    RETURN 1;
  END Wrong;

  Twice: GENERIC
    PROC F (x INT) RETURNS (INT);
    PROC (x INT) RETURNS (INT);
      RETURN F(x) + F(x);
    END Twice;

  Bad: PROC = NEW Twice (PROC F = Wrong;) END;
END M;
