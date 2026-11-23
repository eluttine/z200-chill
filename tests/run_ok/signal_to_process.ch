M: MODULE
  SIGNAL go = (INT) TO Worker;
  DCL wtarget INSTANCE;
  DCL got INT INIT := 0;

  Worker: PROCESS ();
    DCL s INSTANCE;
    wtarget := THIS;
    RECEIVE CASE SET s;
      (go IN n):
        got := n;
        ASSERT n = 8;
    ESAC;
  END Worker;

  Sender: PROCESS ();
    SEND go(8) TO wtarget;
  END Sender;

  START Worker();
  START Sender();
END M;
