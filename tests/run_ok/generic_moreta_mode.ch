M: MODULE
  NEWMODE GenericBox = GENERIC MODE T;
    MODULE BODY
      DCL value T INIT := 0;

      Set: PROC (n T);
        value := n;
      END Set;

      Get: PROC () RETURNS (T);
        RESULT value;
      END Get;
    END;

  NEWMODE BoxInt = NEW GenericBox(INT) END;

  DCL box BoxInt;
  box.Set(4);
  ASSERT box.Get() = 4;
END M;
