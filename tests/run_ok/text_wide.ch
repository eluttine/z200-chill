-- Narrow and wide text modes, including optional index mode and DYNAMIC.
M: MODULE
  DCL narrow TEXT(20);
  DCL wide WTEXT(20);
  DCL indexed TEXT(20) INT;
  DCL dynamic WTEXT(20) DYNAMIC;
  DCL n INT;

  WRITETEXT(narrow, "%C", 7);
  ASSERT narrow = "7";
  READTEXT(narrow, "%C", n);
  ASSERT n = 7;

  WRITETEXT(wide, "%C", 42);
  ASSERT wide = W'"42";
  READTEXT(wide, "%C", n);
  ASSERT n = 42;

  WRITETEXT(indexed, "%C", 3);
  ASSERT indexed = "3";

  WRITETEXT(dynamic, "%C", 9);
  ASSERT dynamic = W'"9";
END M;
