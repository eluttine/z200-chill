-- Equal-priority signal and buffer messages are FIFO.
M: MODULE
  SIGNAL seq = (INT);
  DCL gate EVENT;
  DCL b BUFFER(2) INT;
  DCL first_signal INT INIT := 0;
  DCL second_signal INT INIT := 0;
  DCL first_buffer INT INIT := 0;
  DCL second_buffer INT INIT := 0;

  Receiver: PROCESS ();
    DELAY gate;
    RECEIVE CASE
      (seq IN n):
        first_signal := n;
    ESAC;
    RECEIVE CASE
      (seq IN n):
        second_signal := n;
    ESAC;
    ASSERT first_signal = 1;
    ASSERT second_signal = 2;

    RECEIVE CASE
      (b IN n):
        first_buffer := n;
    ESAC;
    RECEIVE CASE
      (b IN n):
        second_buffer := n;
    ESAC;
    ASSERT first_buffer = 10;
    ASSERT second_buffer = 20;
  END Receiver;

  Sender: PROCESS ();
    SEND seq(1);
    SEND seq(2);
    SEND b(10);
    SEND b(20);
    CONTINUE gate;
  END Sender;

  START Receiver();
  START Sender();
END M;
