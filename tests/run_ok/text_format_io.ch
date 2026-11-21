-- Format conversion, editing, and io lists for READTEXT/WRITETEXT.
M: MODULE
  DCL s CHARS;
  DCL n INT;
  DCL n2 INT;
  DCL flag BOOL;
  DCL ch CHAR;
  DCL text CHARS;

  s := "";
  WRITETEXT(s, "%C", 42);
  ASSERT s = "42";

  s := "";
  WRITETEXT(s, "%C", -15);
  ASSERT s = "-15";

  s := "";
  WRITETEXT(s, "%C5", 42);
  ASSERT s = "   42";

  s := "";
  WRITETEXT(s, "%CL5", 42);
  ASSERT s = "42   ";

  s := "";
  WRITETEXT(s, "%B", 10);
  ASSERT s = "1010";

  s := "";
  WRITETEXT(s, "%O", 15);
  ASSERT s = "17";

  s := "";
  WRITETEXT(s, "%H", 255);
  ASSERT s = "ff";

  s := "";
  WRITETEXT(s, "%C:%C", 1, 2);
  ASSERT s = "1:2";

  s := "";
  WRITETEXT(s, "%3X%C", 5);
  ASSERT s = "   5";

  s := "";
  WRITETEXT(s, "%%"); 
  ASSERT s = "%";

  s := "abc";
  WRITETEXT(s, "%>5%C", 9);
  ASSERT s = "abc  9";

  s := "xx12";
  READTEXT(s, 2, "%C", n);
  ASSERT n = 12;

  s := "1:2";
  READTEXT(s, "%C:%C", n, n2);
  ASSERT n = 1;
  ASSERT n2 = 2;

  s := "  42";
  READTEXT(s, "%C", n);
  ASSERT n = 42;

  s := "  42";
  READTEXT(s, "%C4", n);
  ASSERT n = 42;

  s := "TRUE";
  READTEXT(s, "%C", flag);
  ASSERT flag = TRUE;

  s := "Z";
  READTEXT(s, "%C", ch);
  ASSERT ch = 'Z';

  s := "hello";
  READTEXT(s, "%C", text);
  ASSERT text = "hello";

  s := "";
  WRITETEXT(s, "%2(%C:%C %)", 1, 2, 3, 4);
  ASSERT s = "1:2 3:4 ";

  s := "";
  WRITETEXT(s, 2, "%C", 7);
  ASSERT s = "  7";
END M;
