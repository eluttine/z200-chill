use crate::ast::{Action, Declaration, Module, VisibilityKind};
use crate::lexer::Span;
use crate::types::{ProcType, Type};
use std::collections::{HashMap, HashSet};

use super::{resolve_mode_with, signature_from_params_with, Error, ExportEntry, ExportKind};

/// The portable public interface of the program, sorted by name.
pub(super) fn find_remote_context(module: &Module) -> Option<&crate::ast::Context> {
    if let Some(context) = module
        .contexts
        .iter()
        .find(|context| context.remote.is_some())
    {
        return Some(context);
    }
    let mut found = None;
    let _ = walk_nested_modules(&module.actions, &mut |child| {
        if found.is_none() {
            found = find_remote_context(child);
        }
        Ok(())
    });
    found
}

pub(super) fn walk_nested_modules<'a>(
    actions: &'a [Action],
    visit: &mut impl FnMut(&'a Module) -> Result<(), Error>,
) -> Result<(), Error> {
    for action in actions {
        match action {
            Action::Module(module) => visit(module)?,
            Action::If {
                branches,
                else_actions,
                ..
            } => {
                for branch in branches {
                    walk_nested_modules(&branch.body, visit)?;
                }
                if let Some(actions) = else_actions {
                    walk_nested_modules(actions, visit)?;
                }
            }
            Action::Do { body, .. } => walk_nested_modules(body, visit)?,
            Action::Case {
                alternatives,
                else_actions,
                ..
            } => {
                for alternative in alternatives {
                    walk_nested_modules(&alternative.body, visit)?;
                }
                if let Some(actions) = else_actions {
                    walk_nested_modules(actions, visit)?;
                }
            }
            Action::Labelled { action, .. } => {
                walk_nested_modules(std::slice::from_ref(action), visit)?
            }
            Action::Begin { actions, .. } => walk_nested_modules(actions, visit)?,
            Action::Handled { action, .. } => {
                walk_nested_modules(std::slice::from_ref(action), visit)?
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn check_contexts(
    module: &Module,
    index: usize,
    exports: &[ExportEntry],
) -> Result<(), Error> {
    let empty_modes = HashMap::new();
    let mut declared_names = HashSet::new();
    let mut vars: Vec<(String, Type, Span)> = Vec::new();
    let mut procs: Vec<(String, ProcType, Span)> = Vec::new();

    for (context_index, context) in module.contexts.iter().enumerate() {
        let mut current_names = HashSet::new();
        for declaration in &context.declarations {
            match declaration {
                Declaration::Process(_)
                | Declaration::Signal(_)
                | Declaration::GenericProcess(_)
                | Declaration::GenericProcessInstance(_) => {}
                Declaration::Var(list) => {
                    for var in list {
                        let ty = resolve_mode_with(&var.mode, &empty_modes)?;
                        if !declared_names.insert(var.name.clone()) {
                            return Err(Error::new(
                                var.span,
                                format!("duplicate context declaration `{}`", var.name),
                            ));
                        }
                        current_names.insert(var.name.clone());
                        vars.push((var.name.clone(), ty, var.span));
                    }
                }
                Declaration::ProcSignature(signature) => {
                    let ty = signature_from_params_with(
                        &signature.params,
                        &signature.result,
                        &signature.exceptions,
                        &empty_modes,
                    )?;
                    if !declared_names.insert(signature.name.clone()) {
                        return Err(Error::new(
                            signature.span,
                            format!("duplicate context declaration `{}`", signature.name),
                        ));
                    }
                    current_names.insert(signature.name.clone());
                    procs.push((signature.name.clone(), ty, signature.span));
                }
                Declaration::Proc(proc_def) => {
                    let ty = signature_from_params_with(
                        &proc_def.params,
                        &proc_def.result,
                        &proc_def.exceptions,
                        &empty_modes,
                    )?;
                    if !declared_names.insert(proc_def.name.clone()) {
                        return Err(Error::new(
                            proc_def.span,
                            format!("duplicate context declaration `{}`", proc_def.name),
                        ));
                    }
                    current_names.insert(proc_def.name.clone());
                    procs.push((proc_def.name.clone(), ty, proc_def.span));
                }
                Declaration::Mode(_) => {
                    return Err(Error::new(
                        context.span,
                        "mode definitions in contexts are not implemented yet",
                    ));
                }
                Declaration::Visibility(_)
                | Declaration::SpecModule(_)
                | Declaration::GenericProc(_)
                | Declaration::GenericProcInstance(_) => {}
            }
        }

        for declaration in &context.declarations {
            let Declaration::Visibility(visibility) = declaration else {
                continue;
            };
            if context_index == 0 {
                return Err(Error::new(
                    visibility.span,
                    "a context with no enclosing context cannot contain visibility statements",
                ));
            }
            if visibility.prefix.is_some()
                || !visibility.friends.is_empty()
                || visibility.forbid.is_some()
            {
                return Err(Error::new(
                    visibility.span,
                    "advanced clauses in context visibility are not implemented yet",
                ));
            }
            let targets: Vec<String> = if visibility.all {
                if visibility.kind == VisibilityKind::Grant {
                    current_names.iter().cloned().collect()
                } else {
                    declared_names.iter().cloned().collect()
                }
            } else {
                visibility
                    .names
                    .iter()
                    .map(|name| name.name.clone())
                    .collect()
            };
            for name in targets {
                if visibility.kind == VisibilityKind::Grant {
                    if !current_names.contains(&name) {
                        return Err(Error::new(
                            visibility.span,
                            format!("cannot GRANT `{name}` in a context: not declared here"),
                        ));
                    }
                } else if !declared_names.contains(&name) {
                    return Err(Error::new(
                        visibility.span,
                        format!("cannot SEIZE `{name}` in a context: no earlier declaration"),
                    ));
                } else {
                    declared_names.insert(name);
                }
            }
        }
    }

    for (name, declared, span) in vars {
        let export = context_export(exports, module, index, &name, span)?;
        match &export.kind {
            ExportKind::Var(actual) if *actual == declared => {}
            ExportKind::Var(actual) => {
                return Err(Error::new(
                    span,
                    format!(
                        "context declaration `{name}` has mode {} but the enclosing grant has mode {}",
                        declared.describe(),
                        actual.describe()
                    ),
                ));
            }
            _ => {
                return Err(Error::new(
                    span,
                    format!(
                        "context declaration `{name}` is a variable but the enclosing reach grants a different kind of name"
                    ),
                ));
            }
        }
    }

    for (name, declared, span) in procs {
        let export = context_export(exports, module, index, &name, span)?;
        match &export.kind {
            ExportKind::Proc(actual) if *actual == declared => {}
            ExportKind::Proc(_) => {
                return Err(Error::new(
                    span,
                    format!(
                        "context declaration `{name}` has a procedure signature that does not match the enclosing grant"
                    ),
                ));
            }
            _ => {
                return Err(Error::new(
                    span,
                    format!(
                        "context declaration `{name}` is a procedure but the enclosing reach grants a different kind of name"
                    ),
                ));
            }
        }
    }

    Ok(())
}

fn context_export<'a>(
    exports: &'a [ExportEntry],
    module: &Module,
    index: usize,
    name: &str,
    span: Span,
) -> Result<&'a ExportEntry, Error> {
    let candidates: Vec<&ExportEntry> = exports
        .iter()
        .filter(|export| export.module != index)
        .filter(|export| export.name == name && export.prefix.is_empty())
        .filter(|export| {
            export.is_accessible_to(module.package_name.as_deref(), module.name.as_deref())
        })
        .collect();
    match candidates.as_slice() {
        [] => Err(Error::new(
            span,
            format!("context declaration `{name}` has no matching grant in the enclosing reach"),
        )),
        [export] => Ok(export),
        _ => Err(Error::new(
            span,
            format!("context declaration `{name}` is ambiguous; several modules grant it"),
        )),
    }
}
