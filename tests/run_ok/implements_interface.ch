-- IMPLEMENTS is treated as interface inheritance in this subset.
M: MODULE
  NEWMODE I = INTERFACE
    GRANT Get;
    Get: PROC () RETURNS (INT) END;
  END I;

  NEWMODE P = MODULE BODY IMPLEMENTS I
    DCL value INT INIT := 9;
    Get: PROC () RETURNS (INT);
      RESULT value;
    END Get;
  END P;

  DCL p P;
  ASSERT p.Get() = 9;
END M;
