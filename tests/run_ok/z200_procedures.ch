-- Direct Z.200 procedure examples.
M: MODULE
  DCL acc INT INIT := 0;

  Fact: PROC (n INT) RETURNS (INT);
    IF n <= 1 THEN
      RETURN 1;
    FI;
    RETURN n * Fact(n - 1);
  END Fact;

  Add: PROC (a INT, b INT);
    acc := acc + a + b;
  END Add;

  Clamp: PROC (x INT) RETURNS (INT);
    RESULT x;
  END Clamp;

  Add(2, 3);
  ASSERT acc = 5;
  ASSERT Fact(5) = 120;
  ASSERT Clamp(7) = 7;
END M;
