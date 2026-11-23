-- TEXT/WTEXT actual-index attribute and its use as the default transfer cursor.
M: MODULE
  DCL t TEXT(20);
  DCL w WTEXT(10);

  SETTEXTINDEX(t, 2);
  ASSERT GETTEXTINDEX(t) = 2;
  WRITETEXT(t, "a");
  ASSERT t = "  a";
  ASSERT GETTEXTINDEX(t) = 0;

  SETTEXTINDEX(t, 0);
  READTEXT(t, "  a");
  ASSERT GETTEXTINDEX(t) = 0;

  SETTEXTINDEX(w, 1);
  WRITETEXT(w, W'"x");
  ASSERT w = W'" x";
  ASSERT GETTEXTINDEX(w) = 0;
END M;
