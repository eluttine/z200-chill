-- Module-mode component initializers and non-value methods.
M: MODULE
  NEWMODE Counter = MODULE BODY
    DCL value INT INIT := 5;
    DCL next INT INIT := value + 1;

    Inc: PROC (n INT) RETURNS (INT);
      value := value + n;
      RETURN value;
    END Inc;

    Reset: PROC ();
      value := 0;
    END Reset;
  END Counter;

  DCL c Counter;
  ASSERT c.value = 5;
  ASSERT c.next = 6;
  ASSERT c.Inc(2) = 7;
  ASSERT c.value = 7;
  c.Reset();
  ASSERT c.value = 0;
END M;
