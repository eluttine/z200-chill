-- Direct Z.200 GRANT/SEIZE examples.
A: MODULE
  GRANT a PREFIXED P;
  GRANT b;
  DCL a INT INIT := 1;
  DCL b INT INIT := 2;
END A;

B: MODULE
  SEIZE a PREFIXED P;
  SEIZE b;
  ASSERT a + b = 3;
END B;

M: MODULE
  Child: MODULE
    GRANT x TO Sibling;
    DCL x INT INIT := 4;
  END Child;
  Sibling: MODULE
    SEIZE x;
    ASSERT x = 4;
  END Sibling;
END M;
