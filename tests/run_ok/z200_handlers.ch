-- Direct Z.200 exception and handler examples.
M: MODULE
  DCL state INT INIT := 0;

  P: PROC (n INT) RETURNS (INT) EXCEPTIONS (fail);
    IF n < 0 THEN
      CAUSE fail;
    FI;
    RETURN n;
  END
  ON (fail):
    RESULT -1;
  END P;

  ASSERT P(-1) = -1;
  ASSERT P(3) = 3;

  CAUSE boom;
  state := 1;
END
ON (boom):
  state := 2;
  ASSERT state = 2;
ELSE
  ASSERT FALSE;
END;
