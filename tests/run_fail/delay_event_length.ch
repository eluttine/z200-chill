M: MODULE
  DCL one_slot EVENT(1);

  First: PROCESS ();
    DELAY one_slot;
  END First;

  Second: PROCESS ();
    DELAY one_slot;
  END Second;

  START First();
  START Second();
END M;
