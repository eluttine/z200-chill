-- Seized modes in arrays, nested structures, and set-element constants.
Types: MODULE
  NEWMODE Point = STRUCT (x INT, y INT);
  NEWMODE Color = SET(red, green, blue);
  GRANT Point, Color;
END Types;

Use: MODULE
  SEIZE Point, Color;
  DCL p Point;
  DCL c Color;
  DCL grid ARRAY(1:2) Point;
  NEWMODE Line = STRUCT (a Point, b Point);
  DCL l Line;

  p.x := 1;
  p.y := 2;
  c := green;
  ASSERT c = green;
  ASSERT c /= red;

  grid(1).x := 3;
  grid(2).y := 4;
  l.a := p;
  l.b := grid(1);
  ASSERT l.a.x = 1;
  ASSERT l.b.x = 3;
  ASSERT grid(2).y = 4;
END Use;
