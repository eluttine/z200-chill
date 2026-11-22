-- Process definitions and START/STOP with action-level interleaving.
M: MODULE
  DCL counter INT INIT := 0;
  DCL marker INT INIT := 0;

  Worker: PROCESS (n INT);
    DCL i INT;
    DO FOR i := 1 TO n;
      counter := counter + 1;
    OD;
    ASSERT counter = n;
  END Worker;

  Marker: PROCESS ();
    marker := 7;
    ASSERT marker = 7;
  END Marker;

  Stopper: PROCESS ();
    STOP;
    ASSERT FALSE;
  END Stopper;

  START Worker(3);
  START Marker();
  START Stopper();
END M;
