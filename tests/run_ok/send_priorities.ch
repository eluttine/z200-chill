M: MODULE
  SIGNAL go = (INT);
  DCL gate EVENT;
  DCL b BUFFER(2) INT;
  DCL got INT INIT := 0;
  DCL got_buf INT INIT := 0;

  Receiver: PROCESS ();
    DELAY gate;
    RECEIVE CASE
      (go IN n):
        got := n;
    ESAC;
    ASSERT got = 2;
    CONTINUE gate;
    DELAY gate;
    RECEIVE CASE
      (b IN n):
        got_buf := n;
    ESAC;
    ASSERT got_buf = 20;
  END Receiver;

  Sender: PROCESS ();
    SEND go(1) PRIORITY 1;
    SEND go(2) PRIORITY 5;
    SEND b(10) PRIORITY 1;
    SEND b(20) PRIORITY 5;
    CONTINUE gate;
    DELAY gate;
    CONTINUE gate;
  END Sender;

  START Receiver();
  START Sender();
END M;
