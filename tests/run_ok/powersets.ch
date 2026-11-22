-- Powersets over discrete scalar modes.
M: MODULE
  DCL letters POWERSET CHAR;
  DCL digits POWERSET INT;
  DCL flags POWERSET BOOL;
  DCL a POWERSET CHAR;
  DCL b POWERSET CHAR;
  DCL small POWERSET CHAR;

  letters := ['A':'C', 'x'];
  ASSERT 'A' IN letters;
  ASSERT 'B' IN letters;
  ASSERT 'C' IN letters;
  ASSERT 'x' IN letters;
  ASSERT NOT ('D' IN letters);
  ASSERT letters = ['A', 'B', 'C', 'x'];

  digits := [1, 3:5];
  ASSERT 1 IN digits;
  ASSERT 3 IN digits;
  ASSERT 5 IN digits;
  ASSERT NOT (2 IN digits);
  ASSERT digits = [1, 3, 4, 5];

  flags := [TRUE];
  ASSERT TRUE IN flags;
  ASSERT NOT (FALSE IN flags);

  a := ['A':'C'];
  b := ['B':'D'];
  ASSERT a + b = ['A':'D'];
  ASSERT a * b = ['B':'C'];
  ASSERT a - b = ['A'];

  small := ['A'];
  ASSERT small <= a;
  ASSERT a >= small;
  ASSERT NOT (a <= small);

  a := [];
  ASSERT a = [];
  ASSERT NOT ('A' IN a);
END M;
