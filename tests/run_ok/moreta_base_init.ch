-- Inherited components keep base initializer behavior.
M: MODULE
  NEWMODE Base = MODULE SPEC
    DCL value INT;
  END Base;
  NEWMODE Base = MODULE BODY
    DCL value INT INIT := 4;
    Inc: PROC (n INT) RETURNS (INT);
      value := value + n;
      RETURN value;
    END Inc;
  END Base;

  NEWMODE Derived = MODULE BODY BASED_ON Base
    DCL extra INT INIT := value + 1;
    Total: PROC () RETURNS (INT);
      RETURN value + extra;
    END Total;
  END Derived;

  DCL d Derived;
  ASSERT d.value = 4;
  ASSERT d.extra = 5;
  ASSERT d.Inc(1) = 5;
  ASSERT d.Total() = 10;
END M;
