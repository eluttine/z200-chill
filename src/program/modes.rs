use crate::ast::{Declaration, ModeRef, Module, Param, Program, VisibilityKind};
use crate::lexer::Span;
use crate::modes::{FieldInfo, ModeTable, StructMode};
use crate::types::{self, ModeId, ProcType, Type};
use std::collections::{HashMap, HashSet};

use super::{walk_nested_modules, Error, ModuleInterface, MoretaSource};

type ModeBuildResult = (
    ModeTable,
    Vec<HashMap<String, Type>>,
    HashMap<Span, HashMap<String, Type>>,
    HashMap<crate::types::MoretaId, MoretaSource>,
);

pub(super) fn build_modes(program: &Program) -> Result<ModeBuildResult, Error> {
    let mut table = ModeTable::default();
    let mut top_maps = Vec::with_capacity(program.modules.len());
    let mut span_maps = HashMap::new();
    let mut moreta_sources = HashMap::new();

    for (index, module) in program.modules.iter().enumerate() {
        let mut local = HashMap::new();
        let mut resolution = HashMap::new();
        process_module_modes(
            module,
            index,
            module.package,
            module.package_name.clone(),
            &mut local,
            &mut resolution,
            &mut table,
            &mut span_maps,
            &mut moreta_sources,
        )?;
        top_maps.push(local);
    }

    Ok((table, top_maps, span_maps, moreta_sources))
}

#[allow(clippy::too_many_arguments)]
fn process_module_modes(
    module: &Module,
    top: usize,
    top_package: usize,
    top_package_name: Option<String>,
    local: &mut HashMap<String, Type>,
    resolution: &mut HashMap<String, Type>,
    table: &mut ModeTable,
    span_maps: &mut HashMap<Span, HashMap<String, Type>>,
    moreta_sources: &mut HashMap<crate::types::MoretaId, MoretaSource>,
) -> Result<(), Error> {
    for declaration in &module.declarations {
        let Declaration::Mode(mode_def) = declaration else {
            continue;
        };

        let ty = build_mode_definition(
            mode_def,
            top,
            top_package,
            top_package_name.clone(),
            module.name.clone(),
            resolution,
            table,
            moreta_sources,
        )?;
        if let Type::Moreta(id) = ty {
            for component in &table.moretas[id.0].components {
                if let crate::modes::MoretaComponent::Mode { name, ty } = component {
                    if name.contains('!') {
                        local.insert(name.clone(), *ty);
                    }
                }
            }
        }

        if let Some(existing) = local.get(&mode_def.name) {
            if *existing != ty {
                return Err(Error::new(
                    mode_def.span,
                    format!("duplicate mode `{}`", mode_def.name),
                ));
            }
        } else {
            local.insert(mode_def.name.clone(), ty);
        }
        resolution.insert(mode_def.name.clone(), ty);
    }

    span_maps.insert(module.span, local.clone());

    walk_nested_modules(&module.actions, &mut |child| {
        let mut child_local = HashMap::new();
        let mut child_resolution = resolution.clone();
        process_module_modes(
            child,
            top,
            top_package,
            top_package_name.clone(),
            &mut child_local,
            &mut child_resolution,
            table,
            span_maps,
            moreta_sources,
        )
    })
}

#[allow(clippy::too_many_arguments)]
fn build_mode_definition(
    mode_def: &crate::ast::ModeDefinition,
    top: usize,
    top_package: usize,
    top_package_name: Option<String>,
    module_name: Option<String>,
    resolution: &mut HashMap<String, Type>,
    table: &mut ModeTable,
    moreta_sources: &mut HashMap<crate::types::MoretaId, MoretaSource>,
) -> Result<Type, Error> {
    let ty = match &mode_def.body {
        crate::ast::ModeDefinitionBody::Struct(fields) => {
            // Predeclare the structure so fields may refer to it (directly
            // or through REF), then resolve and install the fields.
            let id = ModeId(table.modes.len());
            table.modes.push(StructMode {
                name: mode_def.name.clone(),
                package: top_package,
                module: top,
                package_name: top_package_name.clone(),
                module_name: module_name.clone(),
                fields: Vec::new(),
                forbidden: std::collections::HashSet::new(),
            });
            let ty = Type::Struct(id);
            resolution.insert(mode_def.name.clone(), ty);

            let mut field_infos = Vec::with_capacity(fields.len());
            let mut names = std::collections::HashSet::new();
            for field in fields {
                if !names.insert(field.name.clone()) {
                    return Err(Error::new(
                        field.span,
                        format!("duplicate field `{}`", field.name),
                    ));
                }
                let ty = resolve_mode_with(&field.mode, resolution)?;
                field_infos.push(FieldInfo {
                    name: field.name.clone(),
                    ty,
                });
            }
            table.modes[id.0].fields = field_infos;
            Type::Struct(id)
        }
        crate::ast::ModeDefinitionBody::Set(elements) => {
            let mut names = HashSet::new();
            let mut values = HashSet::new();
            let mut infos = Vec::with_capacity(elements.len());
            for (index, element) in elements.iter().enumerate() {
                if !names.insert(element.name.clone()) {
                    return Err(Error::new(
                        element.span,
                        format!("duplicate set element `{}`", element.name),
                    ));
                }
                let value = element.value.unwrap_or(index as i128);
                if !values.insert(value) {
                    return Err(Error::new(
                        element.span,
                        format!("duplicate set representation value `{value}`"),
                    ));
                }
                infos.push((element.name.clone(), value));
            }
            let id = crate::types::SetId(table.sets.len());
            table.sets.push(crate::modes::SetMode {
                name: mode_def.name.clone(),
                package: top_package,
                module: top,
                package_name: top_package_name.clone(),
                module_name: module_name.clone(),
                elements: infos,
            });
            Type::Set(id)
        }
        crate::ast::ModeDefinitionBody::Moreta {
            spec,
            bases,
            declarations,
            invariant,
            ..
        } => {
            let mut base_ids = Vec::new();
            let mut components: Vec<crate::modes::MoretaComponent> = Vec::new();
            for base in bases {
                let base_ref = ModeRef {
                    name: base.clone(),
                    arrays: Vec::new(),
                    string_length: None,
                    varying: false,
                    access: false,
                    powerset: false,
                    reference: false,
                    row: false,
                    access_indexed: false,
                    access_index: None,
                    span: mode_def.span,
                };
                let Type::Moreta(base_id) = resolve_mode_with(&base_ref, resolution)? else {
                    return Err(Error::new(
                        mode_def.span,
                        format!("module mode base `{base}` is not a module mode"),
                    ));
                };
                base_ids.push(base_id);
                for component in &table.moretas[base_id.0].components {
                    if !components
                        .iter()
                        .any(|existing| existing.name() == component.name())
                    {
                        components.push(component.clone());
                    }
                }
            }
            let mut seen = HashSet::new();
            for declaration in declarations {
                match declaration {
                    Declaration::Var(vars) => {
                        for var in vars {
                            let ty = resolve_mode_with(&var.mode, resolution)?;
                            if !seen.insert(var.name.clone()) {
                                return Err(Error::new(
                                    var.span,
                                    format!("duplicate component `{}`", var.name),
                                ));
                            }
                            components.retain(|component| component.name() != var.name);
                            components.push(crate::modes::MoretaComponent::Var {
                                name: var.name.clone(),
                                ty,
                            });
                        }
                    }
                    Declaration::Proc(proc_def) => {
                        let signature = signature_from_params_with(
                            &proc_def.params,
                            &proc_def.result,
                            &proc_def.exceptions,
                            resolution,
                        )?;
                        if !seen.insert(proc_def.name.clone()) {
                            return Err(Error::new(
                                proc_def.span,
                                format!("duplicate component `{}`", proc_def.name),
                            ));
                        }
                        components.retain(|component| component.name() != proc_def.name);
                        components.push(crate::modes::MoretaComponent::Proc {
                            name: proc_def.name.clone(),
                            signature,
                            span: proc_def.span,
                            constructor: proc_def.constructor,
                            destructor: proc_def.destructor,
                        });
                    }
                    Declaration::ProcSignature(signature) => {
                        let ty = signature_from_params_with(
                            &signature.params,
                            &signature.result,
                            &signature.exceptions,
                            resolution,
                        )?;
                        if !seen.insert(signature.name.clone()) {
                            return Err(Error::new(
                                signature.span,
                                format!("duplicate component `{}`", signature.name),
                            ));
                        }
                        components.retain(|component| component.name() != signature.name);
                        components.push(crate::modes::MoretaComponent::Proc {
                            name: signature.name.clone(),
                            signature: ty,
                            span: signature.span,
                            constructor: signature.constructor,
                            destructor: signature.destructor,
                        });
                    }
                    Declaration::Mode(nested) => {
                        let ty = build_mode_definition(
                            nested,
                            top,
                            top_package,
                            top_package_name.clone(),
                            module_name.clone(),
                            resolution,
                            table,
                            moreta_sources,
                        )?;
                        for (prefix, ty) in [
                            (nested.name.clone(), ty),
                            (format!("{}!{}", mode_def.name, nested.name), ty),
                        ] {
                            components.retain(|component| component.name() != prefix);
                            components.push(crate::modes::MoretaComponent::Mode {
                                name: prefix.clone(),
                                ty,
                            });
                            resolution.insert(prefix, ty);
                        }
                    }
                    _ => {
                        return Err(Error::new(
                            mode_def.span,
                            "unsupported module mode component",
                        ));
                    }
                }
            }
            let constructors: Vec<_> = components
                .iter()
                .filter(|component| {
                    matches!(
                        component,
                        crate::modes::MoretaComponent::Proc {
                            constructor: true,
                            ..
                        }
                    )
                })
                .collect();
            let destructors: Vec<_> = components
                .iter()
                .filter(|component| {
                    matches!(
                        component,
                        crate::modes::MoretaComponent::Proc {
                            destructor: true,
                            ..
                        }
                    )
                })
                .collect();
            if constructors.len() > 1 {
                return Err(Error::new(
                    mode_def.span,
                    "a module mode may have at most one constructor",
                ));
            }
            if destructors.len() > 1 {
                return Err(Error::new(
                    mode_def.span,
                    "a module mode may have at most one destructor",
                ));
            }
            if let Some(crate::modes::MoretaComponent::Proc {
                signature, span, ..
            }) = constructors.first()
            {
                if signature.result.is_some() {
                    return Err(Error::new(*span, "a constructor cannot return a value"));
                }
            }
            if let Some(crate::modes::MoretaComponent::Proc {
                signature, span, ..
            }) = destructors.first()
            {
                if signature.result.is_some() || !signature.params.is_empty() {
                    return Err(Error::new(
                        *span,
                        "a destructor cannot take parameters or return a value",
                    ));
                }
            }
            let id = crate::types::MoretaId(table.moretas.len());
            moreta_sources.insert(
                id,
                MoretaSource {
                    declarations: declarations.clone(),
                    invariant: invariant.clone(),
                },
            );
            table.moretas.push(crate::modes::MoretaMode {
                name: mode_def.name.clone(),
                package: top_package,
                module: top,
                package_name: top_package_name.clone(),
                module_name: module_name.clone(),
                bases: base_ids,
                abstract_mode: *spec,
                components,
            });
            Type::Moreta(id)
        }
        crate::ast::ModeDefinitionBody::Alias(mode) => resolve_mode_with(mode, resolution)?,
    };
    Ok(ty)
}

pub(crate) fn resolve_mode_with(
    mode: &ModeRef,
    mode_names: &HashMap<String, Type>,
) -> Result<Type, Error> {
    types::resolve_mode_ref(mode, mode_names).ok_or_else(|| {
        if mode.powerset {
            Error::new(
                mode.span,
                "POWERSET member mode must be a scalar mode",
            )
        } else {
            Error::new(
                mode.span,
                format!(
                    "unknown mode `{}`; array elements must be scalar modes or the mode must be previously defined",
                    mode.name
                ),
            )
        }
    })
}

pub fn collect_interface(
    module: &Module,
    mode_names: &HashMap<String, Type>,
) -> Result<ModuleInterface, Error> {
    let mut interface = ModuleInterface {
        modes: mode_names.clone(),
        ..ModuleInterface::default()
    };

    // A mode brought in by `SEIZE` is not known while interfaces are collected;
    // the mode checker resolves those names later. Only those specific names may be
    // skipped here, so genuine typos still fail.
    let seized: HashSet<&str> = module
        .declarations
        .iter()
        .filter_map(|declaration| match declaration {
            Declaration::Visibility(visibility) if visibility.kind == VisibilityKind::Seize => {
                Some(visibility)
            }
            _ => None,
        })
        .flat_map(|visibility| visibility.names.iter())
        .map(|name| name.name.as_str())
        .collect();

    for declaration in &module.declarations {
        match declaration {
            Declaration::Process(_) => {}
            Declaration::Var(list) => {
                for var in list {
                    let ty = match resolve_mode_with(&var.mode, mode_names) {
                        Ok(ty) => ty,
                        Err(_) if seized.contains(var.mode.name.as_str()) => continue,
                        Err(error) => return Err(error),
                    };
                    if interface.vars.insert(var.name.clone(), ty).is_some()
                        || interface.procs.contains_key(&var.name)
                    {
                        return Err(Error::new(
                            var.span,
                            format!("duplicate declaration of `{}`", var.name),
                        ));
                    }
                }
            }
            Declaration::Proc(proc_def) => {
                let signature = match signature_from_params_with(
                    &proc_def.params,
                    &proc_def.result,
                    &proc_def.exceptions,
                    mode_names,
                ) {
                    Ok(signature) => signature,
                    Err(_) if has_seized_mode(&proc_def.params, &proc_def.result, &seized) => {
                        continue
                    }
                    Err(error) => return Err(error),
                };
                if interface
                    .procs
                    .insert(proc_def.name.clone(), signature)
                    .is_some()
                    || interface.vars.contains_key(&proc_def.name)
                {
                    return Err(Error::new(
                        proc_def.span,
                        format!("duplicate declaration of `{}`", proc_def.name),
                    ));
                }
            }
            Declaration::ProcSignature(signature) => {
                let proc_sig = match signature_from_params_with(
                    &signature.params,
                    &signature.result,
                    &signature.exceptions,
                    mode_names,
                ) {
                    Ok(proc_sig) => proc_sig,
                    Err(_) if has_seized_mode(&signature.params, &signature.result, &seized) => {
                        continue
                    }
                    Err(error) => return Err(error),
                };
                if interface
                    .procs
                    .insert(signature.name.clone(), proc_sig)
                    .is_some()
                    || interface.vars.contains_key(&signature.name)
                {
                    return Err(Error::new(
                        signature.span,
                        format!("duplicate declaration of `{}`", signature.name),
                    ));
                }
            }
            Declaration::Mode(_)
            | Declaration::Visibility(_)
            | Declaration::SpecModule(_)
            | Declaration::GenericProc(_)
            | Declaration::GenericProcInstance(_) => {}
        }
    }

    Ok(interface)
}

fn has_seized_mode(params: &[Param], result: &Option<ModeRef>, seized: &HashSet<&str>) -> bool {
    params
        .iter()
        .any(|param| seized.contains(param.mode.name.as_str()))
        || result
            .as_ref()
            .is_some_and(|mode| seized.contains(mode.name.as_str()))
}

pub fn signature_from_params(
    params: &[Param],
    result: &Option<ModeRef>,
    exceptions: &[String],
) -> Result<ProcType, Error> {
    signature_from_params_with(params, result, exceptions, &HashMap::new())
}

pub fn signature_from_params_with(
    params: &[Param],
    result: &Option<ModeRef>,
    exceptions: &[String],
    mode_names: &HashMap<String, Type>,
) -> Result<ProcType, Error> {
    let mut param_types = Vec::with_capacity(params.len());
    let mut passing = Vec::with_capacity(params.len());
    let mut seen = HashSet::new();

    for param in params {
        passing.push(param.passing);
        if !seen.insert(param.name.as_str()) {
            return Err(Error::new(
                param.span,
                format!("duplicate parameter `{}`", param.name),
            ));
        }
        param_types.push(resolve_mode_with(&param.mode, mode_names)?);
    }

    let result_type = match result {
        Some(mode) => Some(resolve_mode_with(mode, mode_names)?),
        None => None,
    };

    Ok(ProcType {
        params: param_types,
        passing,
        result: result_type,
        exceptions: Some(exceptions.to_vec()),
    })
}

pub fn resolve_mode(mode: &ModeRef) -> Result<Type, Error> {
    types::resolve_mode(&mode.name).ok_or_else(|| {
        Error::new(
            mode.span,
            format!(
                "unknown mode `{}`; v0 supports SHORT_INT, INT, LONG_INT, BOOL, CHAR, and CHARS",
                mode.name
            ),
        )
    })
}
