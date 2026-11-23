M: MODULE
  SIGNAL go = (INT);
  DCL b BUFFER(1) INT;
  DCL got INT;

  Sender: PROCESS ();
    AFTER MILLISECS(1) DELAY IN
      SEND go(42);
    TIMEOUT
      ASSERT FALSE;
    END;
    AFTER MILLISECS(1) DELAY IN
      SEND b(7);
    TIMEOUT
      ASSERT FALSE;
    END;
  END Sender;

  START Sender();

  RECEIVE CASE
    (go IN got):
      ASSERT got = 42;
  ESAC;

  RECEIVE CASE
    (b IN got):
      ASSERT got = 7;
  ESAC;
END M;
