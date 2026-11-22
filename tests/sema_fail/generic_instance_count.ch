M: MODULE
  Identity: GENERIC
    MODE T;
    PROC (x T) RETURNS (T);
      RETURN x;
    END Identity;
  Bad: PROC = NEW Identity (INT, CHAR) END;
END M;
