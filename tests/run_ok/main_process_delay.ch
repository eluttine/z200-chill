M: MODULE
  DCL e EVENT;
  DCL x INT INIT := 0;

  P: PROCESS ();
    x := 1;
    CONTINUE e;
  END P;

  START P();
  DELAY e;
  ASSERT x = 1;

  AFTER MILLISECS(1) DELAY IN
    DELAY e;
  TIMEOUT
    x := 2;
  END;
  ASSERT x = 2;
END M;
