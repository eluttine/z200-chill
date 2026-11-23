M: MODULE
  DCL assoc ASSOCIATION;
  DCL file ACCESS CHARS;
  DCL line CHARS;
  ASSOCIATE(assoc, "/tmp/z200c-access-write-only.txt");
  IF EXISTING(assoc) THEN
    DELETE(assoc);
  FI;
  CREATE(assoc);
  CONNECT(file, assoc, WRITEONLY);
  READRECORD(file, line);
END M;
