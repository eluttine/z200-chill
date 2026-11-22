-- Relative timing actions and TIMEOUT handlers.
M: MODULE
  DCL never EVENT;
  SIGNAL never_signal;
  DCL x INT INIT := 0;
  DCL y INT INIT := 0;
  DCL z INT INIT := 0;

  TimedEvent: PROCESS ();
    AFTER MILLISECS(5) DELAY IN
      DELAY never;
    TIMEOUT
      x := 1;
    END;
    ASSERT x = 1;
  END TimedEvent;

  TimedReceive: PROCESS ();
    AFTER MILLISECS(5) DELAY IN
      RECEIVE CASE
        (never_signal):
          x := 99;
      ESAC;
    TIMEOUT
      y := 2;
    END;
    ASSERT y = 2;
  END TimedReceive;

  NoTimeout: PROCESS ();
    AFTER SECS(1) IN
      z := 3;
    TIMEOUT
      z := 4;
    END;
    ASSERT z = 3;
  END NoTimeout;

  START TimedEvent();
  START TimedReceive();
  START NoTimeout();
END M;
