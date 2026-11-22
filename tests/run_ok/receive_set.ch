M: MODULE
  SIGNAL go = (INT);
  DCL b BUFFER(1) INT;
  DCL signal_sender INSTANCE;
  DCL buffer_sender INSTANCE;
  DCL got INT INIT := 0;
  DCL got_buf INT INIT := 0;

  Receiver: PROCESS ();
    DCL s INSTANCE;
    DCL bs INSTANCE;
    RECEIVE CASE SET s;
      (go IN n):
        got := n;
        ASSERT s = signal_sender;
        ASSERT n = 3;
    ESAC;
    RECEIVE CASE SET bs;
      (b IN v):
        got_buf := v;
        ASSERT bs = buffer_sender;
        ASSERT v = 4;
    ESAC;
  END Receiver;

  Sender: PROCESS ();
    signal_sender := THIS;
    SEND go(3);
    buffer_sender := THIS;
    SEND b(4);
  END Sender;

  START Receiver();
  START Sender();
END M;
