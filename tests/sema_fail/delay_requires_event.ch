M: MODULE
  DCL not_event INT;
  P: PROCESS ();
    DELAY not_event;
  END P;
  START P();
END M;
