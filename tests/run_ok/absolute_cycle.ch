M: MODULE
  DCL never EVENT;
  DCL x INT INIT := 0;
  DCL count INT INIT := 0;

  Absolute: PROCESS ();
    AT ABSTIME() IN
      DELAY never;
    TIMEOUT
      x := 1;
    END;
    ASSERT x = 1;
  END Absolute;

  Cyclic: PROCESS ();
    CYCLE MILLISECS(1) IN
      count := count + 1;
      IF count = 2 THEN
        EXIT;
      FI;
    END;
    ASSERT count = 2;
  END Cyclic;

  START Absolute();
  START Cyclic();
END M;
