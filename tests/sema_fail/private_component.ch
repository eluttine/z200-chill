A: MODULE
  GRANT obj;
  NEWMODE P = MODULE SPEC
    GRANT PublicProc;
    PublicProc: PROC () END;
  END P;
  NEWMODE P = MODULE BODY
    PublicProc: PROC (); END;
    PrivateProc: PROC (); END;
  END P;
  DCL obj P;
END A;

B: MODULE
  SEIZE obj;
  obj.PrivateProc();
END B;
