-- Floating-point literals, arithmetic, conversion, arrays, and format codes.
M: MODULE
  DCL f FLOAT;
  DCL g FLOAT INIT := -1.25;
  DCL arr ARRAY (1:2) FLOAT;
  DCL s CHARS;

  f := 1.5;
  ASSERT f = 1.5;
  ASSERT g = -1.25;

  f := 2.25 + 0.75;
  ASSERT f = 3.0;
  f := 3.0 * 0.5;
  ASSERT f = 1.5;
  f := 3.0 / 2.0;
  ASSERT f = 1.5;
  f := FLOAT(3) / 2.0;
  ASSERT f = 1.5;
  ASSERT f < 2.0;

  arr(1) := 1.5;
  arr(2) := arr(1) * 2.0;
  ASSERT arr(2) = 3.0;

  s := "";
  WRITETEXT(s, "%C", 1.5);
  ASSERT s = "1.5";

  s := "";
  WRITETEXT(s, "%C.2", 1.5);
  ASSERT s = "1.50";

  s := "";
  WRITETEXT(s, "%F", 1234.5);
  ASSERT s = "1.2345e003";

  s := "";
  WRITETEXT(s, "%F.2:2", 1234.5);
  ASSERT s = "1.23e03";

  s := "1.5";
  READTEXT(s, "%C", f);
  ASSERT f = 1.5;

  s := "1.23e03";
  READTEXT(s, "%F", f);
  ASSERT f = 1230.0;

  s := "  2.50";
  READTEXT(s, "%C5.2", f);
  ASSERT f = 2.5;
END M;
