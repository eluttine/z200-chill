M: MODULE
  DCL x INT INIT := 1;
  DCL y INT INIT := 2;

  CASE x, y OF
    (1): x := 2;
  ESAC;
END M;
