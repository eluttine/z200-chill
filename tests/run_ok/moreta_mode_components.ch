-- Mode components inside a module mode, including qualified access.
M: MODULE
  NEWMODE Holder = MODULE BODY
    NEWMODE Point = STRUCT (x INT, y INT);
    DCL p Point;

    Init: PROC (a INT, b INT) CONSTR;
      SELF.p.x := a;
      SELF.p.y := b;
    END Init;

    Sum: PROC () RETURNS (INT);
      RESULT SELF.p.x + SELF.p.y;
    END Sum;
  END Holder;

  DCL h Holder (3, 4);
  DCL other Holder!Point;

  ASSERT h.p.x = 3;
  ASSERT h.Sum() = 7;

  other.x := 10;
  other.y := 20;
  ASSERT other.x + other.y = 30;
END M;
