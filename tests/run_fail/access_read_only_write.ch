M: MODULE
  DCL assoc ASSOCIATION;
  DCL file ACCESS CHARS;
  ASSOCIATE(assoc, "/tmp/z200c-access-read-only.txt");
  IF EXISTING(assoc) THEN
    DELETE(assoc);
  FI;
  CREATE(assoc);
  CONNECT(file, assoc, READONLY);
  WRITERECORD(file, "nope");
END M;
