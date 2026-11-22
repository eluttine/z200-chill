-- Module-mode procedure components.
M: MODULE
  NEWMODE Counter = MODULE BODY
    DCL value INT;
    Inc: PROC (n INT) RETURNS (INT);
      value := value + n;
      RETURN value;
    END Inc;
  END Counter;

  DCL c Counter;
  DCL d Counter;

  c.value := 1;
  ASSERT c.Inc(2) = 3;
  ASSERT c.value = 3;

  d.value := 10;
  ASSERT d.Inc(1) = 11;
  ASSERT d.value = 11;
  ASSERT c.value = 3;
END M;
