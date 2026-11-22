-- OUT, INOUT, and LOC parameter passing.
M: MODULE
  Inc: PROC (x INT INOUT);
    DCL x_local INT;
    x := x + 1;
  END Inc;

  Assign: PROC (x INT OUT);
    DCL a INT;
    a := 42;
    x := a;
  END Assign;

  Bump: PROC (x INT LOC);
    x := x + 5;
  END Bump;

  Swap: PROC (a INT INOUT, b INT INOUT);
    DCL temp INT;
    temp := a;
    a := b;
    b := temp;
  END Swap;

  NEWMODE Box = MODULE BODY
    DCL value INT;

    AddValue: PROC (x INT INOUT);
      x := x + value;
    END AddValue;
  END Box;

  DCL a INT INIT := 10;
  DCL b INT;
  DCL c INT INIT := 1;
  DCL box Box;
  DCL n INT INIT := 5;

  Inc(a);
  ASSERT a = 11;

  Assign(b);
  ASSERT b = 42;

  Bump(c);
  ASSERT c = 6;

  Swap(a, c);
  ASSERT a = 6;
  ASSERT c = 11;

  box.value := 100;
  box.AddValue(n);
  ASSERT n = 105;
END M;
