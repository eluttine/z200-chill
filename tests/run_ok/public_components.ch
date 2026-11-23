-- Public components are accessible across modules.
A: MODULE
  GRANT obj;
  NEWMODE P = MODULE SPEC
    GRANT PublicProc;
    PublicProc: PROC () END;
    HiddenProc: PROC () END;
  END P;
  NEWMODE P = MODULE BODY
    PublicProc: PROC ();
    END PublicProc;
    HiddenProc: PROC ();
    END HiddenProc;
  END P;
  DCL obj P;
END A;

B: MODULE
  SEIZE obj;
  obj.PublicProc();
END B;
