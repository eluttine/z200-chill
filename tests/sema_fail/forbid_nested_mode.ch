M: MODULE
  A: MODULE
    NEWMODE S = STRUCT(x INT, hidden BOOL);
    GRANT S FORBID (hidden);
  END A;
  B: MODULE
    SEIZE S;
    DCL s S;
    s.hidden := TRUE;
  END B;
END M;
