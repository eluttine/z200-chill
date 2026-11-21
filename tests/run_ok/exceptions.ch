M: MODULE
  DCL x INT INIT := 0;
  CAUSE boom;
  x := 1;
END
ON (boom):
  x := 2;
  ASSERT x = 2;
END;
