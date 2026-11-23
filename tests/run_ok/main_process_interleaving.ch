M: MODULE
  DCL x INT INIT := 0;
  DCL y INT INIT := 0;

  Worker: PROCESS ();
    x := 1;
    y := 2;
  END Worker;

  START Worker();
  ASSERT x = 1;
  ASSERT y = 2;
END M;
