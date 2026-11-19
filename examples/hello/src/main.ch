Hello: MODULE;
  AddScaled: PROC (a SHORT INT, b SHORT INT) RETURNS (LONG INT);
    DCL scale LONG INT;
    DCL result LONG INT;
    scale := LONG(b) * 2;
    result := LONG(a) + scale;
    RETURN (result);
  END AddScaled;

  DCL answer LONG INT;
  answer := AddScaled(3, 4);
  /* Expected output from entry/test harness: 11 */
END Hello;
