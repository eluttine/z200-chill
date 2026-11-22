-- Indexed ACCESS CHARS over line records.
M: MODULE
  DCL assoc ASSOCIATION;
  DCL file ACCESS (0:3) CHARS;
  DCL line CHARS;

  ASSOCIATE(assoc, "/tmp/z200c-association-indexed.txt");
  CREATE(assoc);
  CONNECT(file, assoc);
  WRITERECORD(file, "zero");
  WRITERECORD(file, "one");
  WRITERECORD(file, "two");
  WRITERECORD(file, "three");
  ASSERT INDEXABLE(assoc);
  DISCONNECT(file);

  CONNECT(file, assoc, 2);
  READRECORD(file, line);
  ASSERT line = "two";
  READRECORD(file, line);
  ASSERT line = "three";
  DISCONNECT(file);

  CONNECT(file, assoc, 1);
  READRECORD(file, 1, line);
  ASSERT line = "one";
  READRECORD(file, 3, line);
  ASSERT line = "three";
  WRITERECORD(file, 1, "ONE");
  READRECORD(file, 1, line);
  ASSERT line = "ONE";
  DISCONNECT(file);

  DELETE(assoc);
  DISSOCIATE(assoc);
END M;
