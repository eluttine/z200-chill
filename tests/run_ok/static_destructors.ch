M: MODULE
  DCL hits INT INIT := 0;
  NEWMODE P = MODULE BODY
    Fin: PROC () DESTR;
      hits := hits + 1;
    END Fin;
  END P;

  Run: PROC ();
    DCL q P;
  END Run;

  BEGIN
    DCL p P;
  END;
  ASSERT hits = 1;

  Run();
  ASSERT hits = 2;
END M;
