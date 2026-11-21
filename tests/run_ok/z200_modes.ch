-- Direct Z.200 mode and structure examples.
M: MODULE
  NEWMODE Point = STRUCT(x SHORT_INT, y SHORT_INT);
  NEWMODE Segment = STRUCT(start Point, finish Point);
  SYNMODE Count = INT;

  DCL a Segment, b Segment;
  DCL n Count;
  DCL big LONG_INT;

  a.start.x := 1;
  a.start.y := 2;
  a.finish.x := 3;
  a.finish.y := 4;

  b := a;
  ASSERT a = b;
  b.finish.y := 5;
  ASSERT a /= b;

  n := INT(a.finish.x) * 10;
  ASSERT n = 30;
  big := LONG_INT(a.finish.y) * 1_000_000;
  ASSERT big = 4_000_000;
END M;
