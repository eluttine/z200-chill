-- CASE with BOOL and CHAR selectors, literal labels, and character ranges.
M: MODULE
  DCL c CHAR INIT := 'm';
  DCL b BOOL INIT := TRUE;
  DCL r INT INIT := 0;

  CASE c OF
    ('a':'l'): r := 1;
    ('m'): r := 2;
    (ELSE): r := 3;
  ESAC;
  ASSERT r = 2;

  CASE b OF
    (TRUE): r := 4;
    (FALSE): r := 5;
  ESAC;
  ASSERT r = 4;

  CASE c, b OF
    ('a'), (TRUE): r := 6;
    ('m'), (TRUE): r := 7;
  ESAC;
  ASSERT r = 7;
END M;
