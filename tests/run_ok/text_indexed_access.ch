M: MODULE
  DCL assoc ASSOCIATION;
  DCL t TEXT(20) 0:3;
  DCL line CHARS;

  ASSOCIATE(assoc, "/tmp/z200c-textindexed.txt");
  IF EXISTING(assoc) THEN
    DELETE(assoc);
  FI;
  CREATE(assoc);

  CONNECT(t, assoc, WRITEONLY, FIRST);
  t := "";
  WRITETEXT(t, 2, "two");
  t := "";
  WRITETEXT(t, 0, "zero");
  t := "";
  WRITETEXT(t, 1, "one");
  DISCONNECT(t);

  CONNECT(t, assoc, READONLY, FIRST);
  READTEXT(t, 1, "%C", line);
  ASSERT line = "one";
  ASSERT NOT OUTOFFILE(t);
  READTEXT(t, 3, "%C", line);
  ASSERT OUTOFFILE(t);
  READTEXT(t, 0, "%C", line);
  ASSERT line = "zero";
  ASSERT NOT OUTOFFILE(t);
  DISCONNECT(t);

  DELETE(assoc);
  DISSOCIATE(assoc);
END M;
