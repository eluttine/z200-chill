M: MODULE
  DCL assoc ASSOCIATION;
  DCL t TEXT(10);
  ASSOCIATE(assoc, "/tmp/z200c-text-read-only.txt");
  IF EXISTING(assoc) THEN
    DELETE(assoc);
  FI;
  CREATE(assoc);
  CONNECT(t, assoc, READONLY);
  WRITETEXT(t, "nope");
END M;
