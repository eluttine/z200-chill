-- Arrays whose element mode is a structure.
M: MODULE
  NEWMODE Point = STRUCT(x INT, y INT);
  DCL points ARRAY (0:1) Point;
  DCL copy ARRAY (0:1) Point;
  DCL p Point;

  points(0).x := 1;
  points(0).y := 2;
  points(1).x := 3;
  points(1).y := 4;

  ASSERT points(0).x = 1;
  ASSERT points(0).y = 2;
  ASSERT points(1).x = 3;

  p := points(1);
  ASSERT p.x = 3;
  ASSERT p.y = 4;

  copy := points;
  ASSERT copy(1).y = 4;
  copy(1).x := 5;
  ASSERT points(1).x = 3;
END M;
