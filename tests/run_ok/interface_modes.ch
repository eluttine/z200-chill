-- Interface modes as abstract moreta bases.
M: MODULE
  NEWMODE ValueHolder = INTERFACE
    GRANT SetValue, GetValue;
    SetValue: PROC (n INT) END;
    GetValue: PROC () RETURNS (INT) END;
  END ValueHolder;

  NEWMODE Box = MODULE SPEC BASED_ON ValueHolder
    GRANT SetValue, GetValue;
    SetValue: PROC (n INT) END;
    GetValue: PROC () RETURNS (INT) END;
  END Box;
  NEWMODE Box = MODULE BODY BASED_ON ValueHolder
    DCL value INT INIT := 0;

    SetValue: PROC (n INT);
      value := n;
    END SetValue;

    GetValue: PROC () RETURNS (INT);
      RESULT value;
    END GetValue;
  END Box;

  DCL box Box;
  box.SetValue(7);
  ASSERT box.GetValue() = 7;
END M;
