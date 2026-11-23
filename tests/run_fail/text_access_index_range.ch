M: MODULE
  DCL assoc ASSOCIATION;
  DCL t TEXT(10) 0:1;
  DCL line CHARS;
  ASSOCIATE(assoc, "/tmp/z200c-text-index-range.txt");
  IF EXISTING(assoc) THEN
    DELETE(assoc);
  FI;
  CREATE(assoc);
  CONNECT(t, assoc, READONLY, FIRST);
  READTEXT(t, 5, "%C", line);
END M;
