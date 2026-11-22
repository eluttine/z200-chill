M: MODULE
  NEWMODE Base = MODULE SPEC
    DCL value INT;
    INVARIANT value >= 0
  END Base;

  NEWMODE Base = MODULE BODY
    DCL value INT INIT := 1;
    Set: PROC (n INT);
      value := n;
    END Set;
  END Base;

  DCL b Base;
  b.Set(2);
  ASSERT b.value = 2;
END M;
