M: MODULE
  DCL not_event INT;
  P: PROCESS ();
    DELAY CASE
      (not_event):
        STOP;
    ESAC;
  END P;
  START P();
END M;
