App: MODULE
  SEIZE value;
  ASSERT value = 10;
  value := value + 5;
  ASSERT value = 15;
END App;
