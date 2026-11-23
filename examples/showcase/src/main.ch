-- Integration showcase: module modes, references, arrays, slices, and CASE.
M: MODULE
  NEWMODE Point = STRUCT (x INT, y INT);
  NEWMODE Counter = MODULE BODY
    DCL value INT INIT := 0;

    Init: PROC (n INT) CONSTR;
      value := n;
    END Init;

    Add: PROC (n INT) RETURNS (INT);
      value := value + n;
      RESULT value;
    END Add;
  END Counter;

  DCL c Counter (10);
  DCL points ARRAY(1:2) Point;
  DCL grid ARRAY(1:3, 1:3) INT;
  DCL part ARRAY(2:3, 2:3) INT;
  DCL r REF INT;

  ASSERT c.Add(5) = 15;

  points(1).x := 1;
  points(1).y := 2;
  points(2).x := 3;
  points(2).y := 4;

  r := ->points(2).x;
  r-> := 9;
  ASSERT points(2).x = 9;

  grid := [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
  part := grid(2:3, 2:3);
  ASSERT part(2, 2) = 5;
  ASSERT part(3, 3) = 9;

  CASE c.value OF
    (15): ASSERT points(1).y = 2;
    (ELSE): ASSERT FALSE;
  ESAC;
END M;
