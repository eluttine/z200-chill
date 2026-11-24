-- Static task objects stop their workers at program shutdown.
M: MODULE
  NEWMODE T = TASK BODY
    DCL value INT INIT := 0;
    Set: PROC (n INT);
      value := n;
    END Set;
  END T;

  DCL task T;
  ASSERT task.value = 0;
END M;
