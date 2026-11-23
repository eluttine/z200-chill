M: MODULE
  DCL assoc ASSOCIATION;
  DCL file ACCESS (0:3) CHARS;
  DCL line CHARS;
  DCL aref REF ASSOCIATION;

  ASSOCIATE(assoc, "/tmp/z200c-access-attr.txt");
  CREATE(assoc);
  CONNECT(file, assoc);
  ASSERT GETUSAGE(file) = READWRITE;
  ASSERT NOT OUTOFFILE(file);
  WRITERECORD(file, "zero");
  WRITERECORD(file, "one");
  WRITERECORD(file, "two");
  DISCONNECT(file);

  CONNECT(file, assoc, READONLY, FIRST);
  ASSERT GETUSAGE(file) = READONLY;
  READRECORD(file, line);
  ASSERT line = "zero";
  ASSERT NOT OUTOFFILE(file);
  READRECORD(file, 3, line);
  ASSERT OUTOFFILE(file);
  DISCONNECT(file);

  CONNECT(file, assoc, READWRITE, FIRST, 1);
  aref := GETASSOCIATION(file);
  ASSERT aref /= NULL;
  READRECORD(file, line);
  ASSERT line = "one";
  DISCONNECT(file);

  CONNECT(file, assoc, WRITEONLY, LAST);
  ASSERT GETUSAGE(file) = WRITEONLY;
  WRITERECORD(file, "three");
  DISCONNECT(file);

  DELETE(assoc);
  DISSOCIATE(assoc);
END M;
