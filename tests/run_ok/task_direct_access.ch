M: MODULE
  SIGNAL inside;
  SIGNAL done;
  DCL gate EVENT;
  DCL c Counter;

  NEWMODE Counter = TASK BODY
    DCL x INT INIT := 0;

    Bump: PROC ();
      x := x + 1;
      SEND inside;
      SEND inside;
      DELAY gate;
      x := x + 10;
    END Bump;
  END Counter;

  Worker: PROCESS ();
    c.Bump();
  END Worker;

  Mutator: PROCESS ();
    RECEIVE CASE
      (inside):
    ESAC;
    c.x := 100;
    SEND done;
  END Mutator;

  Closer: PROCESS ();
    RECEIVE CASE
      (inside):
    ESAC;
    CONTINUE gate;
  END Closer;

  Checker: PROCESS ();
    RECEIVE CASE
      (done):
    ESAC;
    ASSERT c.x = 100;
  END Checker;

  START Worker();
  START Mutator();
  START Closer();
  START Checker();
END M;
