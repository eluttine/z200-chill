M: MODULE
  SIGNAL go = (INT);
  P: PROCESS ();
    SEND go();
  END P;
  START P();
END M;
