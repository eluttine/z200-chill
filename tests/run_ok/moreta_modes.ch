-- Minimal module-mode objects with variable components.
M: MODULE
  NEWMODE Point = MODULE BODY
    DCL x INT;
    DCL y INT;
  END Point;

  DCL p Point;
  DCL q Point;

  p.x := 1;
  p.y := 2;
  ASSERT p.x = 1;
  ASSERT p.y = 2;

  q.x := 3;
  q.y := 4;
  ASSERT q.x = 3;
  ASSERT p.x = 1;
END M;
