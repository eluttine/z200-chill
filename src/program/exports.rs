use crate::ast::{NameRef, Program};
use crate::types::{ProcType, Type};
use std::collections::HashMap;

use super::{
    analyze, normalize, portable_type, Error, ExportInfo, ExportInfoKind, ModeComponentInfo,
    ModeInfo, ModeInfoKind,
};

#[derive(Debug, Clone, Default)]
pub struct ModuleInterface {
    pub vars: HashMap<String, Type>,
    pub procs: HashMap<String, ProcType>,
    pub modes: HashMap<String, Type>,
}

#[derive(Debug, Clone)]
pub enum ExportKind {
    Var(Type),
    Proc(ProcType),
    Mode(Type),
}

#[derive(Debug, Clone)]
pub struct ExportEntry {
    /// Index of the module that owns the implementation. For a spec export this
    /// is the paired body module.
    pub module: usize,
    pub package: usize,
    pub package_name: Option<String>,
    pub module_name: Option<String>,
    pub name: String,
    /// Exported prefix (`GRANT ... PREFIXED ...`). Empty means the automatic
    /// module/package qualifiers.
    pub prefix: Vec<String>,
    pub kind: ExportKind,
    /// `GRANT ... TO ...` friend modules, if restricted.
    pub friends: Option<Vec<String>>,
}

impl ExportEntry {
    pub fn qualified_names(&self) -> Vec<String> {
        if self.prefix.is_empty() {
            let mut names = vec![self.name.clone()];
            if let Some(module) = &self.module_name {
                names.push(format!("{module}!{}", self.name));
                if let Some(package) = &self.package_name {
                    names.push(format!("{package}!{module}!{}", self.name));
                }
            }
            names
        } else {
            let prefix = self.prefix.join("!");
            let mut names = vec![format!("{prefix}!{}", self.name)];
            if let Some(package) = &self.package_name {
                names.push(format!("{package}!{prefix}!{}", self.name));
            }
            names
        }
    }

    fn qualified_prefixes(&self) -> Vec<Vec<String>> {
        if self.prefix.is_empty() {
            let mut prefixes = Vec::new();
            if let Some(module) = &self.module_name {
                prefixes.push(vec![module.clone()]);
                if let Some(package) = &self.package_name {
                    prefixes.push(vec![package.clone(), module.clone()]);
                }
            }
            prefixes
        } else {
            let mut prefixes = vec![self.prefix.clone()];
            if let Some(package) = &self.package_name {
                let mut package_prefix = vec![package.clone()];
                package_prefix.extend(self.prefix.iter().cloned());
                prefixes.push(package_prefix);
            }
            prefixes
        }
    }

    /// Whether the export's `TO` restriction permits this module.
    pub fn is_accessible_to(&self, package: Option<&str>, module: Option<&str>) -> bool {
        let Some(friends) = &self.friends else {
            return true;
        };
        let Some(module) = module else {
            return false;
        };
        let qualified = match package {
            Some(package) => format!("{package}!{module}"),
            None => module.to_owned(),
        };
        friends
            .iter()
            .any(|friend| friend == module || friend == &qualified)
    }
}

/// Resolve a `SEIZE` name against the program's exports.
pub fn resolve_export<'a>(
    exports: &'a [ExportEntry],
    name: &NameRef,
    package: Option<&str>,
    module: Option<&str>,
) -> Result<&'a ExportEntry, String> {
    let candidates: Vec<&ExportEntry> = exports
        .iter()
        .filter(|export| {
            let prefix_matches = if name.prefix.is_empty() {
                export.prefix.is_empty()
            } else {
                export
                    .qualified_prefixes()
                    .iter()
                    .any(|prefix| prefix == &name.prefix)
            };
            export.name == name.name && prefix_matches && export.is_accessible_to(package, module)
        })
        .collect();

    match candidates.as_slice() {
        [] => Err(format!(
            "cannot SEIZE `{}`: no visible module grants it",
            name.qualified()
        )),
        [export] => Ok(export),
        _ => {
            let qualified = candidates
                .iter()
                .flat_map(|export| export.qualified_names())
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!(
                "cannot SEIZE `{}`: ambiguous; use a qualified name ({qualified})",
                name.qualified()
            ))
        }
    }
}

pub fn exports(program: &Program) -> Result<Vec<ExportInfo>, Error> {
    let program = normalize(program)?;
    let info = analyze(&program)?;
    let modes = info.modes;
    let mut exports: Vec<ExportInfo> = info
        .exports
        .into_iter()
        .filter_map(|export| {
            let kind = match export.kind {
                ExportKind::Var(ty) => ExportInfoKind::Var(portable_type(ty, &modes)),
                ExportKind::Proc(signature) => ExportInfoKind::Proc {
                    params: signature
                        .params
                        .into_iter()
                        .map(|ty| portable_type(ty, &modes))
                        .collect(),
                    passing: signature.passing,
                    result: signature.result.map(|ty| portable_type(ty, &modes)),
                },
                ExportKind::Mode(Type::Struct(id)) => {
                    let mode = modes.get(id);
                    let mut forbidden: Vec<String> = mode.forbidden.iter().cloned().collect();
                    forbidden.sort();
                    ExportInfoKind::Mode(ModeInfo {
                        name: mode.name.clone(),
                        implements: Vec::new(),
                        module_name: mode.module_name.clone(),
                        package_name: mode.package_name.clone(),
                        fields: mode
                            .fields
                            .iter()
                            .map(|field| (field.name.clone(), portable_type(field.ty, &modes)))
                            .collect(),
                        forbidden,
                        kind: ModeInfoKind::Struct,
                        components: Vec::new(),
                    })
                }
                ExportKind::Mode(Type::Moreta(id)) => {
                    let mode = modes.get_moreta(id);
                    let components = mode
                        .components
                        .iter()
                        .map(|component| match component {
                            crate::modes::MoretaComponent::Var {
                                name,
                                ty,
                                visibility,
                            } => ModeComponentInfo::Var {
                                name: name.clone(),
                                ty: portable_type(*ty, &modes),
                                public: *visibility == crate::modes::ComponentVisibility::Public,
                            },
                            crate::modes::MoretaComponent::Proc {
                                name,
                                signature,
                                constructor,
                                destructor,
                                visibility,
                                ..
                            } => ModeComponentInfo::Proc {
                                name: name.clone(),
                                params: signature
                                    .params
                                    .iter()
                                    .map(|ty| portable_type(*ty, &modes))
                                    .collect(),
                                passing: signature.passing.clone(),
                                result: signature.result.map(|ty| portable_type(ty, &modes)),
                                constructor: *constructor,
                                destructor: *destructor,
                                public: *visibility == crate::modes::ComponentVisibility::Public,
                            },
                            crate::modes::MoretaComponent::Mode {
                                name,
                                ty,
                                visibility,
                            } => ModeComponentInfo::Mode {
                                name: name.clone(),
                                ty: portable_type(*ty, &modes),
                                public: *visibility == crate::modes::ComponentVisibility::Public,
                            },
                        })
                        .collect();
                    ExportInfoKind::Mode(ModeInfo {
                        name: mode.name.clone(),
                        implements: Vec::new(),
                        module_name: mode.module_name.clone(),
                        package_name: mode.package_name.clone(),
                        fields: Vec::new(),
                        forbidden: Vec::new(),
                        kind: match mode.kind {
                            crate::ast::MoretaKind::Region => ModeInfoKind::Region,
                            crate::ast::MoretaKind::Task => ModeInfoKind::Task,
                            crate::ast::MoretaKind::Interface => ModeInfoKind::Interface,
                            _ => ModeInfoKind::Moreta,
                        },
                        components,
                    })
                }
                ExportKind::Mode(_) => return None,
            };
            Some(ExportInfo {
                name: export.name,
                module_name: export.module_name,
                package_name: export.package_name,
                prefix: export.prefix,
                kind,
                friends: export.friends,
            })
        })
        .collect();
    exports.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.package_name.cmp(&right.package_name))
            .then_with(|| left.module_name.cmp(&right.module_name))
    });
    Ok(exports)
}
