M: MODULE
  DCL t TEXT(3);
  DCL r REF CHARS(3) VARYING;
  r := GETTEXTRECORD(t);
  r-> := "toolong";
END M;
