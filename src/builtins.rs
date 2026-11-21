/// Implementation-defined text I/O routines exposed by z200c.
///
/// Z.200 permits implementation-defined built-in routine names. These are not the
/// standard `READTEXT`/`WRITETEXT` operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextIo {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    /// Write a CHARS value to stdout without a newline.
    Write,
    /// Write a CHARS value (or an empty line) to stdout with a newline.
    WriteLine,
    /// Read one line from stdin as CHARS, without the line terminator.
    ReadLine,
}

pub fn lookup(name: &str) -> Option<Builtin> {
    match name {
        "WRITE" => Some(Builtin::Write),
        "WRITELINE" => Some(Builtin::WriteLine),
        "READLINE" => Some(Builtin::ReadLine),
        _ => None,
    }
}
