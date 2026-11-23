M: MODULE
  NEWMODE S = SET(red, blue);
  NEWMODE P = MODULE BODY
    DCL value INT INIT := 0;
  END P;

  DCL sets ARRAY(1:2) S;
  DCL objs ARRAY(1:2) P;
  DCL ps ARRAY(1:2) POWERSET INT;

  sets(1) := red;
  sets(2) := blue;
  ASSERT sets(1) = red;
  ASSERT sets(2) /= red;

  objs(1).value := 5;
  objs(2).value := 7;
  ASSERT objs(1).value = 5;
  ASSERT objs(2).value = 7;

  ps(1) := [1, 3];
  ASSERT 3 IN ps(1);
END M;
