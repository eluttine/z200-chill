-- Module mode specification paired with a body.
M: MODULE
  NEWMODE Stack = MODULE SPEC
    DCL value INT;
    Inc: PROC (n INT) RETURNS (INT) END;
  END Stack;

  NEWMODE Stack = MODULE BODY
    DCL value INT;
    Inc: PROC (n INT) RETURNS (INT);
      value := value + n;
      RETURN value;
    END Inc;
  END Stack;

  DCL s Stack;
  s.value := 1;
  ASSERT s.Inc(2) = 3;
  ASSERT s.value = 3;
END M;
