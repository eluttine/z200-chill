-- Named set modes with unnumbered and numbered elements.
M: MODULE
  NEWMODE status = SET(occupied, free);
  NEWMODE code = SET(one = 1, three = 3);

  DCL s status;
  DCL t status;
  DCL c code;

  s := occupied;
  ASSERT s = occupied;
  ASSERT s /= free;

  t := free;
  ASSERT t = free;
  ASSERT s /= t;

  c := three;
  ASSERT c = three;
  ASSERT c /= one;

  s := t;
  ASSERT s = free;
END M;
