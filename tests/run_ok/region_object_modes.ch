-- Region object modes: component method calls are mutually exclusive per object.
M: MODULE
  NEWMODE Counter = REGION BODY
    DCL x INT INIT := 0;

    Inc: PROC ();
      DCL tmp INT;
      tmp := x;
      tmp := tmp + 1;
      x := tmp;
    END Inc;

    Get: PROC () RETURNS (INT);
      RESULT x;
    END Get;
  END Counter;

  DCL c Counter;
  DCL done EVENT;

  Worker: PROCESS ();
    DCL i INT;
    DO FOR i := 1 TO 5;
      c.Inc();
    OD;
    CONTINUE done;
  END Worker;

  Checker: PROCESS ();
    DELAY done;
    DELAY done;
    ASSERT c.Get() = 10;
  END Checker;

  START Worker();
  START Worker();
  START Checker();
END M;
