M: MODULE
  DCL base_destroyed INT INIT := 0;
  NEWMODE Base = MODULE BODY
    DCL value INT INIT := 1;
    Init: PROC (n INT) CONSTR;
      value := n;
    END Init;
    Fin: PROC () DESTR;
      base_destroyed := base_destroyed + 1;
    END Fin;
  END Base;

  NEWMODE Derived = MODULE BODY BASED_ON Base
    DCL extra INT INIT := 2;
  END Derived;

  BEGIN
    DCL b Base (5);
    ASSERT b.value = 5;
  END;
  ASSERT base_destroyed = 1;

  BEGIN
    DCL d Derived;
    ASSERT d.value = 1;
    ASSERT d.extra = 2;
  END;
  ASSERT base_destroyed = 1;
END M;
