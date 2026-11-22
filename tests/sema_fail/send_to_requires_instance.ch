M: MODULE
  SIGNAL go;
  P: PROCESS ();
    SEND go TO 5;
  END P;
  START P();
END M;
