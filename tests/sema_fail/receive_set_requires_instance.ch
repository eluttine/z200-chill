M: MODULE
  SIGNAL go;
  DCL not_instance INT;
  P: PROCESS ();
    RECEIVE CASE SET not_instance;
      (go):
        STOP;
    ESAC;
  END P;
  START P();
END M;
