-- Concurrent bounded-buffer pipeline.
--
-- Two producers feed a bounded work buffer, two workers transform items into a
-- results buffer, and a collector verifies delivery. A region object protects
-- statistics, signals coordinate shutdown, and a timed watchdog fails the run
-- if completion takes too long.
M: MODULE
  NEWMODE Stats = REGION BODY
    DCL produced INT INIT := 0;
    DCL consumed INT INIT := 0;

    ItemProduced: PROC ();
      produced := produced + 1;
    END ItemProduced;

    ItemConsumed: PROC ();
      consumed := consumed + 1;
    END ItemConsumed;

    GetProduced: PROC () RETURNS (INT);
      RESULT produced;
    END GetProduced;

    GetConsumed: PROC () RETURNS (INT);
      RESULT consumed;
    END GetConsumed;
  END Stats;

  DCL stats Stats;
  DCL work BUFFER(3) INT;
  DCL results BUFFER(3) INT;

  SIGNAL work_done;
  SIGNAL collector_done;
  DCL finished EVENT;

  DCL checker_instance INSTANCE;
  DCL collector_instance INSTANCE;
  DCL p1_instance INSTANCE;
  DCL p2_instance INSTANCE;
  DCL w1_instance INSTANCE;
  DCL w2_instance INSTANCE;
  DCL result_sum INT INIT := 0;

  Producer: PROCESS (base INT);
    DCL i INT;
    IF base = 1 THEN
      p1_instance := THIS;
    ELSE
      p2_instance := THIS;
    FI;

    DO FOR i := 1 TO 5;
      SEND work(base * 10 + i);
      stats.ItemProduced();
    OD;

    SEND work_done;
  END Producer;

  Worker: PROCESS (id INT);
    DCL v INT;
    IF id = 1 THEN
      w1_instance := THIS;
    ELSE
      w2_instance := THIS;
    FI;

    DO FOR EVER;
      RECEIVE CASE
        (work IN v):
          IF v = -1 THEN
            EXIT;
          FI;
          stats.ItemConsumed();
          SEND results(v * 2);
      ESAC;
    OD;
  END Worker;

  Collector: PROCESS ();
    DCL v INT;
    DCL count INT INIT := 0;
    DCL sender INSTANCE;
    DCL worker_ok BOOL INIT := FALSE;

    collector_instance := THIS;
    DO WHILE count < 10;
      RECEIVE CASE SET sender;
        (results IN v):
          worker_ok := sender = w1_instance OR sender = w2_instance;
          ASSERT worker_ok;
          result_sum := result_sum + v;
          count := count + 1;
      ESAC;
    OD;
    SEND collector_done TO checker_instance;
  END Collector;

  Checker: PROCESS ();
    checker_instance := THIS;

    RECEIVE CASE
      (work_done):
    ESAC;
    RECEIVE CASE
      (work_done):
    ESAC;

    -- All work is enqueued; one sentinel per worker.
    SEND work(-1);
    SEND work(-1);

    RECEIVE CASE
      (collector_done):
    ESAC;

    ASSERT stats.GetProduced() = 10;
    ASSERT stats.GetConsumed() = 10;
    ASSERT result_sum = 360;
    CONTINUE finished;
  END Checker;

  Watchdog: PROCESS ();
    AFTER SECS(5) DELAY IN
      DELAY CASE
        (finished):
          ASSERT TRUE;
      ESAC;
    TIMEOUT
      ASSERT FALSE;
    END;
  END Watchdog;

  START Checker();
  START Watchdog();
  START Producer(1);
  START Producer(2);
  START Worker(1);
  START Worker(2);
  START Collector();
END M;
