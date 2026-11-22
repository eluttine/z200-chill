-- Process instance values, THIS, and SEND ... TO targeting.
M: MODULE
  SIGNAL go = (INT);
  DCL gate EVENT;
  DCL target INSTANCE;
  DCL got INT INIT := 0;

  Receiver: PROCESS ();
    target := THIS;
    DELAY gate;
    RECEIVE CASE
      (go IN n):
        got := n;
        ASSERT got = 7;
    ESAC;
  END Receiver;

  Sender: PROCESS ();
    SEND go(7) TO target;
    CONTINUE gate;
  END Sender;

  START Receiver();
  START Sender();
END M;
