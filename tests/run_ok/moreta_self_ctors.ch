-- SELF, constructors, destructors, and constructor arguments.
M: MODULE
  DCL destroyed INT INIT := 0;

  NEWMODE Counter = MODULE BODY
    DCL value INT;

    Init: PROC (n INT) CONSTR;
      SELF.value := n;
    END Init;

    Add: PROC (n INT) RETURNS (INT);
      SELF.value := SELF.value + n;
      RETURN SELF.value;
    END Add;

    BumpTwice: PROC ();
      SELF.Add(1);
      SELF.Add(1);
    END BumpTwice;

    Fin: PROC () DESTR;
      destroyed := destroyed + 1;
    END Fin;
  END Counter;

  DCL a Counter (10);
  DCL b Counter (20);
  DCL p REF Counter;

  ASSERT a.value = 10;
  ASSERT a.Add(5) = 15;
  a.BumpTwice();
  ASSERT a.value = 17;
  ASSERT b.value = 20;

  p := ALLOCATE(Counter, (30));
  ASSERT p->.value = 30;
  p->.Add(2);
  ASSERT p->.value = 32;
  TERMINATE(p);
  ASSERT destroyed = 1;
END M;
