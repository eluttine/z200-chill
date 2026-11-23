use crate::modes::{FieldInfo, ModeTable, StructMode};
use crate::types::{ModeId, ProcType, Type};
use std::collections::{HashMap, HashSet};

use super::{ExportEntry, ExportKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalType {
    Builtin(Type),
    Mode(String),
    Moreta(String),
    Set(String),
    Ref(Box<ExternalType>),
    Row(Box<ExternalType>),
    Ptr,
    Buffer {
        length: Option<usize>,
        element: Box<ExternalType>,
    },
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
    pub implements: Vec<String>,
    pub module_name: Option<String>,
    pub package_name: Option<String>,
    pub fields: Vec<(String, ExternalType)>,
    pub elements: Vec<(String, i128)>,
    pub forbidden: Vec<String>,
    pub kind: ModeInfoKind,
    pub components: Vec<ModeComponentInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeInfoKind {
    Struct,
    Set,
    Moreta,
    Region,
    Task,
    Interface,
}

#[derive(Debug, Clone)]
pub enum ModeComponentInfo {
    Var {
        name: String,
        ty: ExternalType,
        public: bool,
    },
    Proc {
        name: String,
        params: Vec<ExternalType>,
        passing: Vec<crate::ast::Passing>,
        result: Option<ExternalType>,
        constructor: bool,
        destructor: bool,
        public: bool,
    },
    Mode {
        name: String,
        ty: ExternalType,
        public: bool,
    },
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
        Type::Set(id) => ExternalType::Set(modes.get_set(id).symbol()),
        Type::Moreta(id) => ExternalType::Moreta(modes.get_moreta(id).symbol()),
        Type::Ref(target) => ExternalType::Ref(Box::new(portable_type(target.ty(), modes))),
        Type::Row(target) => ExternalType::Row(Box::new(portable_type(target.ty(), modes))),
        Type::Ptr => ExternalType::Ptr,
        Type::Buffer { length, element } => ExternalType::Buffer {
            length,
            element: Box::new(portable_type(element.ty(), modes)),
        },
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
    moreta_ids: HashMap<String, crate::types::MoretaId>,
    set_ids: HashMap<String, crate::types::SetId>,
}

impl ExternalModeIds {
    pub fn get(&self, symbol: &str) -> Option<ModeId> {
        self.ids.get(symbol).copied()
    }

    pub fn get_moreta(&self, symbol: &str) -> Option<crate::types::MoretaId> {
        self.moreta_ids.get(symbol).copied()
    }

    pub fn get_set(&self, symbol: &str) -> Option<crate::types::SetId> {
        self.set_ids.get(symbol).copied()
    }
}

/// Instantiate structure and module modes from external grant files into `modes`.
pub fn add_external_modes(
    modes: &mut ModeTable,
    external: &[ExternalInterface],
) -> Result<ExternalModeIds, String> {
    let mut ids = ExternalModeIds::default();
    let mut definitions: Vec<&ModeInfo> = Vec::new();
    let mut set_definitions: Vec<&ModeInfo> = Vec::new();
    let mut moreta_definitions: Vec<&ModeInfo> = Vec::new();

    for interface in external {
        for mode in &interface.modes {
            let symbol = mode.symbol();
            if ids.ids.contains_key(&symbol)
                || ids.set_ids.contains_key(&symbol)
                || ids.moreta_ids.contains_key(&symbol)
            {
                continue;
            }
            match mode.kind {
                ModeInfoKind::Struct => {
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
                ModeInfoKind::Set => {
                    let id = crate::types::SetId(modes.sets.len());
                    ids.set_ids.insert(symbol.clone(), id);
                    modes.sets.push(crate::modes::SetMode {
                        name: mode.name.clone(),
                        package: usize::MAX,
                        module: usize::MAX,
                        package_name: mode.package_name.clone(),
                        module_name: mode.module_name.clone(),
                        elements: Vec::new(),
                    });
                    set_definitions.push(mode);
                }
                ModeInfoKind::Moreta
                | ModeInfoKind::Region
                | ModeInfoKind::Task
                | ModeInfoKind::Interface => {
                    let id = crate::types::MoretaId(modes.moretas.len());
                    ids.moreta_ids.insert(symbol.clone(), id);
                    modes.moretas.push(crate::modes::MoretaMode {
                        name: mode.name.clone(),
                        kind: match mode.kind {
                            ModeInfoKind::Region => crate::ast::MoretaKind::Region,
                            ModeInfoKind::Task => crate::ast::MoretaKind::Task,
                            ModeInfoKind::Interface => crate::ast::MoretaKind::Interface,
                            _ => crate::ast::MoretaKind::Module,
                        },
                        package: usize::MAX,
                        module: usize::MAX,
                        package_name: mode.package_name.clone(),
                        module_name: mode.module_name.clone(),
                        bases: Vec::new(),
                        implements: Vec::new(),
                        abstract_mode: matches!(
                            mode.kind,
                            ModeInfoKind::Region | ModeInfoKind::Task
                        ),
                        final_mode: false,
                        assignable: false,
                        components: Vec::new(),
                    });
                    moreta_definitions.push(mode);
                }
            }
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

    for mode in set_definitions {
        let id = ids.set_ids[&mode.symbol()];
        modes.sets[id.0].elements = mode.elements.clone();
    }

    for mode in moreta_definitions {
        let id = ids.moreta_ids[&mode.symbol()];
        let mut components = Vec::with_capacity(mode.components.len());
        for component in &mode.components {
            match component {
                ModeComponentInfo::Var { name, ty, public } => {
                    components.push(crate::modes::MoretaComponent::Var {
                        name: name.clone(),
                        ty: resolve_external_type(ty, &ids)?,
                        visibility: if *public {
                            crate::modes::ComponentVisibility::Public
                        } else {
                            crate::modes::ComponentVisibility::Internal
                        },
                    });
                }
                ModeComponentInfo::Proc {
                    name,
                    params,
                    passing,
                    result,
                    constructor,
                    destructor,
                    public,
                } => {
                    let params = params
                        .iter()
                        .map(|ty| resolve_external_type(ty, &ids))
                        .collect::<Result<Vec<_>, _>>()?;
                    let result = match result {
                        Some(ty) => Some(resolve_external_type(ty, &ids)?),
                        None => None,
                    };
                    components.push(crate::modes::MoretaComponent::Proc {
                        name: name.clone(),
                        signature: ProcType {
                            params,
                            passing: passing.clone(),
                            result,
                            exceptions: None,
                        },
                        span: crate::lexer::Span::new(crate::lexer::SourceId(0), 0, 0),
                        constructor: *constructor,
                        destructor: *destructor,
                        implemented: false,
                        visibility: if *public {
                            crate::modes::ComponentVisibility::Public
                        } else {
                            crate::modes::ComponentVisibility::Internal
                        },
                    });
                }
                ModeComponentInfo::Mode { name, ty, public } => {
                    components.push(crate::modes::MoretaComponent::Mode {
                        name: name.clone(),
                        ty: resolve_external_type(ty, &ids)?,
                        visibility: if *public {
                            crate::modes::ComponentVisibility::Public
                        } else {
                            crate::modes::ComponentVisibility::Internal
                        },
                    });
                }
            }
        }
        let implements =
            mode.implements
                .iter()
                .map(|symbol| {
                    ids.moreta_ids.get(symbol).copied().ok_or_else(|| {
                        format!("external interface references unknown mode `{symbol}`")
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
        modes.moretas[id.0].components = components;
        modes.moretas[id.0].implements = implements;
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
                    let symbol = mode.symbol();
                    let ty = match mode.kind {
                        ModeInfoKind::Struct => ids.get(&symbol).map(Type::Struct),
                        ModeInfoKind::Set => ids.get_set(&symbol).map(Type::Set),
                        ModeInfoKind::Moreta
                        | ModeInfoKind::Region
                        | ModeInfoKind::Task
                        | ModeInfoKind::Interface => ids.get_moreta(&symbol).map(Type::Moreta),
                    }
                    .ok_or_else(|| format!("external mode `{symbol}` was not materialized"))?;
                    ExportKind::Mode(ty)
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
        ExternalType::Builtin(Type::Struct(_) | Type::Moreta(_)) => {
            Err("internal: modes must use symbolic references".to_owned())
        }
        ExternalType::Builtin(ty) => Ok(*ty),
        ExternalType::Mode(symbol) => ids
            .get(symbol)
            .map(Type::Struct)
            .ok_or_else(|| format!("external interface references unknown mode `{symbol}`")),
        ExternalType::Moreta(symbol) => ids
            .get_moreta(symbol)
            .map(Type::Moreta)
            .ok_or_else(|| format!("external interface references unknown mode `{symbol}`")),
        ExternalType::Set(symbol) => ids
            .get_set(symbol)
            .map(Type::Set)
            .ok_or_else(|| format!("external interface references unknown set mode `{symbol}`")),
        ExternalType::Ptr => Ok(Type::Ptr),
        ExternalType::Ref(target) => {
            let target = resolve_external_type(target, ids)?;
            let Some(target) = crate::types::RefTarget::from_type(target) else {
                return Err("reference target must be a referable mode".to_owned());
            };
            Ok(Type::Ref(target))
        }
        ExternalType::Row(target) => {
            let target = resolve_external_type(target, ids)?;
            let Some(target) = crate::types::RefTarget::from_type(target) else {
                return Err("row target must be a referable mode".to_owned());
            };
            Ok(Type::Row(target))
        }
        ExternalType::Buffer { length, element } => {
            let element = resolve_external_type(element, ids)?;
            let Some(element) = crate::types::RefTarget::from_type(element) else {
                return Err("buffer element must be a referable mode".to_owned());
            };
            Ok(Type::Buffer {
                length: *length,
                element,
            })
        }
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
