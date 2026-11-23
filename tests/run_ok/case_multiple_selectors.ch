-- CASE with multiple selectors and per-selector label lists.
M: MODULE
  DCL x INT INIT := 1;
  DCL y INT INIT := 2;
  DCL r INT INIT := 0;

  CASE x, y OF
    (1), (2): r := 12;
    (1), (3): r := 13;
    (ELSE), (ELSE): r := 99;
  ESAC;
  ASSERT r = 12;

  CASE x, y OF
    (2), (1): r := 21;
    (*), (*): r := 22;
  ESAC;
  ASSERT r = 22;

  CASE x, y OF
    (7), (7): r := 77;
    (ELSE), (*): r := 30;
  ESAC;
  ASSERT r = 30;
END M;
