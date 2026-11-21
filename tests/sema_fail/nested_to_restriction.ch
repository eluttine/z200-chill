M: MODULE
  A: MODULE
    GRANT x TO B;
    DCL x INT INIT := 1;
  END A;
  C: MODULE
    SEIZE x;
  END C;
END M;
