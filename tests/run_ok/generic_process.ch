M: MODULE
  SIGNAL out = (INT);

  GenericSender: GENERIC MODE T;
    PROCESS (value T);
      SEND out(value);
    END GenericSender;

  Sender: PROCESS = NEW GenericSender(INT) END;

  Checker: PROCESS ();
    RECEIVE CASE
      (out IN value):
        ASSERT value = 7;
    ESAC;
  END Checker;

  START Sender(7);
  START Checker();
END M;
