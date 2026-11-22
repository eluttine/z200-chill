-- Bit string literals and BOOLS modes.
M: MODULE
  DCL b BOOLS INIT := B'1010';
  DCL fixed BOOLS(4) INIT := B'1010';
  DCL varying BOOLS(4) VARYING INIT := B'10';
  DCL o BOOLS INIT := O'53';
  DCL h BOOLS INIT := H'f';
  DCL arr ARRAY (1:2) BOOLS(2);

  ASSERT b = B'1010';
  ASSERT fixed = b;
  ASSERT o = B'101011';
  ASSERT h = B'1111';
  ASSERT B'' = B'';
  ASSERT NOT B'1010' = B'0101';
  ASSERT (B'1010' AND B'1100') = B'1000';
  ASSERT (B'1010' OR B'0101') = B'1111';
  ASSERT (B'1010' XOR B'1100') = B'0110';
  ASSERT b // B'11' = B'101011';
  ASSERT B'1' < B'10';

  varying := B'1111';
  ASSERT varying = B'1111';

  arr(1) := B'00';
  arr(2) := B'11';
  ASSERT arr = [B'00', B'11'];
END M;
