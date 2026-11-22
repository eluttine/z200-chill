M: MODULE
  SIGNAL never;
  P: PROCESS ();
    RECEIVE CASE
      (never):
        STOP;
    ESAC;
  END P;
  START P();
END M;
