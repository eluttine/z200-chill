-- Generic procedure templates parameterized by modes.
M: MODULE
  Identity: GENERIC
    MODE T;
    PROC (x T) RETURNS (T);
      RETURN x;
    END Identity;

  IdentityInt: PROC = NEW Identity (INT) END;
  IdentityChar: PROC = NEW Identity (CHAR) END;

  Copying: GENERIC
    MODE T;
    PROC (x T) RETURNS (T);
      DCL y T;
      y := x;
      RETURN y;
    END Copying;

  CopyInt: PROC = NEW Copying (INT) END;

  ASSERT IdentityInt(4) = 4;
  ASSERT IdentityChar('A') = 'A';
  ASSERT CopyInt(9) = 9;
END M;
