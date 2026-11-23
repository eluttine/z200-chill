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
    /// End the lifetime of an allocated location.
    Terminate,
    /// Implementation-defined duration built-ins.
    Duration(DurationUnit),
    /// Absolute time construction.
    AbsoluteTime,
    /// Return the actual index of a TEXT/WTEXT location.
    GetTextIndex,
    /// Set the actual index of a TEXT/WTEXT location.
    SetTextIndex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationUnit {
    Milliseconds,
    Seconds,
    Minutes,
    Hours,
    Days,
}

pub fn lookup(name: &str) -> Option<Builtin> {
    match name {
        "WRITE" => Some(Builtin::Write),
        "WRITELINE" => Some(Builtin::WriteLine),
        "READLINE" => Some(Builtin::ReadLine),
        "TERMINATE" => Some(Builtin::Terminate),
        "MILLISECS" => Some(Builtin::Duration(DurationUnit::Milliseconds)),
        "SECS" => Some(Builtin::Duration(DurationUnit::Seconds)),
        "MINUTES" => Some(Builtin::Duration(DurationUnit::Minutes)),
        "HOURS" => Some(Builtin::Duration(DurationUnit::Hours)),
        "DAYS" => Some(Builtin::Duration(DurationUnit::Days)),
        "ABSTIME" => Some(Builtin::AbsoluteTime),
        "GETTEXTINDEX" => Some(Builtin::GetTextIndex),
        "SETTEXTINDEX" => Some(Builtin::SetTextIndex),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionCode {
    Binary,
    Octal,
    Hex,
    Default,
    Scientific,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClauseWidth {
    Fixed(usize),
    Variable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversion {
    pub code: ConversionCode,
    pub width: ClauseWidth,
    pub fractional_width: Option<usize>,
    pub exponent_width: Option<usize>,
    pub left: bool,
    pub overflow: bool,
    pub pad: char,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditingCode {
    Space,
    SkipRight,
    SkipLeft,
    Tab,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editing {
    pub code: EditingCode,
    pub width: ClauseWidth,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatOp {
    Text(String),
    Convert(Conversion),
    Edit(Editing),
    Io(IoControl),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoControl {
    NextRecord,
    NextPage,
    CurrentLine,
    Prompt,
    Emit,
    EndPage,
}

/// Parse the supported subset of a Z.200 format control string.
pub fn parse_format(text: &str) -> Result<Vec<FormatOp>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    let mut ops = Vec::new();
    let mut literal = String::new();

    let flush = |literal: &mut String, ops: &mut Vec<FormatOp>| {
        if !literal.is_empty() {
            ops.push(FormatOp::Text(std::mem::take(literal)));
        }
    };

    while index < chars.len() {
        if chars[index] != '%' {
            literal.push(chars[index]);
            index += 1;
            continue;
        }
        flush(&mut literal, &mut ops);
        index += 1;
        if index >= chars.len() {
            return Err("trailing `%` in format text".to_owned());
        }
        if chars[index] == '%' {
            literal.push('%');
            index += 1;
            continue;
        }
        let repetition_start = index;
        while index < chars.len() && chars[index].is_ascii_digit() {
            index += 1;
        }
        let repetition = if index == repetition_start {
            1
        } else {
            chars[repetition_start..index]
                .iter()
                .collect::<String>()
                .parse::<usize>()
                .map_err(|_| "invalid repetition factor in format text".to_owned())?
        };
        if index >= chars.len() {
            return Err("missing format clause after `%`".to_owned());
        }
        let code = chars[index];
        index += 1;
        match code {
            'B' | 'O' | 'H' | 'C' | 'F' => {
                let code = match code {
                    'B' => ConversionCode::Binary,
                    'O' => ConversionCode::Octal,
                    'H' => ConversionCode::Hex,
                    'C' => ConversionCode::Default,
                    _ => ConversionCode::Scientific,
                };
                let mut left = false;
                let mut overflow = false;
                let mut pad = ' ';
                loop {
                    if index >= chars.len() {
                        break;
                    }
                    match chars[index] {
                        'L' => {
                            left = true;
                            index += 1;
                        }
                        'E' => {
                            overflow = true;
                            index += 1;
                        }
                        'P' => {
                            index += 1;
                            let Some(character) = chars.get(index) else {
                                return Err("`%P` requires a padding character".to_owned());
                            };
                            pad = *character;
                            index += 1;
                        }
                        _ => break,
                    }
                }
                let width = if index < chars.len() && chars[index] == 'V' {
                    index += 1;
                    ClauseWidth::Variable
                } else {
                    let width_start = index;
                    while index < chars.len() && chars[index].is_ascii_digit() {
                        index += 1;
                    }
                    if index == width_start {
                        ClauseWidth::Fixed(0)
                    } else {
                        ClauseWidth::Fixed(
                            chars[width_start..index]
                                .iter()
                                .collect::<String>()
                                .parse::<usize>()
                                .map_err(|_| "invalid format width".to_owned())?,
                        )
                    }
                };
                let mut fractional_width = None;
                if index + 1 < chars.len()
                    && chars[index] == '.'
                    && chars[index + 1].is_ascii_digit()
                {
                    index += 1;
                    let start = index;
                    while index < chars.len() && chars[index].is_ascii_digit() {
                        index += 1;
                    }
                    if index == start {
                        return Err("fractional format width has no digits".to_owned());
                    }
                    fractional_width = Some(
                        chars[start..index]
                            .iter()
                            .collect::<String>()
                            .parse::<usize>()
                            .map_err(|_| "invalid fractional format width".to_owned())?,
                    );
                }
                let mut exponent_width = None;
                if index + 1 < chars.len()
                    && chars[index] == ':'
                    && chars[index + 1].is_ascii_digit()
                {
                    index += 1;
                    let start = index;
                    while index < chars.len() && chars[index].is_ascii_digit() {
                        index += 1;
                    }
                    if index == start {
                        return Err("exponent format width has no digits".to_owned());
                    }
                    exponent_width = Some(
                        chars[start..index]
                            .iter()
                            .collect::<String>()
                            .parse::<usize>()
                            .map_err(|_| "invalid exponent format width".to_owned())?,
                    );
                }
                let conversion = Conversion {
                    code,
                    width,
                    fractional_width,
                    exponent_width,
                    left,
                    overflow,
                    pad,
                };
                for _ in 0..repetition {
                    ops.push(FormatOp::Convert(conversion.clone()));
                }
                continue;
            }
            'X' | '>' | '<' | 'T' => {
                let code = match code {
                    'X' => EditingCode::Space,
                    '>' => EditingCode::SkipRight,
                    '<' => EditingCode::SkipLeft,
                    _ => EditingCode::Tab,
                };
                let width = if index < chars.len() && chars[index] == 'V' {
                    index += 1;
                    ClauseWidth::Variable
                } else {
                    let width_start = index;
                    while index < chars.len() && chars[index].is_ascii_digit() {
                        index += 1;
                    }
                    if index == width_start {
                        if matches!(code, EditingCode::Tab) {
                            ClauseWidth::Fixed(0)
                        } else {
                            ClauseWidth::Fixed(1)
                        }
                    } else {
                        ClauseWidth::Fixed(
                            chars[width_start..index]
                                .iter()
                                .collect::<String>()
                                .parse::<usize>()
                                .map_err(|_| "invalid format width".to_owned())?,
                        )
                    }
                };
                let editing = Editing { code, width };
                for _ in 0..repetition {
                    ops.push(FormatOp::Edit(editing.clone()));
                }
                continue;
            }
            '/' | '-' | '+' | '?' | '!' | '=' => {
                let control = match code {
                    '/' => IoControl::NextRecord,
                    '+' => IoControl::NextPage,
                    '-' => IoControl::CurrentLine,
                    '?' => IoControl::Prompt,
                    '!' => IoControl::Emit,
                    _ => IoControl::EndPage,
                };
                for _ in 0..repetition {
                    ops.push(FormatOp::Io(control));
                }
                continue;
            }
            '(' => {
                let inner_start = index;
                let mut depth = 1usize;
                let mut scan = index;
                let mut inner_end = None;
                while scan < chars.len() {
                    if chars[scan] == '%' {
                        match chars.get(scan + 1) {
                            Some('(') => {
                                depth += 1;
                                scan += 2;
                                continue;
                            }
                            Some(')') => {
                                depth -= 1;
                                if depth == 0 {
                                    inner_end = Some(scan);
                                    scan += 2;
                                    break;
                                }
                                scan += 2;
                                continue;
                            }
                            _ => {}
                        }
                    }
                    scan += 1;
                }
                let Some(inner_end) = inner_end else {
                    return Err("unterminated parenthesized format clause".to_owned());
                };
                let inner: String = chars[inner_start..inner_end].iter().collect();
                let inner_ops = parse_format(&inner)?;
                for _ in 0..repetition {
                    ops.extend(inner_ops.clone());
                }
                index = scan;
                continue;
            }
            other => {
                return Err(format!("unknown format code `%{other}`"));
            }
        }
    }
    flush(&mut literal, &mut ops);
    Ok(ops)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatItem {
    Conversion(ConversionCode),
    Width,
}

pub fn format_items(ops: &[FormatOp]) -> Vec<FormatItem> {
    let mut items = Vec::new();
    for op in ops {
        match op {
            FormatOp::Text(_) => {}
            FormatOp::Convert(conversion) => {
                if conversion.width == ClauseWidth::Variable {
                    items.push(FormatItem::Width);
                }
                items.push(FormatItem::Conversion(conversion.code));
            }
            FormatOp::Edit(editing) => {
                if editing.width == ClauseWidth::Variable {
                    items.push(FormatItem::Width);
                }
            }
            FormatOp::Io(_) => {}
        }
    }
    items
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssociationOp {
    Associate,
    IsAssociated,
    Dissociate,
    Create,
    Delete,
    Modify,
    Connect,
    Disconnect,
    ReadRecord,
    WriteRecord,
    Existing,
    Readable,
    Writeable,
    Indexable,
    Sequencible,
    Variable,
}

pub fn lookup_association(name: &str) -> Option<AssociationOp> {
    match name {
        "ASSOCIATE" => Some(AssociationOp::Associate),
        "ISASSOCIATED" => Some(AssociationOp::IsAssociated),
        "DISSOCIATE" => Some(AssociationOp::Dissociate),
        "CREATE" => Some(AssociationOp::Create),
        "DELETE" => Some(AssociationOp::Delete),
        "MODIFY" => Some(AssociationOp::Modify),
        "CONNECT" => Some(AssociationOp::Connect),
        "DISCONNECT" => Some(AssociationOp::Disconnect),
        "READRECORD" => Some(AssociationOp::ReadRecord),
        "WRITERECORD" => Some(AssociationOp::WriteRecord),
        "EXISTING" => Some(AssociationOp::Existing),
        "READABLE" => Some(AssociationOp::Readable),
        "WRITEABLE" => Some(AssociationOp::Writeable),
        "INDEXABLE" => Some(AssociationOp::Indexable),
        "SEQUENCIBLE" => Some(AssociationOp::Sequencible),
        "VARIABLE" => Some(AssociationOp::Variable),
        _ => None,
    }
}
