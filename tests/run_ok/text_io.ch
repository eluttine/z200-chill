M: MODULE
  DCL s CHARS INIT := "hello";
  WRITETEXT(s, 1, "i");
  ASSERT s = "hillo";
  READTEXT(s, 1, "i");
END M;
