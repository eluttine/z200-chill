-- Module-mode invariants are checked around method calls.
M: MODULE
  NEWMODE Checked = MODULE BODY
    DCL value INT INIT := 1;

    SetValue: PROC (n INT);
      value := n;
    END SetValue;

    Get: PROC () RETURNS (INT);
      RETURN value;
    END Get;

    INVARIANT value > 0
  END Checked;

  DCL c Checked;
  ASSERT c.Get() = 1;
  c.SetValue(2);
  ASSERT c.Get() = 2;
END M;
