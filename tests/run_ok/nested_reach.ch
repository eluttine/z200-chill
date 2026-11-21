M: MODULE
  A: MODULE
    GRANT x;
    DCL x INT INIT := 1;
  END A;
  B: MODULE
    SEIZE x;
    x := 2;
  END B;
  C: MODULE
    SEIZE x;
    ASSERT x = 2;
  END C;
END M;
