M: MODULE
  P: PROCESS ();
    AFTER 5 IN
      STOP;
    TIMEOUT
      STOP;
    END;
  END P;
  START P();
END M;
