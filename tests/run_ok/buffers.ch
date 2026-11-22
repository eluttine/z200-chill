-- BUFFER modes, buffered send/receive, capacity, and receive timeout.
M: MODULE
  DCL b BUFFER(1) INT;
  DCL empty BUFFER(2) INT;
  DCL total INT INIT := 0;
  DCL timed_out INT INIT := 0;

  Producer: PROCESS ();
    SEND b(1);
    SEND b(2);
    SEND b(3);
  END Producer;

  Consumer: PROCESS ();
    DCL v INT;
    RECEIVE CASE
      (b IN v):
        total := total + v;
    ESAC;
    RECEIVE CASE
      (b IN v):
        total := total + v;
    ESAC;
    RECEIVE CASE
      (b IN v):
        total := total + v;
    ESAC;
    ASSERT total = 6;
  END Consumer;

  TimedReceiver: PROCESS ();
    AFTER MILLISECS(5) DELAY IN
      RECEIVE CASE
        (empty IN v):
          timed_out := 0;
      ESAC;
    TIMEOUT
      timed_out := 1;
    END;
    ASSERT timed_out = 1;
  END TimedReceiver;

  START Producer();
  START Consumer();
  START TimedReceiver();
END M;
