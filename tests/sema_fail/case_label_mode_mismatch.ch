M: MODULE
  DCL b BOOL INIT := TRUE;
  CASE b OF
    ('x'): b := FALSE;
  ESAC;
END M;
