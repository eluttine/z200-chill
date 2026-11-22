-- EVENT locations, DELAY, and CONTINUE.
M: MODULE
  DCL ready EVENT;
  DCL one_slot EVENT(1);
  DCL first INT INIT := 0;
  DCL second INT INIT := 0;

  Waiter: PROCESS ();
    DELAY ready;
    ASSERT first = 1;
  END Waiter;

  Signaler: PROCESS ();
    first := 1;
    CONTINUE ready;
  END Signaler;

  Single: PROCESS ();
    DELAY one_slot PRIORITY 5;
    ASSERT second = 2;
  END Single;

  Waker: PROCESS ();
    second := 2;
    CONTINUE one_slot;
  END Waker;

  START Waiter();
  START Signaler();
  START Single();
  START Waker();
END M;
