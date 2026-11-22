-- Direct Z.200 expression examples: based literals, comments, operators.
M: MODULE
  DCL a INT INIT := B'1010;          /* binary 10 */
  DCL b INT INIT := H'0F;           /* hexadecimal 15 */
  DCL c INT INIT := B'0110;         /* binary 6 */
  DCL d INT INIT := O'17;           /* octal 15 */
  DCL e INT INIT := 1_000;           /* underscores are allowed */
  DCL x INT;
  DCL s CHARS;
  DCL ch CHAR INIT := 'A';

  x := a + b * 2 - 1;
  ASSERT x = 39;
  ASSERT (a + b) * 2 = 50;
  ASSERT b MOD 4 = 3;
  ASSERT b REM 4 = 3;
  ASSERT 2 ** 5 = 32;
  ASSERT c = 6 AND d = 15 AND e = 1_000;
  ASSERT (TRUE XOR FALSE) AND NOT (FALSE XOR FALSE);
  ASSERT TRUE ANDIF FALSE OR TRUE;
  ASSERT NOT (a = b) OR a < b;

  s := "CHILL" // " " // "Z.200";
  ASSERT s = "CHILL Z.200";
  ASSERT ch = 'A';
END M;
