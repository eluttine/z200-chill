-- Simplified association/access file I/O: line records in a text file.
M: MODULE
  DCL assoc ASSOCIATION;
  DCL file ACCESS CHARS;
  DCL line CHARS;

  ASSERT NOT ISASSOCIATED(assoc);
  ASSOCIATE(assoc, "/tmp/z200c-association-fixture.txt");
  ASSERT ISASSOCIATED(assoc);

  ASSERT NOT EXISTING(assoc);
  CREATE(assoc);
  ASSERT EXISTING(assoc);
  ASSERT READABLE(assoc);
  ASSERT WRITEABLE(assoc);
  ASSERT NOT INDEXABLE(assoc);
  ASSERT SEQUENCIBLE(assoc);
  ASSERT NOT VARIABLE(assoc);
  CONNECT(file, assoc);
  WRITERECORD(file, "one");
  WRITERECORD(file, "two");
  DISCONNECT(file);

  CONNECT(file, assoc);
  READRECORD(file, line);
  ASSERT line = "one";
  READRECORD(file, line);
  ASSERT line = "two";
  DISCONNECT(file);

  DELETE(assoc);
  ASSERT NOT EXISTING(assoc);
  DISSOCIATE(assoc);
  ASSERT NOT ISASSOCIATED(assoc);
END M;
