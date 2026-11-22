M: MODULE
  NEWMODE Checked = MODULE BODY
    DCL value INT INIT := 1;
    SetValue: PROC (n INT);
      value := n;
    END SetValue;
    INVARIANT value > 0
  END Checked;

  DCL c Checked;
  c.SetValue(0);
END M;
