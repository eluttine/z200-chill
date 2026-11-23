M: MODULE
  DCL assoc ASSOCIATION;
  DCL t TEXT(20);
  DCL r REF ACCESS CHARS;

  ASSOCIATE(assoc, "/tmp/z200c-refaccess.txt");
  IF EXISTING(assoc) THEN
    DELETE(assoc);
  FI;
  CREATE(assoc);

  CONNECT(t, assoc, WRITEONLY);
  r := GETTEXTACCESS(t);
  ASSERT GETUSAGE(r->) = WRITEONLY;
  ASSERT NOT OUTOFFILE(r->);
  WRITETEXT(t, "via ref");
  DISCONNECT(r->);
  CONNECT(r->, assoc, READONLY, FIRST);
  ASSERT GETUSAGE(r->) = READONLY;
  READTEXT(t, "via ref");
  ASSERT t = "via ref";
  DISCONNECT(t);

  DELETE(assoc);
  DISSOCIATE(assoc);
END M;
