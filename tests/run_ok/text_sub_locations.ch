-- Text record/access sub-locations and reference-backed record storage.
M: MODULE
  DCL t TEXT(20);
  DCL s CHARS(20) VARYING INIT := "hello";
  DCL r REF CHARS(20) VARYING;
  DCL assoc ASSOCIATION;
  DCL access ACCESS CHARS;

  SETTEXTRECORD(t, s);
  ASSERT t = "hello";
  s := "world";
  ASSERT t = "world";

  r := GETTEXTRECORD(t);
  r-> := "ref!";
  ASSERT t = "ref!";
  ASSERT s = "ref!";
  ASSERT GETTEXTACCESS(t) /= NULL;

  ASSOCIATE(assoc, "/tmp/z200c-text-sub-locations.txt");
  IF EXISTING(assoc) THEN
    DELETE(assoc);
  FI;
  CREATE(assoc);

  CONNECT(access, assoc, WRITEONLY);
  SETTEXTACCESS(t, access);
  ASSERT GETUSAGE(t) = WRITEONLY;
  ASSERT GETTEXTACCESS(t) /= NULL;
  WRITETEXT(t, "shared");
  DISCONNECT(t);

  DELETE(assoc);
  DISSOCIATE(assoc);
END M;
