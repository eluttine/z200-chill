M: MODULE
  DCL x INT INIT := 0;
  DCL done EVENT;

  Bump: PROCESS (y INT LOC);
    y := y + 1;
    CONTINUE done;
  END Bump;

  Checker: PROCESS ();
    DELAY done;
    ASSERT x = 1;
  END Checker;

  START Checker();
  START Bump(x);
END M;
