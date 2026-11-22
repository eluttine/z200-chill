M: MODULE
  DCL a EVENT;
  DCL b EVENT;
  DCL x INT INIT := 0;
  DCL y INT INIT := 0;

  Waiter: PROCESS ();
    DELAY CASE
      (a):
        x := 1;
      (b):
        x := 2;
    ESAC;
    ASSERT x = 2;
  END Waiter;

  Signaler: PROCESS ();
    CONTINUE b;
  END Signaler;

  Timed: PROCESS ();
    AFTER MILLISECS(5) DELAY IN
      DELAY CASE
        (a):
          y := 1;
      ESAC;
    TIMEOUT
      y := 9;
    END;
    ASSERT y = 9;
  END Timed;

  START Waiter();
  START Signaler();
  START Timed();
END M;
