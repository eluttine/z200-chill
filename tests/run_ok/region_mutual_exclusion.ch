R: SPEC REGION
  GRANT Inc, Get;
  Inc: PROC () END;
  Get: PROC () RETURNS (INT) END;
END R;

R: REGION
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
END R;

M: MODULE
  SEIZE Inc, Get;
  SIGNAL done;

  Worker: PROCESS ();
    DCL i INT;
    DO FOR i := 1 TO 5;
      Inc();
    OD;
    SEND done;
  END Worker;

  Checker: PROCESS ();
    RECEIVE CASE
      (done):
        RECEIVE CASE
          (done):
            ASSERT Get() = 10;
        ESAC;
    ESAC;
  END Checker;

  START Checker();
  START Worker();
  START Worker();
END M;
