-- Wide character and wide character string modes.
M: MODULE
  DCL w WCHAR INIT := W'Ω';
  DCL lower WCHAR INIT := w'λ';
  DCL s WCHARS INIT := W'"héllo";
  DCL quoted WCHARS INIT := W'"a""b";
  DCL fixed WCHARS(5) INIT := W'"hello";
  DCL varying WCHARS(5) VARYING INIT := W'"hi";
  DCL wide_array ARRAY (1:2) WCHARS;
  DCL letters POWERSET WCHAR;

  ASSERT w = W'Ω';
  ASSERT lower = W'λ';
  ASSERT W'A' < W'B';
  ASSERT s = W'"héllo";
  ASSERT quoted = W'"a""b";
  ASSERT fixed = W'"hello";
  ASSERT varying // W'"!" = W'"hi!";
  letters := [W'A':W'Z'];
  ASSERT W'Z' IN letters;

  wide_array(1) := W'"one";
  wide_array(2) := W'"two";
  ASSERT wide_array(1) = W'"one";
  ASSERT wide_array = [W'"one", W'"two"];

  letters := [W'a':W'c'];
  ASSERT W'b' IN letters;
  ASSERT NOT (W'd' IN letters);
END M;
