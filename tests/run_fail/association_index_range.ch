M: MODULE
  DCL assoc ASSOCIATION;
  DCL file ACCESS (0:1) CHARS;
  DCL line CHARS;
  ASSOCIATE(assoc, "/tmp/z200c-association-range.txt");
  IF EXISTING(assoc) THEN
    DELETE(assoc);
  FI;
  CREATE(assoc);
  CONNECT(file, assoc, 0);
  READRECORD(file, 5, line);
END M;
