-- Generic PROC parameters.
M: MODULE
  AddOne: PROC (x INT) RETURNS (INT);
    RETURN x + 1;
  END AddOne;

  Twice: GENERIC
    PROC F (x INT) RETURNS (INT);
    PROC (x INT) RETURNS (INT);
      RETURN F(x) + F(x);
    END Twice;

  TwiceAddOne: PROC = NEW Twice (PROC F = AddOne;) END;

  ASSERT TwiceAddOne(3) = 8;
END M;
