use crate::ast::{ArrayBounds, ModeRef, Passing};
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
pub struct SetId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MoretaId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scalar {
    Int(IntKind),
    Float,
    Bool,
    Char,
    String,
    BoundedString { length: usize, varying: bool },
    WChar,
    WString,
    WBoundedString { length: usize, varying: bool },
    Bits,
    BoundedBits { length: usize, varying: bool },
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
            Type::Float => Some(Self::Scalar(Scalar::Float)),
            Type::Bool => Some(Self::Scalar(Scalar::Bool)),
            Type::Char => Some(Self::Scalar(Scalar::Char)),
            Type::String => Some(Self::Scalar(Scalar::String)),
            Type::BoundedString { length, varying } => {
                Some(Self::Scalar(Scalar::BoundedString { length, varying }))
            }
            Type::WChar => Some(Self::Scalar(Scalar::WChar)),
            Type::WString => Some(Self::Scalar(Scalar::WString)),
            Type::WBoundedString { length, varying } => {
                Some(Self::Scalar(Scalar::WBoundedString { length, varying }))
            }
            Type::Bits => Some(Self::Scalar(Scalar::Bits)),
            Type::BoundedBits { length, varying } => {
                Some(Self::Scalar(Scalar::BoundedBits { length, varying }))
            }
            Type::Struct(id) => Some(Self::Struct(id)),
            Type::Association
            | Type::Access(_)
            | Type::PowerSet(_)
            | Type::Set(_)
            | Type::Moreta(_)
            | Type::IntLiteral
            | Type::Array(_)
            | Type::MultiArray(_)
            | Type::DynamicArray { .. }
            | Type::ArrayTuple => None,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PowerSetMember {
    Int(IntKind),
    Bool,
    Char,
    WChar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PowerSetType {
    pub member: PowerSetMember,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AccessType {
    pub indexed: bool,
    pub index: Option<ArrayDimension>,
}

pub const MAX_ARRAY_RANK: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArrayDimension {
    pub low: i128,
    pub high: i128,
}

impl ArrayDimension {
    pub fn length(self) -> i128 {
        self.high - self.low + 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MultiArrayType {
    dims: [ArrayDimension; MAX_ARRAY_RANK],
    rank: usize,
    pub element: ArrayElement,
}

impl MultiArrayType {
    pub fn new(dims: &[ArrayDimension], element: ArrayElement) -> Option<Self> {
        if dims.is_empty() || dims.len() > MAX_ARRAY_RANK {
            return None;
        }
        let mut slots = [ArrayDimension { low: 0, high: 0 }; MAX_ARRAY_RANK];
        slots[..dims.len()].copy_from_slice(dims);
        Some(Self {
            dims: slots,
            rank: dims.len(),
            element,
        })
    }

    pub fn dimensions(&self) -> &[ArrayDimension] {
        &self.dims[..self.rank]
    }

    pub fn rank(&self) -> usize {
        self.rank
    }

    pub fn total_len(&self) -> i128 {
        self.dimensions().iter().map(|dim| dim.length()).product()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Int(IntKind),
    /// The `&INT`-derived class of an integer literal.
    IntLiteral,
    Float,
    Bool,
    Char,
    String,
    /// `CHARS(n)` fixed or `CHARS(n) VARYING` bounded string mode.
    BoundedString {
        length: usize,
        varying: bool,
    },
    /// A `WCHAR` value, a UCS-2 character.
    WChar,
    WString,
    /// `WCHARS(n)` fixed or `WCHARS(n) VARYING` bounded wide string mode.
    WBoundedString {
        length: usize,
        varying: bool,
    },
    /// An unbounded bit string (`BOOLS`).
    Bits,
    /// `BOOLS(n)` fixed or `BOOLS(n) VARYING` bounded bit string mode.
    BoundedBits {
        length: usize,
        varying: bool,
    },
    Struct(ModeId),
    Set(SetId),
    Moreta(MoretaId),
    Array(ArrayType),
    MultiArray(MultiArrayType),
    /// A slice whose upper bound is only known at runtime. The lower bound is
    /// inherited from the source array.
    DynamicArray {
        low: i128,
        element: ArrayElement,
    },
    /// An opaque association-mode value.
    Association,
    /// An opaque access-mode value.
    Access(AccessType),
    /// A powerset over a discrete scalar member mode.
    PowerSet(PowerSetType),
    /// An array tuple before its target array mode is known.
    ArrayTuple,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcType {
    pub params: Vec<Type>,
    pub passing: Vec<Passing>,
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
            Self::Float => Some(Scalar::Float),
            Self::Bool => Some(Scalar::Bool),
            Self::Char => Some(Scalar::Char),
            Self::String => Some(Scalar::String),
            Self::BoundedString { length, varying } => {
                Some(Scalar::BoundedString { length, varying })
            }
            Self::WChar => Some(Scalar::WChar),
            Self::WString => Some(Scalar::WString),
            Self::WBoundedString { length, varying } => {
                Some(Scalar::WBoundedString { length, varying })
            }
            Self::Bits => Some(Scalar::Bits),
            Self::BoundedBits { length, varying } => Some(Scalar::BoundedBits { length, varying }),
            Self::Association
            | Self::Access(_)
            | Self::PowerSet(_)
            | Self::Set(_)
            | Self::Moreta(_)
            | Self::IntLiteral
            | Self::Struct(_)
            | Self::Array(_)
            | Self::MultiArray(_)
            | Self::DynamicArray { .. }
            | Self::ArrayTuple => None,
        }
    }

    pub fn from_scalar(scalar: Scalar) -> Self {
        match scalar {
            Scalar::Int(kind) => Self::Int(kind),
            Scalar::Float => Self::Float,
            Scalar::Bool => Self::Bool,
            Scalar::Char => Self::Char,
            Scalar::String => Self::String,
            Scalar::BoundedString { length, varying } => Self::BoundedString { length, varying },
            Scalar::WChar => Self::WChar,
            Scalar::WString => Self::WString,
            Scalar::WBoundedString { length, varying } => Self::WBoundedString { length, varying },
            Scalar::Bits => Self::Bits,
            Scalar::BoundedBits { length, varying } => Self::BoundedBits { length, varying },
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Self::Int(kind) => kind.name(),
            Self::IntLiteral => "integer literal",
            Self::Float => "FLOAT",
            Self::Bool => "BOOL",
            Self::Char => "CHAR",
            Self::String => "character string",
            Self::BoundedString { varying: true, .. } => "varying character string",
            Self::BoundedString { .. } => "fixed character string",
            Self::WChar => "WCHAR",
            Self::WString => "wide character string",
            Self::WBoundedString { varying: true, .. } => "varying wide character string",
            Self::WBoundedString { .. } => "fixed wide character string",
            Self::Bits => "bit string",
            Self::BoundedBits { varying: true, .. } => "varying bit string",
            Self::BoundedBits { .. } => "fixed bit string",
            Self::Struct(_) => "structure",
            Self::Set(_) => "set",
            Self::Moreta(_) => "module mode object",
            Self::Array(_) => "array",
            Self::MultiArray(_) => "multidimensional array",
            Self::DynamicArray { .. } => "dynamic array slice",
            Self::Association => "association",
            Self::Access(_) => "access",
            Self::PowerSet(_) => "powerset",
            Self::ArrayTuple => "array literal",
        }
    }
}

/// Resolve a mode name supported by the v0 subset.
pub fn resolve_mode(name: &str) -> Option<Type> {
    match name {
        "SHORT_INT" => Some(Type::Int(IntKind::Short)),
        "INT" => Some(Type::Int(IntKind::Int)),
        "FLOAT" => Some(Type::Float),
        "LONG_INT" => Some(Type::Int(IntKind::Long)),
        "BOOL" => Some(Type::Bool),
        "CHAR" => Some(Type::Char),
        "CHARS" => Some(Type::String),
        "WCHAR" => Some(Type::WChar),
        "WCHARS" => Some(Type::WString),
        "BOOLS" => Some(Type::Bits),
        "ASSOCIATION" => Some(Type::Association),
        _ => None,
    }
}

pub fn array_type_from_bounds(bounds: &[ArrayBounds], element: ArrayElement) -> Option<Type> {
    match bounds {
        [] => None,
        [bounds] => Some(Type::Array(ArrayType {
            low: bounds.low,
            high: bounds.high,
            element,
        })),
        bounds => {
            let dims: Vec<ArrayDimension> = bounds
                .iter()
                .map(|bounds| ArrayDimension {
                    low: bounds.low,
                    high: bounds.high,
                })
                .collect();
            Some(Type::MultiArray(MultiArrayType::new(&dims, element)?))
        }
    }
}

/// Resolve a mode reference, including an inline `ARRAY (low:high) element` shape.
pub fn resolve_mode_ref(
    mode: &ModeRef,
    mode_names: &std::collections::HashMap<String, Type>,
) -> Option<Type> {
    if mode.powerset {
        let member = match resolve_mode(&mode.name)? {
            Type::Int(kind) => PowerSetMember::Int(kind),
            Type::Bool => PowerSetMember::Bool,
            Type::Char => PowerSetMember::Char,
            Type::WChar => PowerSetMember::WChar,
            _ => return None,
        };
        return Some(Type::PowerSet(PowerSetType { member }));
    }
    if mode.access {
        return Some(Type::Access(AccessType {
            indexed: mode.access_indexed,
            index: mode.access_index.map(|bounds| ArrayDimension {
                low: bounds.low,
                high: bounds.high,
            }),
        }));
    }
    let base = mode_names
        .get(&mode.name)
        .copied()
        .or_else(|| resolve_mode(&mode.name))?;
    let base = match mode.string_length {
        Some(length) => match base {
            Type::WString => Type::WBoundedString {
                length,
                varying: mode.varying,
            },
            Type::Bits => Type::BoundedBits {
                length,
                varying: mode.varying,
            },
            _ => Type::BoundedString {
                length,
                varying: mode.varying,
            },
        },
        None => base,
    };
    if mode.arrays.is_empty() {
        Some(base)
    } else {
        let element = ArrayElement::from_type(base)?;
        array_type_from_bounds(&mode.arrays, element)
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
