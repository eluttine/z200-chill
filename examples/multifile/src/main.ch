Main: MODULE
  SEIZE value;
  ASSERT value = 41;
  value := value + 1;
  ASSERT value = 42;
END Main;
