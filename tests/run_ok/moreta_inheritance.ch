-- Moreta module-body inheritance.
M: MODULE
  NEWMODE Base = MODULE BODY
    DCL value INT;
    Inc: PROC (n INT) RETURNS (INT);
      value := value + n;
      RETURN value;
    END Inc;
  END Base;

  NEWMODE Derived = MODULE BODY BASED_ON Base
    DCL extra INT;
    Total: PROC () RETURNS (INT);
      RETURN value + extra;
    END Total;
  END Derived;

  DCL d Derived;
  d.value := 3;
  d.extra := 4;
  ASSERT d.Inc(2) = 5;
  ASSERT d.Total() = 9;
END M;
