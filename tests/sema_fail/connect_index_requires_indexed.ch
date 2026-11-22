M: MODULE
  DCL assoc ASSOCIATION;
  DCL file ACCESS CHARS;
  DCL line CHARS;
  ASSOCIATE(assoc, "/tmp/z200c-indexed-sema.txt");
  CONNECT(file, assoc, 1);
END M;
