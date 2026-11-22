use crate::ast::{Action, Declaration, Module, ModuleKind};
use crate::lexer::Span;
use crate::types::Type;
use std::collections::HashMap;

use super::{collect_interface, Error, ModuleInterface};

fn collect_nested_specs(sources: &[&Module]) -> Result<Vec<Module>, Error> {
    let mut nested: HashMap<String, Module> = HashMap::new();
    for source in sources {
        let declarations = source.declarations.iter().chain(
            source
                .contexts
                .iter()
                .flat_map(|context| context.declarations.iter()),
        );
        for declaration in declarations {
            let Declaration::SpecModule(spec) = declaration else {
                continue;
            };
            let Some(name) = &spec.name else {
                return Err(Error::new(spec.span, "nested spec module must be named"));
            };
            if nested.insert(name.clone(), (**spec).clone()).is_some() {
                return Err(Error::new(
                    spec.span,
                    format!("duplicate nested spec module `{name}`"),
                ));
            }
        }
    }
    Ok(nested.into_values().collect())
}

pub(super) fn check_nested_specs(
    sources: &[&Module],
    body: &Module,
    span_modes: &HashMap<Span, HashMap<String, Type>>,
) -> Result<(), Error> {
    let nested_specs = collect_nested_specs(sources)?;
    if nested_specs.is_empty() {
        return Ok(());
    }

    let mut nested_bodies: HashMap<String, &Module> = HashMap::new();
    for action in &body.actions {
        let Action::Module(nested) = action else {
            continue;
        };
        if nested.kind == ModuleKind::Body {
            if let Some(name) = &nested.name {
                nested_bodies.insert(name.clone(), nested);
            }
        }
    }

    for nested_spec in &nested_specs {
        let Some(name) = &nested_spec.name else {
            continue;
        };
        let Some(nested_body) = nested_bodies.get(name).copied() else {
            return Err(Error::new(
                nested_spec.span,
                format!("nested spec module `{name}` has no body"),
            ));
        };
        let empty = HashMap::new();
        let spec_modes = span_modes.get(&nested_spec.span).unwrap_or(&empty);
        let body_modes = span_modes.get(&nested_body.span).unwrap_or(&empty);
        let spec_interface = collect_interface(nested_spec, spec_modes)?;
        let body_interface = collect_interface(nested_body, body_modes)?;
        check_interface(&spec_interface, &body_interface, nested_spec.span)?;

        let body_clone = nested_body.clone();
        check_nested_specs(&[nested_spec, &body_clone], nested_body, span_modes)?;
    }
    Ok(())
}

/// Copy `GRANT`/`SEIZE` declarations from nested spec modules into their matching
/// nested body modules. This is the virtual-grant step for nested specs.
pub(super) fn apply_virtual_grants(modules: &mut [Module]) -> Result<(), Error> {
    let mut specs: HashMap<(usize, String), usize> = HashMap::new();
    let mut bodies: Vec<usize> = Vec::new();
    for (index, module) in modules.iter().enumerate() {
        match module.kind {
            ModuleKind::Spec => {
                if let Some(name) = &module.name {
                    specs.insert((module.package, name.clone()), index);
                }
            }
            ModuleKind::Body => bodies.push(index),
            ModuleKind::Region | ModuleKind::SpecRegion => {}
        }
    }

    for body_index in bodies {
        let body_clone = modules[body_index].clone();
        let spec_clone = body_clone
            .name
            .as_ref()
            .and_then(|name| specs.get(&(body_clone.package, name.clone())).copied())
            .map(|spec_index| modules[spec_index].clone());

        let mut sources: Vec<&Module> = Vec::new();
        if let Some(spec) = &spec_clone {
            sources.push(spec);
        }
        sources.push(&body_clone);
        let nested_specs = collect_nested_specs(&sources)?;
        if nested_specs.is_empty() {
            continue;
        }
        apply_virtual_visibility(&nested_specs, &mut modules[body_index])?;
    }
    Ok(())
}

fn apply_virtual_visibility(nested_specs: &[Module], body: &mut Module) -> Result<(), Error> {
    for nested_spec in nested_specs {
        let Some(name) = &nested_spec.name else {
            continue;
        };
        let mut found = None;
        for action in &mut body.actions {
            if let Action::Module(module) = action {
                if module.kind == ModuleKind::Body && module.name.as_deref() == Some(name.as_str())
                {
                    found = Some(module);
                    break;
                }
            }
        }
        let Some(nested_body) = found else {
            continue;
        };

        let visibility = nested_spec
            .declarations
            .iter()
            .filter(|declaration| matches!(declaration, Declaration::Visibility(_)))
            .cloned()
            .collect::<Vec<_>>();

        let body_visibility: Vec<Span> = nested_body
            .declarations
            .iter()
            .filter_map(|declaration| match declaration {
                Declaration::Visibility(visibility) => Some(visibility.span),
                _ => None,
            })
            .collect();
        if !body_visibility.is_empty() {
            let spec_visibility: Vec<Span> = visibility
                .iter()
                .map(|declaration| match declaration {
                    Declaration::Visibility(visibility) => visibility.span,
                    _ => unreachable!(),
                })
                .collect();
            if body_visibility != spec_visibility {
                return Err(Error::new(
                    nested_body.span,
                    format!(
                        "nested body `{name}` must not contain visibility statements; they belong in the nested spec"
                    ),
                ));
            }
        } else {
            nested_body.declarations.extend(visibility);
        }

        let nested_body_clone = nested_body.clone();
        let child_specs = collect_nested_specs(&[nested_spec, &nested_body_clone])?;
        apply_virtual_visibility(&child_specs, nested_body)?;
    }
    Ok(())
}

pub(super) fn check_interface(
    spec: &ModuleInterface,
    body: &ModuleInterface,
    span: Span,
) -> Result<(), Error> {
    for (name, ty) in &spec.vars {
        match body.vars.get(name) {
            Some(body_ty) if body_ty == ty => {}
            Some(_) => {
                return Err(Error::new(
                    span,
                    format!("spec variable `{name}` has a different mode in the body"),
                ));
            }
            None => {
                return Err(Error::new(
                    span,
                    format!("body is missing spec variable `{name}`"),
                ));
            }
        }
    }

    for (name, signature) in &spec.procs {
        match body.procs.get(name) {
            Some(body_signature) if body_signature == signature => {}
            Some(_) => {
                return Err(Error::new(
                    span,
                    format!("spec procedure `{name}` has a different signature in the body"),
                ));
            }
            None => {
                return Err(Error::new(
                    span,
                    format!("body is missing spec procedure `{name}`"),
                ));
            }
        }
    }

    Ok(())
}
