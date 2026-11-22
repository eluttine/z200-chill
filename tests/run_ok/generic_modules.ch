-- Generic module templates parameterized by modes.
Pair: GENERIC
  MODE T;
  MODULE
    DCL first T;
    DCL second T;
  END Pair;

PairInt: MODULE = NEW Pair (INT) END;
PairChar: MODULE = NEW Pair (CHAR) END;

Main: MODULE
  ASSERT TRUE;
END Main;
