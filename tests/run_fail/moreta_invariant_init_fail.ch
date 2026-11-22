M: MODULE
  NEWMODE Checked = MODULE BODY
    DCL value INT INIT := 0;
    INVARIANT value > 0
  END Checked;

  DCL c Checked;
END M;
