use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Identifier(String),
    Integer { value: i128, base: u32 },
    Bool(bool),
    Char(char),
    String(String),
    Keyword(Keyword),

    Semicolon,
    Comma,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LParenColon,
    RParenColon,
    Colon,
    Dot,
    Assign,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    StarStar,
    Slash,
    Concat,
    Arrow,
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,
    ConcatAssign,
    Hash,
    Bang,
    Diamond,
}

/// Reserved simple name strings from Z.200 Appendix III.1.
macro_rules! define_keywords {
    ($($variant:ident => $text:literal),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Keyword {
            $($variant),+
        }

        impl Keyword {
            pub fn from_name(text: &str) -> Option<Self> {
                match text {
                    $($text => Some(Self::$variant),)+
                    _ => None,
                }
            }

            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }
        }
    };
}

define_keywords! {
    Abstract => "ABSTRACT",
    Access => "ACCESS",
    After => "AFTER",
    All => "ALL",
    And => "AND",
    AndIf => "ANDIF",
    Any => "ANY",
    AnyAssign => "ANY_ASSIGN",
    AnyDiscrete => "ANY_DISCRETE",
    AnyInt => "ANY_INT",
    AnyReal => "ANY_REAL",
    Array => "ARRAY",
    Assignable => "ASSIGNABLE",
    Assert => "ASSERT",
    At => "AT",
    BasedOn => "BASED_ON",
    Begin => "BEGIN",
    Bin => "BIN",
    Body => "BODY",
    Bools => "BOOLS",
    Buffer => "BUFFER",
    By => "BY",
    Case => "CASE",
    Cause => "CAUSE",
    Chars => "CHARS",
    Constr => "CONSTR",
    Context => "CONTEXT",
    Continue => "CONTINUE",
    Cycle => "CYCLE",
    Dcl => "DCL",
    Delay => "DELAY",
    Destr => "DESTR",
    Do => "DO",
    Down => "DOWN",
    Dynamic => "DYNAMIC",
    Else => "ELSE",
    Elsif => "ELSIF",
    End => "END",
    Esac => "ESAC",
    Event => "EVENT",
    Ever => "EVER",
    Exceptions => "EXCEPTIONS",
    Exit => "EXIT",
    Fi => "FI",
    Final => "FINAL",
    For => "FOR",
    Forbid => "FORBID",
    General => "GENERAL",
    Generic => "GENERIC",
    Goto => "GOTO",
    Grant => "GRANT",
    If => "IF",
    Implements => "IMPLEMENTS",
    In => "IN",
    Incomplete => "INCOMPLETE",
    Init => "INIT",
    Inline => "INLINE",
    InOut => "INOUT",
    Interface => "INTERFACE",
    Invariant => "INVARIANT",
    Loc => "LOC",
    Mod => "MOD",
    Mode => "MODE",
    Module => "MODULE",
    New => "NEW",
    NewMode => "NEWMODE",
    NonRef => "NONREF",
    NotAssignable => "NOT_ASSIGNABLE",
    Not => "NOT",
    Od => "OD",
    Of => "OF",
    On => "ON",
    Or => "OR",
    OrIf => "ORIF",
    Out => "OUT",
    Pack => "PACK",
    Pos => "POS",
    Post => "POST",
    Powerset => "POWERSET",
    Pre => "PRE",
    Prefixed => "PREFIXED",
    Priority => "PRIORITY",
    Proc => "PROC",
    Process => "PROCESS",
    Range => "RANGE",
    Read => "READ",
    Receive => "RECEIVE",
    Ref => "REF",
    Region => "REGION",
    Reimplement => "REIMPLEMENT",
    Rem => "REM",
    Remote => "REMOTE",
    Result => "RESULT",
    Return => "RETURN",
    Returns => "RETURNS",
    Row => "ROW",
    Seize => "SEIZE",
    SelfKw => "SELF",
    Send => "SEND",
    Set => "SET",
    Signal => "SIGNAL",
    Simple => "SIMPLE",
    Spec => "SPEC",
    Start => "START",
    Static => "STATIC",
    Step => "STEP",
    Stop => "STOP",
    Struct => "STRUCT",
    Syn => "SYN",
    SynMode => "SYNMODE",
    Task => "TASK",
    Text => "TEXT",
    Then => "THEN",
    This => "THIS",
    Timeout => "TIMEOUT",
    To => "TO",
    Up => "UP",
    Varying => "VARYING",
    WChars => "WCHARS",
    While => "WHILE",
    With => "WITH",
    WText => "WTEXT",
    Xor => "XOR",
}

impl fmt::Display for Keyword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identifier(name) => write!(f, "identifier `{name}`"),
            Self::Integer { value, base } => write!(f, "integer `{value}` (base {base})"),
            Self::Bool(value) => write!(f, "`{value}`"),
            Self::Char(value) => write!(f, "character literal `{value}`"),
            Self::String(_) => f.write_str("string literal"),
            Self::Keyword(keyword) => write!(f, "`{keyword}`"),
            Self::Semicolon => f.write_str("`;`"),
            Self::Comma => f.write_str("`,`"),
            Self::LParen => f.write_str("`(`"),
            Self::RParen => f.write_str("`)`"),
            Self::LBracket => f.write_str("`[`"),
            Self::RBracket => f.write_str("`]`"),
            Self::LParenColon => f.write_str("`(:`"),
            Self::RParenColon => f.write_str("`:)`"),
            Self::Colon => f.write_str("`:`"),
            Self::Dot => f.write_str("`.`"),
            Self::Assign => f.write_str("`:=`"),
            Self::Eq => f.write_str("`=`"),
            Self::Ne => f.write_str("`/=`"),
            Self::Lt => f.write_str("`<`"),
            Self::Le => f.write_str("`<=`"),
            Self::Gt => f.write_str("`>`"),
            Self::Ge => f.write_str("`>=`"),
            Self::Plus => f.write_str("`+`"),
            Self::Minus => f.write_str("`-`"),
            Self::Star => f.write_str("`*`"),
            Self::StarStar => f.write_str("`**`"),
            Self::Slash => f.write_str("`/`"),
            Self::Concat => f.write_str("`//`"),
            Self::Arrow => f.write_str("`->`"),
            Self::PlusAssign => f.write_str("`+:=`"),
            Self::MinusAssign => f.write_str("`-:=`"),
            Self::StarAssign => f.write_str("`*:=`"),
            Self::SlashAssign => f.write_str("`/:=`"),
            Self::ConcatAssign => f.write_str("`//:=`"),
            Self::Hash => f.write_str("`#`"),
            Self::Bang => f.write_str("`!`"),
            Self::Diamond => f.write_str("`<>`"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub offset: usize,
    pub message: String,
}

impl Error {
    pub fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// Tokenize a complete source file, collecting every lexical error.
pub fn tokenize(source: &str) -> Result<Vec<Token>, Vec<Error>> {
    let mut lexer = Lexer::new(source);
    let mut tokens = Vec::new();
    let mut errors = Vec::new();

    loop {
        let before = lexer.pos;
        match lexer.next_token() {
            Ok(Some(token)) => tokens.push(token),
            Ok(None) => break,
            Err(error) => {
                errors.push(error);
                // Guarantee progress even if the error was raised without consuming.
                if lexer.pos == before {
                    lexer.bump();
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(tokens)
    } else {
        Err(errors)
    }
}

/// Compute a 1-based line and column for a byte offset.
pub fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;
    for (index, ch) in source.char_indices() {
        if index >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

struct Lexer<'a> {
    source: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, pos: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.source[self.pos..].chars().next()
    }

    fn peek_second(&self) -> Option<char> {
        let mut chars = self.source[self.pos..].chars();
        chars.next();
        chars.next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn starts_with(&self, text: &str) -> bool {
        self.source[self.pos..].starts_with(text)
    }

    fn next_token(&mut self) -> Result<Option<Token>, Error> {
        self.skip_trivia()?;

        let Some(ch) = self.peek() else {
            return Ok(None);
        };
        let start = self.pos;

        let kind = if is_based_prefix(ch) && self.peek_second() == Some('\'') {
            self.lex_based_integer()?
        } else if is_identifier_start(ch) {
            self.lex_identifier()
        } else if ch.is_ascii_digit() {
            self.lex_decimal_integer()?
        } else if ch == '\'' {
            self.lex_character()?
        } else if ch == '"' {
            self.lex_string()?
        } else {
            self.lex_operator()?
        };

        Ok(Some(Token {
            kind,
            span: Span {
                start,
                end: self.pos,
            },
        }))
    }

    fn skip_trivia(&mut self) -> Result<(), Error> {
        loop {
            while matches!(self.peek(), Some(ch) if ch.is_whitespace()) {
                self.bump();
            }

            if self.starts_with("/*") {
                let start = self.pos;
                self.pos += 2;
                match self.source[self.pos..].find("*/") {
                    Some(index) => self.pos += index + 2,
                    None => {
                        self.pos = self.source.len();
                        return Err(Error::new(start, "unterminated bracketed comment"));
                    }
                }
            } else if self.starts_with("--") {
                while let Some(ch) = self.bump() {
                    if ch == '\n' {
                        break;
                    }
                }
            } else {
                return Ok(());
            }
        }
    }

    fn lex_identifier(&mut self) -> TokenKind {
        let start = self.pos;
        self.bump();
        while matches!(self.peek(), Some(ch) if ch.is_ascii_alphanumeric() || ch == '_') {
            self.bump();
        }
        let text = &self.source[start..self.pos];

        match text {
            "TRUE" => TokenKind::Bool(true),
            "FALSE" => TokenKind::Bool(false),
            _ => match Keyword::from_name(text) {
                Some(keyword) => TokenKind::Keyword(keyword),
                None => TokenKind::Identifier(text.to_owned()),
            },
        }
    }

    fn lex_decimal_integer(&mut self) -> Result<TokenKind, Error> {
        let start = self.pos;
        self.bump();
        while matches!(self.peek(), Some(ch) if ch.is_ascii_digit() || ch == '_') {
            self.bump();
        }
        let text = &self.source[start..self.pos];
        parse_integer(text, 10, start)
    }

    fn lex_based_integer(&mut self) -> Result<TokenKind, Error> {
        let start = self.pos;
        let prefix = self.bump().expect("checked by caller");
        let base = match prefix.to_ascii_uppercase() {
            'B' => 2,
            'O' => 8,
            'D' => 10,
            'H' => 16,
            _ => unreachable!("checked by caller"),
        };
        self.bump(); // opening apostrophe

        let digits_start = self.pos;
        loop {
            match self.peek() {
                Some('\'') => break,
                Some('\n') | None => {
                    return Err(Error::new(start, "unterminated based integer literal"));
                }
                Some(_) => {
                    self.bump();
                }
            }
        }
        let digits = &self.source[digits_start..self.pos];
        self.bump(); // closing apostrophe

        if digits.chars().all(|ch| ch == '_') {
            return Err(Error::new(
                start,
                "based integer literal must contain at least one digit",
            ));
        }

        parse_integer(digits, base, start)
    }

    fn lex_character(&mut self) -> Result<TokenKind, Error> {
        let start = self.pos;
        self.bump(); // opening apostrophe

        if self.peek() == Some('^') {
            return Err(Error::new(
                start,
                "character literal control sequences are not supported yet",
            ));
        }

        let Some(value) = self.bump() else {
            return Err(Error::new(start, "unterminated character literal"));
        };

        if self.peek() != Some('\'') {
            return Err(Error::new(
                start,
                "character literal must contain exactly one character",
            ));
        }
        self.bump(); // closing apostrophe

        Ok(TokenKind::Char(value))
    }

    fn lex_string(&mut self) -> Result<TokenKind, Error> {
        let start = self.pos;
        self.bump(); // opening quote
        let mut value = String::new();

        loop {
            let Some(ch) = self.peek() else {
                return Err(Error::new(start, "unterminated character string literal"));
            };

            if ch == '"' {
                self.bump();
                if self.peek() == Some('"') {
                    self.bump();
                    value.push('"');
                    continue;
                }
                return Ok(TokenKind::String(value));
            }

            if ch == '^' {
                return Err(Error::new(
                    self.pos,
                    "character string control sequences are not supported yet",
                ));
            }

            self.bump();
            value.push(ch);
        }
    }

    fn lex_operator(&mut self) -> Result<TokenKind, Error> {
        let rest = &self.source[self.pos..];

        // Longest match first.
        if rest.starts_with("//:=") {
            self.pos += 4;
            return Ok(TokenKind::ConcatAssign);
        }
        for (text, kind) in [
            ("+:=", TokenKind::PlusAssign),
            ("-:=", TokenKind::MinusAssign),
            ("*:=", TokenKind::StarAssign),
            ("/:=", TokenKind::SlashAssign),
        ] {
            if rest.starts_with(text) {
                self.pos += text.len();
                return Ok(kind);
            }
        }
        for (text, kind) in [
            (":=", TokenKind::Assign),
            ("<=", TokenKind::Le),
            (">=", TokenKind::Ge),
            ("/=", TokenKind::Ne),
            ("<>", TokenKind::Diamond),
            ("->", TokenKind::Arrow),
            ("(:", TokenKind::LParenColon),
            (":)", TokenKind::RParenColon),
            ("//", TokenKind::Concat),
            ("**", TokenKind::StarStar),
        ] {
            if rest.starts_with(text) {
                self.pos += text.len();
                return Ok(kind);
            }
        }

        let ch = self.peek().expect("checked by caller");
        self.pos += ch.len_utf8();
        match ch {
            ';' => Ok(TokenKind::Semicolon),
            ',' => Ok(TokenKind::Comma),
            '(' => Ok(TokenKind::LParen),
            ')' => Ok(TokenKind::RParen),
            '[' => Ok(TokenKind::LBracket),
            ']' => Ok(TokenKind::RBracket),
            ':' => Ok(TokenKind::Colon),
            '.' => Ok(TokenKind::Dot),
            '=' => Ok(TokenKind::Eq),
            '<' => Ok(TokenKind::Lt),
            '>' => Ok(TokenKind::Gt),
            '+' => Ok(TokenKind::Plus),
            '-' => Ok(TokenKind::Minus),
            '*' => Ok(TokenKind::Star),
            '/' => Ok(TokenKind::Slash),
            '#' => Ok(TokenKind::Hash),
            '!' => Ok(TokenKind::Bang),
            other => Err(Error::new(
                self.pos - other.len_utf8(),
                format!("unexpected character `{other}`"),
            )),
        }
    }
}

fn is_identifier_start(ch: char) -> bool {
    ch.is_ascii_alphabetic()
}

fn is_based_prefix(ch: char) -> bool {
    matches!(ch, 'B' | 'b' | 'D' | 'd' | 'O' | 'o' | 'H' | 'h')
}

fn parse_integer(text: &str, base: u32, offset: usize) -> Result<TokenKind, Error> {
    let cleaned: String = text.chars().filter(|ch| *ch != '_').collect();
    if cleaned.is_empty() {
        return Err(Error::new(offset, "integer literal has no digits"));
    }

    match i128::from_str_radix(&cleaned, base) {
        Ok(value) => Ok(TokenKind::Integer { value, base }),
        Err(_) => Err(Error::new(
            offset,
            format!("integer literal `{text}` is invalid or out of range for base {base}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<TokenKind> {
        tokenize(source)
            .unwrap()
            .into_iter()
            .map(|token| token.kind)
            .collect()
    }

    #[test]
    fn hello_example_lexes() {
        let source = include_str!("../examples/hello/src/main.ch");
        let tokens = tokenize(source).unwrap();
        assert!(tokens
            .iter()
            .any(|token| token.kind == TokenKind::Keyword(Keyword::Module)));
        assert!(tokens
            .iter()
            .any(|token| { token.kind == TokenKind::Identifier("AddScaled".to_owned()) }));
        assert!(tokens
            .iter()
            .any(|token| token.kind == TokenKind::Keyword(Keyword::Assert)));
    }

    #[test]
    fn keywords_are_case_sensitive() {
        assert_eq!(kinds("IF"), vec![TokenKind::Keyword(Keyword::If)]);
        assert_eq!(kinds("if"), vec![TokenKind::Identifier("if".to_owned())]);
        assert_eq!(kinds("If"), vec![TokenKind::Identifier("If".to_owned())]);
    }

    #[test]
    fn booleans_are_predefined_names() {
        assert_eq!(
            kinds("TRUE FALSE"),
            vec![TokenKind::Bool(true), TokenKind::Bool(false)]
        );
        assert_eq!(
            kinds("true"),
            vec![TokenKind::Identifier("true".to_owned())]
        );
    }

    #[test]
    fn comments_are_skipped() {
        assert_eq!(
            kinds("a /* b */ c"),
            vec![
                TokenKind::Identifier("a".to_owned()),
                TokenKind::Identifier("c".to_owned())
            ]
        );
        assert_eq!(
            kinds("a -- b\nc"),
            vec![
                TokenKind::Identifier("a".to_owned()),
                TokenKind::Identifier("c".to_owned())
            ]
        );
    }

    #[test]
    fn integer_literals() {
        assert_eq!(
            kinds("1_000"),
            vec![TokenKind::Integer {
                value: 1000,
                base: 10
            }]
        );
        assert_eq!(
            kinds("D'1_000' H'FF' B'1010' O'17'"),
            vec![
                TokenKind::Integer {
                    value: 1000,
                    base: 10
                },
                TokenKind::Integer {
                    value: 255,
                    base: 16
                },
                TokenKind::Integer { value: 10, base: 2 },
                TokenKind::Integer { value: 15, base: 8 },
            ]
        );
    }

    #[test]
    fn character_and_string_literals() {
        assert_eq!(kinds("'M'"), vec![TokenKind::Char('M')]);
        assert_eq!(
            kinds("\"a\"\"b\""),
            vec![TokenKind::String("a\"b".to_owned())]
        );
    }

    #[test]
    fn multi_character_operators() {
        assert_eq!(
            kinds(":= <= >= /= <> -> // (: :) +:= //:= **"),
            vec![
                TokenKind::Assign,
                TokenKind::Le,
                TokenKind::Ge,
                TokenKind::Ne,
                TokenKind::Diamond,
                TokenKind::Arrow,
                TokenKind::Concat,
                TokenKind::LParenColon,
                TokenKind::RParenColon,
                TokenKind::PlusAssign,
                TokenKind::ConcatAssign,
                TokenKind::StarStar,
            ]
        );
    }

    #[test]
    fn rejects_identifier_starting_with_underscore() {
        let errors = tokenize("_x").unwrap_err();
        assert!(errors[0].message.contains("unexpected character `_`"));
    }

    #[test]
    fn rejects_unterminated_comment() {
        let errors = tokenize("/* nope").unwrap_err();
        assert!(errors[0].message.contains("unterminated bracketed comment"));
    }

    #[test]
    fn reports_line_and_column() {
        assert_eq!(line_col("a\nbc", 3), (2, 2));
    }
}
