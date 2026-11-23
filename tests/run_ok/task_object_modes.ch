M: MODULE
  SIGNAL done;
  DCL c Counter;

  NEWMODE Counter = TASK BODY
    DCL x INT INIT := 0;

    Inc: PROC ();
      DCL tmp INT;
      tmp := x;
      tmp := tmp + 1;
      x := tmp;
    END Inc;

    Done: PROC ();
      SEND done;
    END Done;
  END Counter;

  Worker: PROCESS ();
    DCL i INT;
    DO FOR i := 1 TO 5;
      c.Inc();
    OD;
    c.Done();
  END Worker;

  Checker: PROCESS ();
    RECEIVE CASE
      (done):
    ESAC;
    RECEIVE CASE
      (done):
    ESAC;
    ASSERT c.x = 10;
  END Checker;

  START Worker();
  START Worker();
  START Checker();
END M;
