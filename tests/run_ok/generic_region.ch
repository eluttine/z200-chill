Stack: GENERIC MODE T;
  REGION
    GRANT Set, Get;
    DCL value T;

    Set: PROC (n T);
      value := n;
    END Set;

    Get: PROC () RETURNS (T);
      RESULT value;
    END Get;
  END Stack;

StackInt: REGION = NEW Stack(INT) END;

Client: MODULE
  SEIZE Set, Get;
  Set(5);
  ASSERT Get() = 5;
END Client;
