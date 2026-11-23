use crate::ast::{Declaration, Module, NameRef, PrefixClause, VisibilityKind};
use crate::lexer::Span;
use crate::modes::{ModeTable, StructMode};
use crate::types::Type;
use std::collections::{HashMap, HashSet};

use super::{Error, ExportEntry, ExportKind, ModuleInterface};

pub(super) fn has_grant(module: &Module) -> bool {
    module.declarations.iter().any(|declaration| {
        matches!(declaration, Declaration::Visibility(v) if v.kind == VisibilityKind::Grant)
    })
}

/// Check context-list quasi declarations against the grants visible in the
/// module's enclosing reach. Contexts have no runtime effect; in this complete
/// program model they act as static assertions about the surrounding reach.
pub fn seize_old_prefix(prefix: &Option<PrefixClause>) -> Result<&[String], String> {
    match prefix {
        None | Some(PrefixClause::Prefixed(None)) => Ok(&[]),
        Some(PrefixClause::Prefixed(Some(prefix))) => Ok(prefix),
        Some(PrefixClause::Rename { old, new }) => {
            if new.is_empty() {
                Ok(old)
            } else {
                Err("SEIZE with a non-empty new prefix is not implemented yet".to_owned())
            }
        }
    }
}

/// Resolve a `SEIZE` postfix to its source name and the local simple name.
pub fn seize_name(
    name: &NameRef,
    prefix: &Option<PrefixClause>,
) -> Result<(NameRef, String), String> {
    let old = seize_old_prefix(prefix)?;
    let mut source_prefix = old.to_vec();
    source_prefix.extend(name.prefix.iter().cloned());
    Ok((
        NameRef {
            prefix: source_prefix,
            name: name.name.clone(),
            span: name.span,
        },
        name.name.clone(),
    ))
}

pub(crate) fn grant_prefixes(
    visibility: &crate::ast::Visibility,
    module: &Module,
) -> Result<(Vec<String>, Vec<String>), Error> {
    match &visibility.prefix {
        None => Ok((Vec::new(), Vec::new())),
        Some(PrefixClause::Prefixed(Some(prefix))) => Ok((Vec::new(), prefix.clone())),
        Some(PrefixClause::Prefixed(None)) => {
            let name = module.name.clone().ok_or_else(|| {
                Error::new(
                    visibility.span,
                    "GRANT ... PREFIXED without a prefix requires a named module",
                )
            })?;
            Ok((Vec::new(), vec![name]))
        }
        Some(PrefixClause::Rename { old, new }) => Ok((old.clone(), new.clone())),
    }
}

pub(crate) fn apply_forbid(
    mode: &mut StructMode,
    name: &str,
    forbid: &crate::ast::Forbid,
    span: Span,
) -> Result<(), Error> {
    let fields: Vec<String> = if forbid.all {
        mode.fields.iter().map(|field| field.name.clone()).collect()
    } else {
        forbid.fields.clone()
    };
    for field in fields {
        if !mode.fields.iter().any(|info| info.name == field) {
            return Err(Error::new(
                span,
                format!("FORBID field `{field}` is not a field of `{name}`"),
            ));
        }
        mode.forbidden.insert(field);
    }
    Ok(())
}

pub(super) fn add_grants(
    module: &Module,
    interface: &ModuleInterface,
    export_module: usize,
    exports: &mut Vec<ExportEntry>,
    modes: &mut ModeTable,
    reach_exports: &HashMap<String, ExportKind>,
) -> Result<(), Error> {
    let module_name = module.name.clone();
    let package = module.package;
    let package_name = module.package_name.clone();
    let mut granted = HashSet::new();

    for declaration in &module.declarations {
        let Declaration::Visibility(visibility) = declaration else {
            continue;
        };
        if visibility.kind != VisibilityKind::Grant {
            continue;
        }

        let friends = if visibility.friends.is_empty() {
            None
        } else {
            Some(
                visibility
                    .friends
                    .iter()
                    .map(|friend| friend.qualified())
                    .collect::<Vec<_>>(),
            )
        };
        let (old_prefix, export_prefix) = grant_prefixes(visibility, module)?;

        let names: Vec<(String, Span)> = if visibility.all {
            if old_prefix.is_empty() {
                let mut names: Vec<String> = interface
                    .vars
                    .keys()
                    .chain(interface.procs.keys())
                    .chain(
                        interface
                            .modes
                            .iter()
                            .filter(|(_, ty)| matches!(ty, Type::Struct(_)))
                            .map(|(name, _)| name),
                    )
                    .cloned()
                    .collect();
                names.sort();
                names.dedup();
                names
                    .into_iter()
                    .map(|name| (name, visibility.span))
                    .collect()
            } else {
                // Only prefixed names in the module's reach match; the subset
                // has no such grantable names (nested modules are unsupported).
                Vec::new()
            }
        } else {
            let mut names = Vec::with_capacity(visibility.names.len());
            for name in &visibility.names {
                if !old_prefix.is_empty() || !name.prefix.is_empty() {
                    let mut source = old_prefix.clone();
                    source.extend(name.prefix.iter().cloned());
                    return Err(Error::new(
                        name.span,
                        format!(
                            "cannot GRANT `{}`: prefixed names are not declared in this module",
                            format_args!("{}!{}", source.join("!"), name.name)
                        ),
                    ));
                }
                names.push((name.name.clone(), name.span));
            }
            names
        };

        for (name, span) in names {
            let grant_key = format!("{}!{name}", old_prefix.join("!"));
            if !granted.insert(grant_key) {
                return Err(Error::new(
                    span,
                    format!("`{name}` is granted more than once"),
                ));
            }

            let kind = if let Some(mode) = interface.modes.get(&name) {
                if !matches!(mode, Type::Struct(_) | Type::Set(_) | Type::Moreta(_)) {
                    return Err(Error::new(
                        span,
                        format!(
                            "cannot GRANT `{name}`: only structure, set, and module modes are supported as mode grants"
                        ),
                    ));
                }
                if let Some(forbid) = &visibility.forbid {
                    let Type::Struct(id) = mode else {
                        return Err(Error::new(
                            span,
                            format!("FORBID requires a structure mode, not `{name}`"),
                        ));
                    };
                    apply_forbid(modes.get_mut(*id), &name, forbid, span)?;
                }
                ExportKind::Mode(*mode)
            } else if let Some(ty) = interface.vars.get(&name) {
                if visibility.forbid.is_some() {
                    return Err(Error::new(
                        span,
                        format!("FORBID requires a structure mode, not `{name}`"),
                    ));
                }
                ExportKind::Var(*ty)
            } else if let Some(signature) = interface.procs.get(&name) {
                if visibility.forbid.is_some() {
                    return Err(Error::new(
                        span,
                        format!("FORBID requires a structure mode, not `{name}`"),
                    ));
                }
                ExportKind::Proc(signature.clone())
            } else if let Some(kind) = reach_exports.get(&name) {
                match kind {
                    ExportKind::Mode(ty) => {
                        if let Some(forbid) = &visibility.forbid {
                            let Type::Struct(id) = ty else {
                                return Err(Error::new(
                                    span,
                                    format!("FORBID requires a structure mode, not `{name}`"),
                                ));
                            };
                            apply_forbid(modes.get_mut(*id), &name, forbid, span)?;
                        }
                        ExportKind::Mode(*ty)
                    }
                    ExportKind::Proc(signature) => {
                        if visibility.forbid.is_some() {
                            return Err(Error::new(
                                span,
                                format!("FORBID requires a structure mode, not `{name}`"),
                            ));
                        }
                        ExportKind::Proc(signature.clone())
                    }
                    ExportKind::Var(ty) => {
                        if visibility.forbid.is_some() {
                            return Err(Error::new(
                                span,
                                format!("FORBID requires a structure mode, not `{name}`"),
                            ));
                        }
                        ExportKind::Var(*ty)
                    }
                }
            } else {
                return Err(Error::new(
                    span,
                    format!("cannot GRANT `{name}`: not declared in this module"),
                ));
            };

            exports.push(ExportEntry {
                module: export_module,
                package,
                package_name: package_name.clone(),
                module_name: module_name.clone(),
                name,
                prefix: export_prefix.clone(),
                kind,
                friends: friends.clone(),
            });
        }
    }

    Ok(())
}
