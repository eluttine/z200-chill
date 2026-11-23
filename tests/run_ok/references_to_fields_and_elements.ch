M: MODULE
  NEWMODE P = STRUCT (x INT, y INT);
  DCL p P;
  DCL a ARRAY(1:2) INT;
  DCL grid ARRAY(1:2, 1:2) P;
  DCL r REF INT;
  DCL rp REF P;

  r := ->p.x;
  r-> := 7;
  ASSERT p.x = 7;

  r := ->a(2);
  r-> := 9;
  ASSERT a(2) = 9;

  rp := ->grid(2, 1);
  rp->.x := 4;
  ASSERT grid(2, 1).x = 4;

  rp->.y := 5;
  ASSERT rp->.y = 5;
END M;
