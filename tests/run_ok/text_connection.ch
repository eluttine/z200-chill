M: MODULE
  DCL assoc ASSOCIATION;
  DCL t TEXT(20);

  ASSOCIATE(assoc, "/tmp/z200c-textconn.txt");
  IF EXISTING(assoc) THEN
    DELETE(assoc);
  FI;
  CREATE(assoc);

  CONNECT(t, assoc, WRITEONLY);
  ASSERT GETUSAGE(t) = WRITEONLY;
  WRITETEXT(t, "hello");
  WRITETEXT(t, "world");
  DISCONNECT(t);

  CONNECT(t, assoc, READONLY, FIRST);
  READTEXT(t, "hello");
  ASSERT t = "hello";
  ASSERT NOT OUTOFFILE(t);
  READTEXT(t, "world");
  ASSERT t = "world";
  READTEXT(t, "x");
  ASSERT OUTOFFILE(t);
  DISCONNECT(t);

  DELETE(assoc);
  DISSOCIATE(assoc);
END M;
