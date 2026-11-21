R: SPEC REGION
  GRANT x;
  DCL x INT;
END R;

R: REGION
  DCL x INT INIT := 1;
END R;

Client: MODULE
  SEIZE x;
  ASSERT x = 1;
END Client;
