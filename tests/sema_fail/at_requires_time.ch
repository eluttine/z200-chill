M: MODULE
  P: PROCESS ();
    AT SECS(1) IN
      STOP;
    TIMEOUT
      STOP;
    END;
  END P;
  START P();
END M;
