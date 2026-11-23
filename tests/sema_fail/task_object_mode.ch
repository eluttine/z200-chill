M: MODULE
  NEWMODE T = TASK BODY
    Get: PROC () RETURNS (INT);
      RESULT 1;
    END Get;
  END T;
  DCL t T;
END M;
