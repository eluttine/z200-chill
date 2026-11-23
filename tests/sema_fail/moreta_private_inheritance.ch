M: MODULE
  NEWMODE Base = MODULE BODY
    DCL secret INT;
    Get: PROC () RETURNS (INT);
      RESULT secret;
    END Get;
  END Base;

  NEWMODE Derived = MODULE BODY BASED_ON Base
    Bad: PROC () RETURNS (INT);
      RESULT SELF.secret;
    END Bad;
  END Derived;
END M;
