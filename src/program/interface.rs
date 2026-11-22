use crate::modes::{FieldInfo, ModeTable, StructMode};
use crate::types::{ModeId, ProcType, Type};
use std::collections::{HashMap, HashSet};

use super::{ExportEntry, ExportKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalType {
    Builtin(Type),
    Mode(String),
    Array {
        low: i128,
        high: i128,
        element: Box<ExternalType>,
    },
    MultiArray {
        dims: Vec<crate::types::ArrayDimension>,
        element: Box<ExternalType>,
    },
}

#[derive(Debug, Clone)]
pub struct ModeInfo {
    pub name: String,
    pub module_name: Option<String>,
    pub package_name: Option<String>,
    pub fields: Vec<(String, ExternalType)>,
    pub forbidden: Vec<String>,
}

impl ModeInfo {
    pub fn symbol(&self) -> String {
        crate::modes::mode_symbol(
            self.package_name.as_deref(),
            self.module_name.as_deref(),
            &self.name,
        )
    }
}

#[derive(Debug, Clone)]
pub struct ExportInfo {
    pub name: String,
    pub module_name: Option<String>,
    pub package_name: Option<String>,
    pub prefix: Vec<String>,
    pub kind: ExportInfoKind,
    pub friends: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub enum ExportInfoKind {
    Var(ExternalType),
    Proc {
        params: Vec<ExternalType>,
        passing: Vec<crate::ast::Passing>,
        result: Option<ExternalType>,
    },
    Mode(ModeInfo),
}

pub fn portable_type(ty: Type, modes: &ModeTable) -> ExternalType {
    match ty {
        Type::Struct(id) => ExternalType::Mode(modes.get(id).symbol()),
        Type::Array(array) => ExternalType::Array {
            low: array.low,
            high: array.high,
            element: Box::new(portable_type(array.element.ty(), modes)),
        },
        Type::MultiArray(array) => ExternalType::MultiArray {
            dims: array.dimensions().to_vec(),
            element: Box::new(portable_type(array.element.ty(), modes)),
        },
        other => ExternalType::Builtin(other),
    }
}

/// A dependency's portable interface: mode definitions needed to interpret its
/// exported types, plus the names it actually grants.
#[derive(Debug, Clone, Default)]
pub struct ExternalInterface {
    pub modes: Vec<ModeInfo>,
    pub exports: Vec<ExportInfo>,
}

/// Symbolic mode names encountered in external interfaces, mapped to freshly
/// allocated local `ModeId`s.
#[derive(Debug, Default)]
pub struct ExternalModeIds {
    ids: HashMap<String, ModeId>,
}

impl ExternalModeIds {
    pub fn get(&self, symbol: &str) -> Option<ModeId> {
        self.ids.get(symbol).copied()
    }
}

/// Instantiate structure modes from external grant files into `modes`.
pub fn add_external_modes(
    modes: &mut ModeTable,
    external: &[ExternalInterface],
) -> Result<ExternalModeIds, String> {
    let mut ids = ExternalModeIds::default();
    let mut definitions: Vec<&ModeInfo> = Vec::new();

    for interface in external {
        for mode in &interface.modes {
            let symbol = mode.symbol();
            if ids.ids.contains_key(&symbol) {
                continue;
            }
            let id = ModeId(modes.modes.len());
            ids.ids.insert(symbol.clone(), id);
            modes.modes.push(StructMode {
                name: mode.name.clone(),
                // External modes are never the local defining module.
                package: usize::MAX,
                module: usize::MAX,
                package_name: mode.package_name.clone(),
                module_name: mode.module_name.clone(),
                fields: Vec::new(),
                forbidden: HashSet::new(),
            });
            definitions.push(mode);
        }
    }

    detect_mode_cycles(&definitions, &ids)?;

    for mode in definitions {
        let id = ids.ids[&mode.symbol()];
        let mut fields = Vec::with_capacity(mode.fields.len());
        let mut names = HashSet::new();
        for (name, ty) in &mode.fields {
            if !names.insert(name.clone()) {
                return Err(format!(
                    "duplicate field `{name}` in external mode `{}`",
                    mode.symbol()
                ));
            }
            let ty = resolve_external_type(ty, &ids)?;
            fields.push(FieldInfo {
                name: name.clone(),
                ty,
            });
        }
        let mut forbidden = HashSet::new();
        for name in &mode.forbidden {
            if !fields.iter().any(|field| &field.name == name) {
                return Err(format!(
                    "external mode `{}` forbids unknown field `{name}`",
                    mode.symbol()
                ));
            }
            forbidden.insert(name.clone());
        }
        let entry = modes.get_mut(id);
        entry.fields = fields;
        entry.forbidden = forbidden;
    }

    Ok(ids)
}

fn detect_mode_cycles(definitions: &[&ModeInfo], ids: &ExternalModeIds) -> Result<(), String> {
    fn visit(
        index: usize,
        definitions: &[&ModeInfo],
        by_symbol: &HashMap<String, usize>,
        ids: &ExternalModeIds,
        visiting: &mut HashSet<usize>,
        done: &mut HashSet<usize>,
    ) -> Result<(), String> {
        if done.contains(&index) {
            return Ok(());
        }
        if !visiting.insert(index) {
            return Err(format!(
                "cyclic structure mode `{}` in external interface",
                definitions[index].symbol()
            ));
        }
        for (_, ty) in &definitions[index].fields {
            let ExternalType::Mode(reference) = ty else {
                continue;
            };
            if ids.get(reference).is_none() {
                return Err(format!(
                    "external mode `{}` references unknown mode `{reference}`",
                    definitions[index].symbol()
                ));
            }
            if let Some(next) = by_symbol.get(reference) {
                visit(*next, definitions, by_symbol, ids, visiting, done)?;
            }
        }
        visiting.remove(&index);
        done.insert(index);
        Ok(())
    }

    let by_symbol: HashMap<String, usize> = definitions
        .iter()
        .enumerate()
        .map(|(index, mode)| (mode.symbol(), index))
        .collect();
    let mut done = HashSet::new();
    for index in 0..definitions.len() {
        let mut visiting = HashSet::new();
        visit(
            index,
            definitions,
            &by_symbol,
            ids,
            &mut visiting,
            &mut done,
        )?;
    }
    Ok(())
}

pub fn add_external_exports(
    exports: &mut Vec<ExportEntry>,
    external: &[ExternalInterface],
    ids: &ExternalModeIds,
) -> Result<(), String> {
    for interface in external {
        for info in &interface.exports {
            let kind = match &info.kind {
                ExportInfoKind::Var(ty) => ExportKind::Var(resolve_external_type(ty, ids)?),
                ExportInfoKind::Proc {
                    params,
                    passing,
                    result,
                } => ExportKind::Proc(ProcType {
                    params: params
                        .iter()
                        .map(|ty| resolve_external_type(ty, ids))
                        .collect::<Result<_, _>>()?,
                    passing: passing.clone(),
                    result: match result {
                        Some(ty) => Some(resolve_external_type(ty, ids)?),
                        None => None,
                    },
                    exceptions: None,
                }),
                ExportInfoKind::Mode(mode) => {
                    let id = ids.get(&mode.symbol()).ok_or_else(|| {
                        format!("external mode `{}` was not materialized", mode.symbol())
                    })?;
                    ExportKind::Mode(id)
                }
            };
            exports.push(ExportEntry {
                module: usize::MAX,
                package: usize::MAX,
                package_name: info.package_name.clone(),
                module_name: info.module_name.clone(),
                name: info.name.clone(),
                prefix: info.prefix.clone(),
                kind,
                friends: info.friends.clone(),
            });
        }
    }
    Ok(())
}

fn resolve_external_type(ty: &ExternalType, ids: &ExternalModeIds) -> Result<Type, String> {
    match ty {
        ExternalType::Builtin(Type::Struct(_)) => {
            Err("internal: structure modes must use symbolic references".to_owned())
        }
        ExternalType::Builtin(ty) => Ok(*ty),
        ExternalType::Mode(symbol) => ids
            .get(symbol)
            .map(Type::Struct)
            .ok_or_else(|| format!("external interface references unknown mode `{symbol}`")),
        ExternalType::Array { low, high, element } => {
            let element = resolve_external_type(element, ids)?;
            let Some(element) = crate::types::ArrayElement::from_type(element) else {
                return Err("array element must be a scalar or structure mode".to_owned());
            };
            Ok(Type::Array(crate::types::ArrayType {
                low: *low,
                high: *high,
                element,
            }))
        }
        ExternalType::MultiArray { dims, element } => {
            let element = resolve_external_type(element, ids)?;
            let Some(element) = crate::types::ArrayElement::from_type(element) else {
                return Err("array element must be a scalar or structure mode".to_owned());
            };
            let Some(array) = crate::types::MultiArrayType::new(dims, element) else {
                return Err("unsupported multidimensional array shape".to_owned());
            };
            Ok(Type::MultiArray(array))
        }
    }
}
