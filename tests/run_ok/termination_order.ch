-- Child processes drain before static object destructors.
M: MODULE
  DCL child_ran INT INIT := 0;

  NEWMODE P = MODULE BODY
    Fin: PROC () EXCEPTIONS (ASSERTFAIL) DESTR;
      ASSERT child_ran = 1;
    END Fin;
  END P;

  Worker: PROCESS ();
    child_ran := 1;
  END Worker;

  DCL p P;
  START Worker();
END M;
