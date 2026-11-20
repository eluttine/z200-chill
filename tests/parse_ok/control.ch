Control: MODULE
  DCL sum INT INIT := 0;

  DO FOR i := 1 TO 3;
    sum := sum + i;
  OD;

  CASE sum OF
    (6): ;
    (ELSE): ;
  ESAC;
END Control;
