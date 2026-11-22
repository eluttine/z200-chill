-- Signal definitions, SEND, RECEIVE CASE, pending signals, and ELSE.
M: MODULE
  SIGNAL go = (INT), ping, never;

  DCL got INT INIT := 0;
  DCL after INT INIT := 0;
  DCL else_value INT INIT := 0;

  Receiver: PROCESS ();
    RECEIVE CASE
      (go IN n):
        got := n;
        ASSERT got = 42;
      (ping):
        got := 1;
    ESAC;

    RECEIVE CASE
      (ping):
        after := 100;
    ESAC;

    RECEIVE CASE
      (never):
        else_value := 1;
      ELSE
        else_value := 2;
    ESAC;
    ASSERT else_value = 2;
  END Receiver;

  Sender: PROCESS ();
    SEND go(42);
    SEND ping;
  END Sender;

  START Receiver();
  START Sender();
END M;
