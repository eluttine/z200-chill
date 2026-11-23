Hello: MODULE
  AddScaled: PROC (a SHORT_INT, b SHORT_INT) RETURNS (LONG_INT);
    DCL scale LONG_INT;
    DCL result LONG_INT;
    scale := LONG_INT(b) * 2;
    result := LONG_INT(a) + scale;
    RETURN (result);
  END AddScaled;

  DCL answer LONG_INT;
  answer := AddScaled(3, 4);
  ASSERT answer = 11;
END Hello;
