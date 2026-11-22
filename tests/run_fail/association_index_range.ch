M: MODULE
  DCL assoc ASSOCIATION;
  DCL file ACCESS (0:1) CHARS;
  DCL line CHARS;
  ASSOCIATE(assoc, "/tmp/z200c-association-range.txt");
  CONNECT(file, assoc, 0);
  READRECORD(file, 5, line);
END M;
