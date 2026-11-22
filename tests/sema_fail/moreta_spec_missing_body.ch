M: MODULE
  NEWMODE Stack = MODULE SPEC
    P: PROC () END;
  END Stack;

  NEWMODE Stack = MODULE BODY
    DCL value INT;
  END Stack;
END M;
