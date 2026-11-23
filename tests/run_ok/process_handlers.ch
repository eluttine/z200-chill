-- Process handlers and STOP/CAUSE termination behavior.
M: MODULE
  DCL done EVENT;
  DCL handled INT INIT := 0;

  Worker: PROCESS ();
    CAUSE process_failure;
  END
  ON (process_failure):
    handled := 1;
    CONTINUE done;
  END;

  Checker: PROCESS ();
    DELAY done;
    ASSERT handled = 1;
  END Checker;

  START Checker();
  START Worker();
END M;
