use crate::ast::ModeRef;
use crate::manifest::WidthProfile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntKind {
    Short,
    Int,
    Long,
}

impl IntKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Short => "SHORT_INT",
            Self::Int => "INT",
            Self::Long => "LONG_INT",
        }
    }

    pub fn width(self, profile: WidthProfile) -> u16 {
        match self {
            Self::Short => profile.short,
            Self::Int => profile.int,
            Self::Long => profile.long,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModeId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scalar {
    Int(IntKind),
    Bool,
    Char,
    String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArrayElement {
    Scalar(Scalar),
    Struct(ModeId),
}

impl ArrayElement {
    pub fn from_type(ty: Type) -> Option<Self> {
        match ty {
            Type::Int(kind) => Some(Self::Scalar(Scalar::Int(kind))),
            Type::Bool => Some(Self::Scalar(Scalar::Bool)),
            Type::Char => Some(Self::Scalar(Scalar::Char)),
            Type::String => Some(Self::Scalar(Scalar::String)),
            Type::Struct(id) => Some(Self::Struct(id)),
            Type::IntLiteral | Type::Array(_) | Type::DynamicArray { .. } | Type::ArrayTuple => {
                None
            }
        }
    }

    pub fn ty(self) -> Type {
        match self {
            Self::Scalar(scalar) => Type::from_scalar(scalar),
            Self::Struct(id) => Type::Struct(id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArrayType {
    pub low: i128,
    pub high: i128,
    pub element: ArrayElement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Int(IntKind),
    /// The `&INT`-derived class of an integer literal.
    IntLiteral,
    Bool,
    Char,
    String,
    Struct(ModeId),
    Array(ArrayType),
    /// A slice whose upper bound is only known at runtime. The lower bound is
    /// inherited from the source array.
    DynamicArray {
        low: i128,
        element: ArrayElement,
    },
    /// An array tuple before its target array mode is known.
    ArrayTuple,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcType {
    pub params: Vec<Type>,
    pub result: Option<Type>,
    /// Exceptions that may escape to the caller. `None` means the set is not
    /// statically known (for example an interface loaded from a grant file).
    pub exceptions: Option<Vec<String>>,
}

impl Type {
    pub fn is_int(self) -> bool {
        matches!(self, Self::Int(_) | Self::IntLiteral)
    }

    pub fn scalar(self) -> Option<Scalar> {
        match self {
            Self::Int(kind) => Some(Scalar::Int(kind)),
            Self::Bool => Some(Scalar::Bool),
            Self::Char => Some(Scalar::Char),
            Self::String => Some(Scalar::String),
            Self::IntLiteral
            | Self::Struct(_)
            | Self::Array(_)
            | Self::DynamicArray { .. }
            | Self::ArrayTuple => None,
        }
    }

    pub fn from_scalar(scalar: Scalar) -> Self {
        match scalar {
            Scalar::Int(kind) => Self::Int(kind),
            Scalar::Bool => Self::Bool,
            Scalar::Char => Self::Char,
            Scalar::String => Self::String,
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Self::Int(kind) => kind.name(),
            Self::IntLiteral => "integer literal",
            Self::Bool => "BOOL",
            Self::Char => "CHAR",
            Self::String => "character string",
            Self::Struct(_) => "structure",
            Self::Array(_) => "array",
            Self::DynamicArray { .. } => "dynamic array slice",
            Self::ArrayTuple => "array literal",
        }
    }
}

/// Resolve a mode name supported by the v0 subset.
pub fn resolve_mode(name: &str) -> Option<Type> {
    match name {
        "SHORT_INT" => Some(Type::Int(IntKind::Short)),
        "INT" => Some(Type::Int(IntKind::Int)),
        "LONG_INT" => Some(Type::Int(IntKind::Long)),
        "BOOL" => Some(Type::Bool),
        "CHAR" => Some(Type::Char),
        "CHARS" => Some(Type::String),
        _ => None,
    }
}

/// Resolve a mode reference, including an inline `ARRAY (low:high) element` shape.
pub fn resolve_mode_ref(
    mode: &ModeRef,
    mode_names: &std::collections::HashMap<String, Type>,
) -> Option<Type> {
    let base = mode_names
        .get(&mode.name)
        .copied()
        .or_else(|| resolve_mode(&mode.name))?;
    match mode.array {
        None => Some(base),
        Some(bounds) => {
            let element = ArrayElement::from_type(base)?;
            Some(Type::Array(ArrayType {
                low: bounds.low,
                high: bounds.high,
                element,
            }))
        }
    }
}

/// Inclusive bounds for a signed integer mode, or `None` if the width is
/// outside the range this implementation can represent.
pub fn int_bounds(kind: IntKind, profile: WidthProfile) -> Option<(i128, i128)> {
    let width = kind.width(profile);
    if width == 0 || width > 127 {
        return None;
    }
    let magnitude = 1i128 << (width - 1);
    Some((-magnitude, magnitude - 1))
}

pub fn contains(kind: IntKind, value: i128, profile: WidthProfile) -> bool {
    int_bounds(kind, profile).is_some_and(|(low, high)| (low..=high).contains(&value))
}
