-- Fixed and bounded varying character string modes.
M: MODULE
  DCL fixed CHARS(5) INIT := "hello";
  DCL varying CHARS(3) VARYING;
  DCL unbounded CHARS INIT := "world";
  DCL f ARRAY (1:2) CHARS(2);
  DCL s CHARS;

  ASSERT fixed = "hello";
  varying := "";
  ASSERT varying = "";
  varying := "abc";
  ASSERT varying = "abc";
  ASSERT fixed // varying = "helloabc";

  f(1) := "ab";
  f(2) := "cd";
  ASSERT f(1) = "ab";
  ASSERT f(2) = "cd";
  ASSERT f = ["ab", "cd"];

  s := fixed;
  ASSERT s = "hello";
  s := varying;
  ASSERT s = "abc";
  s := unbounded;
  ASSERT s = "world";
END M;
