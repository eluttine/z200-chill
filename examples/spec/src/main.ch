Counter: SPEC MODULE
  GRANT value, Bump;
  DCL value INT;
  Bump: PROC () END;
END Counter;

Counter: MODULE
  DCL value INT INIT := 40;

  Bump: PROC ();
    value := value + 1;
  END Bump;
END Counter;

Driver: MODULE
  SEIZE value, Bump;
  ASSERT value = 40;
  Bump();
  ASSERT value = 41;
END Driver;
