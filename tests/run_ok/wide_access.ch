M: MODULE
  DCL assoc ASSOCIATION;
  DCL file ACCESS WCHARS;
  DCL line WCHARS;

  ASSOCIATE(assoc, "/tmp/z200c-wideaccess.txt");
  IF EXISTING(assoc) THEN
    DELETE(assoc);
  FI;
  CREATE(assoc);

  CONNECT(file, assoc);
  WRITERECORD(file, W'"wide");
  WRITERECORD(file, W'"data");
  DISCONNECT(file);

  CONNECT(file, assoc, READONLY, FIRST);
  READRECORD(file, line);
  ASSERT line = W'"wide";
  READRECORD(file, line);
  ASSERT line = W'"data";
  DISCONNECT(file);

  DELETE(assoc);
  DISSOCIATE(assoc);
END M;
