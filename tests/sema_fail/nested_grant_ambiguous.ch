M: MODULE
  A: MODULE
    GRANT x;
    DCL x INT INIT := 1;
  END A;
  B: MODULE
    GRANT x;
    DCL x INT INIT := 2;
  END B;
  C: MODULE
    SEIZE x;
  END C;
END M;
