M: MODULE
  P: PROCESS (x INT LOC);
    x := 1;
  END P;
  START P(1 + 2);
END M;
