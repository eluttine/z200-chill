M: MODULE
  BEGIN
    A: MODULE
      GRANT x;
      DCL x INT INIT := 1;
    END A;
    B: MODULE
      SEIZE x;
      ASSERT x = 1;
    END B;
  END;
END M;
