M: MODULE
  DCL assoc ASSOCIATION;
  DCL file ACCESS CHARS;
  DCL line CHARS;
  ASSOCIATE(assoc, "/tmp/z200c-association-not-connected.txt");
  READRECORD(file, line);
END M;
