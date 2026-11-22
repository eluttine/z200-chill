M: MODULE
  DCL e EVENT;
  P: PROCESS ();
    AFTER SECS(1) DELAY IN
      DELAY e;
      DELAY e;
    TIMEOUT
      STOP;
    END;
  END P;
  START P();
END M;
