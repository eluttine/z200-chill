-- ASSIGNABLE module-mode objects support whole-object copying.
M: MODULE
  NEWMODE P = MODULE BODY ASSIGNABLE
    DCL value INT INIT := 0;
    Set: PROC (n INT);
      value := n;
    END Set;
  END P;

  DCL a P;
  DCL b P;
  a.Set(5);
  b := a;
  ASSERT b.value = 5;
END M;
