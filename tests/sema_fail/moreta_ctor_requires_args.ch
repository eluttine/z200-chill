M: MODULE
  NEWMODE C = MODULE BODY
    DCL value INT;
    Init: PROC (n INT) CONSTR;
      value := n;
    END Init;
  END C;

  DCL c C;
END M;
