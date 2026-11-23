use crate::ast::{Action, Declaration, Expr, ModeDefinitionBody, ModeRef, Module, Program};
use crate::lexer::Span;
use std::collections::{HashMap, HashSet};

use super::{apply_virtual_grants, Error};

pub fn normalize(program: &Program) -> Result<Program, Error> {
    let mut normalized = program.clone();
    apply_virtual_grants(&mut normalized.modules)?;
    for module in &mut normalized.modules {
        expand_module_generics(module)?;
        expand_mode_generics(module)?;
        merge_moreta_specs(module)?;
    }
    expand_generic_modules(&mut normalized)?;
    Ok(normalized)
}

fn merge_moreta_specs(module: &mut Module) -> Result<(), Error> {
    let mut specs = HashMap::new();
    let mut bodies = HashMap::new();
    for (index, declaration) in module.declarations.iter().enumerate() {
        if let Declaration::Mode(mode_def) = declaration {
            if let crate::ast::ModeDefinitionBody::Moreta { kind, spec, .. } = &mode_def.body {
                if *kind == crate::ast::MoretaKind::Interface {
                    continue;
                }
                if *spec {
                    specs.insert(mode_def.name.clone(), index);
                } else {
                    bodies.insert(mode_def.name.clone(), index);
                }
            }
        }
    }
    let mut remove = HashSet::new();
    for (name, body_index) in &bodies {
        let Some(spec_index) = specs.get(name).copied() else {
            continue;
        };
        let body_mode = match module.declarations[*body_index].clone() {
            Declaration::Mode(mode_def) => mode_def,
            _ => unreachable!(),
        };
        let spec_mode = match module.declarations[spec_index].clone() {
            Declaration::Mode(mode_def) => mode_def,
            _ => unreachable!(),
        };
        let body_declarations = match body_mode.body {
            crate::ast::ModeDefinitionBody::Moreta { declarations, .. } => declarations,
            _ => unreachable!(),
        };
        let (spec_declarations, spec_bases, spec_invariant) = match spec_mode.body {
            crate::ast::ModeDefinitionBody::Moreta {
                declarations,
                bases,
                invariant,
                ..
            } => (declarations, bases, invariant),
            _ => unreachable!(),
        };
        let body_proc_names: HashSet<String> = body_declarations
            .iter()
            .filter_map(|declaration| match declaration {
                Declaration::Proc(proc_def) => Some(proc_def.name.clone()),
                _ => None,
            })
            .collect();
        for declaration in &spec_declarations {
            if let Declaration::ProcSignature(signature) = declaration {
                if !body_proc_names.contains(&signature.name) {
                    return Err(Error::new(
                        signature.span,
                        format!(
                            "module mode body `{name}` is missing procedure component `{}`",
                            signature.name
                        ),
                    ));
                }
            }
        }
        let body_names: HashSet<String> = body_declarations
            .iter()
            .filter_map(declaration_name)
            .collect();
        let spec_components: Vec<String> = spec_declarations
            .iter()
            .filter_map(declaration_name)
            .collect();
        let mut merged: Vec<Declaration> = spec_declarations
            .into_iter()
            .filter(|declaration| {
                declaration_name(declaration).is_none_or(|name| !body_names.contains(&name))
            })
            .collect();
        merged.extend(body_declarations);
        if let Declaration::Mode(mode_def) = &mut module.declarations[*body_index] {
            if let crate::ast::ModeDefinitionBody::Moreta {
                spec,
                bases,
                spec_components: components,
                declarations,
                invariant,
                ..
            } = &mut mode_def.body
            {
                *spec = false;
                *components = spec_components;
                if bases.is_empty() {
                    *bases = spec_bases;
                }
                *declarations = merged;
                *invariant = match (spec_invariant, invariant.take()) {
                    (Some(spec_invariant), Some(body_invariant)) => Some(Expr::Binary {
                        op: crate::ast::BinaryOp::And,
                        span: body_invariant.span(),
                        left: Box::new(spec_invariant),
                        right: Box::new(body_invariant),
                    }),
                    (Some(spec_invariant), None) => Some(spec_invariant),
                    (None, body_invariant) => body_invariant,
                };
            }
        }
        remove.insert(spec_index);
    }
    let mut index = 0;
    module.declarations.retain(|_| {
        let keep = !remove.contains(&index);
        index += 1;
        keep
    });
    for action in &mut module.actions {
        merge_moreta_specs_in_action(action)?;
    }
    Ok(())
}

fn declaration_name(declaration: &Declaration) -> Option<String> {
    match declaration {
        Declaration::Var(vars) => vars.first().map(|var| var.name.clone()),
        Declaration::Proc(proc_def) => Some(proc_def.name.clone()),
        Declaration::ProcSignature(signature) => Some(signature.name.clone()),
        _ => None,
    }
}

fn merge_moreta_specs_in_action(action: &mut Action) -> Result<(), Error> {
    match action {
        Action::Module(module) => merge_moreta_specs(module),
        Action::Begin { actions, .. } | Action::Do { body: actions, .. } => {
            for action in actions {
                merge_moreta_specs_in_action(action)?;
            }
            Ok(())
        }
        Action::If {
            branches,
            else_actions,
            ..
        } => {
            for branch in branches {
                for action in &mut branch.body {
                    merge_moreta_specs_in_action(action)?;
                }
            }
            if let Some(actions) = else_actions {
                for action in actions {
                    merge_moreta_specs_in_action(action)?;
                }
            }
            Ok(())
        }
        Action::Case {
            alternatives,
            else_actions,
            ..
        } => {
            for alternative in alternatives {
                for action in &mut alternative.body {
                    merge_moreta_specs_in_action(action)?;
                }
            }
            if let Some(actions) = else_actions {
                for action in actions {
                    merge_moreta_specs_in_action(action)?;
                }
            }
            Ok(())
        }
        Action::Labelled { action, .. } => merge_moreta_specs_in_action(action),
        Action::Handled { handler, .. } => {
            for alternative in &mut handler.alternatives {
                for action in &mut alternative.actions {
                    merge_moreta_specs_in_action(action)?;
                }
            }
            if let Some(actions) = &mut handler.else_actions {
                for action in actions {
                    merge_moreta_specs_in_action(action)?;
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn expand_mode_generics(module: &mut Module) -> Result<(), Error> {
    let mut templates: HashMap<String, (Vec<crate::ast::GenericFormal>, ModeDefinitionBody)> =
        HashMap::new();
    for declaration in &module.declarations {
        if let Declaration::Mode(mode_def) = declaration {
            if let ModeDefinitionBody::Generic { formals, body, .. } = &mode_def.body {
                if templates
                    .insert(mode_def.name.clone(), (formals.clone(), (**body).clone()))
                    .is_some()
                {
                    return Err(Error::new(
                        mode_def.span,
                        format!("duplicate generic mode `{}`", mode_def.name),
                    ));
                }
            }
        }
    }
    let mut declarations = Vec::with_capacity(module.declarations.len());
    for declaration in module.declarations.drain(..) {
        match declaration {
            Declaration::Mode(mut mode_def) => match mode_def.body {
                ModeDefinitionBody::Generic { .. } => {}
                ModeDefinitionBody::GenericInstance {
                    template: instance_template,
                    actuals,
                    span,
                } => {
                    let Some((formals, body)) = templates.get(&instance_template) else {
                        return Err(Error::new(
                            span,
                            format!("unknown generic mode `{instance_template}`"),
                        ));
                    };
                    let (substitutions, syn_substitutions, proc_substitutions) =
                        generic_substitutions(formals, &actuals, span, &instance_template)?;
                    let mut concrete = body.clone();
                    substitute_mode_definition_body(
                        &mut concrete,
                        &substitutions,
                        &syn_substitutions,
                        &proc_substitutions,
                    )?;
                    mode_def.body = concrete;
                    mode_def.span = span;
                    declarations.push(Declaration::Mode(mode_def));
                }
                _ => declarations.push(Declaration::Mode(mode_def)),
            },
            other => declarations.push(other),
        }
    }
    module.declarations = declarations;
    Ok(())
}

fn substitute_mode_definition_body(
    body: &mut ModeDefinitionBody,
    substitutions: &HashMap<&str, &ModeRef>,
    syns: &HashMap<&str, &Expr>,
    procs: &HashMap<&str, &str>,
) -> Result<(), Error> {
    match body {
        ModeDefinitionBody::Struct(fields) => {
            for field in fields {
                substitute_mode(&mut field.mode, substitutions)?;
            }
        }
        ModeDefinitionBody::Set(_) => {}
        ModeDefinitionBody::Moreta {
            declarations,
            invariant,
            ..
        } => {
            substitute_declaration_modes(declarations, substitutions)?;
            substitute_declarations_syns(declarations, syns, procs);
            if let Some(invariant) = invariant {
                *invariant = substitute_expr_syns(invariant, syns, procs);
            }
        }
        ModeDefinitionBody::Alias(mode) => substitute_mode(mode, substitutions)?,
        ModeDefinitionBody::Generic { .. } => {
            return Err(Error::new(
                crate::lexer::Span::new(crate::lexer::SourceId(0), 0, 0),
                "nested generic mode templates are not implemented",
            ));
        }
        ModeDefinitionBody::GenericInstance { .. } => {
            return Err(Error::new(
                crate::lexer::Span::new(crate::lexer::SourceId(0), 0, 0),
                "nested generic mode instances are not implemented",
            ));
        }
    }
    Ok(())
}

fn expand_generic_modules(program: &mut Program) -> Result<(), Error> {
    if program.generic_module_instances.is_empty() {
        program.generic_modules.clear();
        return Ok(());
    }
    let mut templates: HashMap<String, crate::ast::GenericModuleTemplate> = HashMap::new();
    for template in &program.generic_modules {
        if templates
            .insert(template.name.clone(), template.clone())
            .is_some()
        {
            return Err(Error::new(
                template.span,
                format!("duplicate generic module template `{}`", template.name),
            ));
        }
    }
    let instances = std::mem::take(&mut program.generic_module_instances);
    for instance in instances {
        let Some(template) = templates.get(&instance.template) else {
            return Err(Error::new(
                instance.span,
                format!("unknown generic module template `{}`", instance.template),
            ));
        };
        let (substitutions, syn_substitutions, proc_substitutions) = generic_substitutions(
            &template.formal_params,
            &instance.actuals,
            instance.span,
            &instance.template,
        )?;
        if template.module.kind != instance.kind {
            return Err(Error::new(
                instance.span,
                format!(
                    "generic `{}` instance `{}` has the wrong unit kind",
                    instance.template, instance.name
                ),
            ));
        }
        let mut module = template.module.clone();
        module.name = Some(instance.name.clone());
        module.kind = instance.kind;
        module.span = instance.span;
        substitute_module_modes(&mut module, &substitutions)?;
        substitute_module_syns(&mut module, &syn_substitutions, &proc_substitutions);
        program.modules.push(module);
    }
    program.generic_modules.clear();
    Ok(())
}

fn substitute_module_modes(
    module: &mut Module,
    substitutions: &HashMap<&str, &ModeRef>,
) -> Result<(), Error> {
    substitute_declaration_modes(&mut module.declarations, substitutions)?;
    for context in &mut module.contexts {
        substitute_declaration_modes(&mut context.declarations, substitutions)?;
    }
    substitute_action_modes(&mut module.actions, substitutions)?;
    if let Some(handler) = &mut module.handler {
        for alternative in &mut handler.alternatives {
            substitute_action_modes(&mut alternative.actions, substitutions)?;
        }
        if let Some(actions) = &mut handler.else_actions {
            substitute_action_modes(actions, substitutions)?;
        }
    }
    Ok(())
}

fn substitute_action_modes(
    actions: &mut [Action],
    substitutions: &HashMap<&str, &ModeRef>,
) -> Result<(), Error> {
    for action in actions {
        match action {
            Action::Begin {
                declarations,
                actions,
                ..
            } => {
                substitute_declaration_modes(declarations, substitutions)?;
                substitute_action_modes(actions, substitutions)?;
            }
            Action::If {
                branches,
                else_actions,
                ..
            } => {
                for branch in branches {
                    substitute_action_modes(&mut branch.body, substitutions)?;
                }
                if let Some(actions) = else_actions {
                    substitute_action_modes(actions, substitutions)?;
                }
            }
            Action::Case {
                alternatives,
                else_actions,
                ..
            } => {
                for alternative in alternatives {
                    substitute_action_modes(&mut alternative.body, substitutions)?;
                }
                if let Some(actions) = else_actions {
                    substitute_action_modes(actions, substitutions)?;
                }
            }
            Action::Do { body, .. } => substitute_action_modes(body, substitutions)?,
            Action::Labelled { action, .. } => {
                substitute_action_modes(std::slice::from_mut(action.as_mut()), substitutions)?;
            }
            Action::Handled { handler, .. } => {
                for alternative in &mut handler.alternatives {
                    substitute_action_modes(&mut alternative.actions, substitutions)?;
                }
                if let Some(actions) = &mut handler.else_actions {
                    substitute_action_modes(actions, substitutions)?;
                }
            }
            Action::Module(module) => substitute_module_modes(module, substitutions)?,
            _ => {}
        }
    }
    Ok(())
}

fn expand_module_generics(module: &mut Module) -> Result<(), Error> {
    let mut templates: HashMap<String, crate::ast::GenericProcTemplate> = HashMap::new();
    let mut process_templates: HashMap<String, crate::ast::GenericProcessTemplate> = HashMap::new();
    for declaration in &module.declarations {
        if let Declaration::GenericProc(template) = declaration {
            if templates
                .insert(template.name.clone(), template.clone())
                .is_some()
            {
                return Err(Error::new(
                    template.span,
                    format!("duplicate generic template `{}`", template.name),
                ));
            }
        }
        if let Declaration::GenericProcess(template) = declaration {
            if process_templates
                .insert(template.name.clone(), template.clone())
                .is_some()
            {
                return Err(Error::new(
                    template.span,
                    format!("duplicate generic process template `{}`", template.name),
                ));
            }
        }
    }
    let mut declarations = Vec::with_capacity(module.declarations.len());
    for declaration in module.declarations.drain(..) {
        match declaration {
            Declaration::GenericProc(_) | Declaration::GenericProcess(_) => {}
            Declaration::GenericProcessInstance(instance) => {
                let Some(template) = process_templates.get(&instance.template) else {
                    return Err(Error::new(
                        instance.span,
                        format!("unknown generic process template `{}`", instance.template),
                    ));
                };
                let (substitutions, syn_substitutions, proc_substitutions) = generic_substitutions(
                    &template.formal_params,
                    &instance.actuals,
                    instance.span,
                    &instance.template,
                )?;
                let mut process_def = template.process.clone();
                process_def.name = instance.name.clone();
                process_def.span = instance.span;
                substitute_process_modes(&mut process_def, &substitutions)?;
                substitute_process_syns(&mut process_def, &syn_substitutions, &proc_substitutions);
                declarations.push(Declaration::Process(process_def));
            }
            Declaration::GenericProcInstance(instance) => {
                let Some(template) = templates.get(&instance.template) else {
                    return Err(Error::new(
                        instance.span,
                        format!("unknown generic template `{}`", instance.template),
                    ));
                };
                let (substitutions, syn_substitutions, proc_substitutions) = generic_substitutions(
                    &template.formal_params,
                    &instance.actuals,
                    instance.span,
                    &instance.template,
                )?;
                let mut proc_def = template.proc_def.clone();
                proc_def.name = instance.name.clone();
                proc_def.span = instance.span;
                substitute_proc_modes(&mut proc_def, &substitutions)?;
                substitute_proc_syns(&mut proc_def, &syn_substitutions, &proc_substitutions);
                declarations.push(Declaration::Proc(proc_def));
            }
            other => declarations.push(other),
        }
    }
    module.declarations = declarations;
    Ok(())
}

type GenericModeSubstitutions<'a, 'b> = HashMap<&'a str, &'b ModeRef>;
type GenericSynSubstitutions<'a, 'b> = HashMap<&'a str, &'b Expr>;
type GenericProcSubstitutions<'a, 'b> = HashMap<&'a str, &'b str>;

fn generic_substitutions<'a, 'b>(
    formals: &'a [crate::ast::GenericFormal],
    actuals: &'b [crate::ast::GenericActual],
    span: Span,
    template: &str,
) -> Result<
    (
        GenericModeSubstitutions<'a, 'b>,
        GenericSynSubstitutions<'a, 'b>,
        GenericProcSubstitutions<'a, 'b>,
    ),
    Error,
> {
    if formals.len() != actuals.len() {
        return Err(Error::new(
            span,
            format!(
                "generic template `{template}` expects {} parameter(s), found {}",
                formals.len(),
                actuals.len()
            ),
        ));
    }
    let mut modes = HashMap::new();
    let mut syns = HashMap::new();
    let mut procs = HashMap::new();
    for (formal, actual) in formals.iter().zip(actuals) {
        match (formal, actual) {
            (crate::ast::GenericFormal::Mode(name), crate::ast::GenericActual::Mode(mode)) => {
                modes.insert(name.as_str(), mode);
            }
            (
                crate::ast::GenericFormal::Syn { name, .. },
                crate::ast::GenericActual::Value(value),
            ) => {
                syns.insert(name.as_str(), value);
            }
            (
                crate::ast::GenericFormal::Proc { name, .. },
                crate::ast::GenericActual::Proc { target, .. },
            ) => {
                procs.insert(name.as_str(), target.as_str());
            }
            (crate::ast::GenericFormal::Mode(name), crate::ast::GenericActual::Value(_)) => {
                return Err(Error::new(
                    span,
                    format!("generic mode parameter `{name}` requires a mode actual"),
                ));
            }
            (crate::ast::GenericFormal::Syn { name, .. }, crate::ast::GenericActual::Mode(_)) => {
                return Err(Error::new(
                    span,
                    format!("generic SYN parameter `{name}` requires a value actual"),
                ));
            }
            (crate::ast::GenericFormal::Proc { name, .. }, crate::ast::GenericActual::Mode(_)) => {
                return Err(Error::new(
                    span,
                    format!("generic PROC parameter `{name}` requires a PROC actual"),
                ));
            }
            (crate::ast::GenericFormal::Proc { name, .. }, crate::ast::GenericActual::Value(_)) => {
                return Err(Error::new(
                    span,
                    format!("generic PROC parameter `{name}` requires a PROC actual"),
                ));
            }
            (crate::ast::GenericFormal::Mode(name), crate::ast::GenericActual::Proc { .. }) => {
                return Err(Error::new(
                    span,
                    format!("generic mode parameter `{name}` requires a mode actual"),
                ));
            }
            (
                crate::ast::GenericFormal::Syn { name, .. },
                crate::ast::GenericActual::Proc { .. },
            ) => {
                return Err(Error::new(
                    span,
                    format!("generic SYN parameter `{name}` requires a value actual"),
                ));
            }
        }
    }
    Ok((modes, syns, procs))
}

fn substitute_expr_syns(
    expr: &Expr,
    syns: &HashMap<&str, &Expr>,
    procs: &HashMap<&str, &str>,
) -> Expr {
    match expr {
        Expr::Name { name, span } => syns
            .get(name.as_str())
            .map(|value| (*value).clone())
            .or_else(|| {
                procs.get(name.as_str()).map(|target| Expr::Name {
                    name: (*target).to_owned(),
                    span: *span,
                })
            })
            .unwrap_or_else(|| expr.clone()),
        Expr::Apply { name, args, span } => Expr::Apply {
            name: procs
                .get(name.as_str())
                .map(|target| (*target).to_owned())
                .unwrap_or_else(|| name.clone()),
            args: args
                .iter()
                .map(|arg| substitute_expr_syns(arg, syns, procs))
                .collect(),
            span: *span,
        },
        Expr::Unary { op, operand, span } => Expr::Unary {
            op: *op,
            operand: Box::new(substitute_expr_syns(operand, syns, procs)),
            span: *span,
        },
        Expr::Binary {
            op,
            left,
            right,
            span,
        } => Expr::Binary {
            op: *op,
            left: Box::new(substitute_expr_syns(left, syns, procs)),
            right: Box::new(substitute_expr_syns(right, syns, procs)),
            span: *span,
        },
        Expr::Field { base, field, span } => Expr::Field {
            base: Box::new(substitute_expr_syns(base, syns, procs)),
            field: field.clone(),
            span: *span,
        },
        Expr::ArrayTuple { elements, span } => Expr::ArrayTuple {
            elements: elements
                .iter()
                .map(|element| crate::ast::ArrayTupleElement {
                    labels: element.labels.clone(),
                    value: substitute_expr_syns(&element.value, syns, procs),
                    range_end: element
                        .range_end
                        .as_ref()
                        .map(|end| Box::new(substitute_expr_syns(end, syns, procs))),
                    span: element.span,
                })
                .collect(),
            span: *span,
        },
        Expr::ArraySlice {
            name,
            first,
            last,
            up,
            span,
        } => Expr::ArraySlice {
            name: name.clone(),
            first: Box::new(substitute_expr_syns(first, syns, procs)),
            last: Box::new(substitute_expr_syns(last, syns, procs)),
            up: *up,
            span: *span,
        },
        Expr::MethodCall {
            base,
            method,
            args,
            span,
        } => Expr::MethodCall {
            base: Box::new(substitute_expr_syns(base, syns, procs)),
            method: method.clone(),
            args: args
                .iter()
                .map(|arg| substitute_expr_syns(arg, syns, procs))
                .collect(),
            span: *span,
        },
        _ => expr.clone(),
    }
}

fn substitute_actions_syns(
    actions: &mut [Action],
    syns: &HashMap<&str, &Expr>,
    procs: &HashMap<&str, &str>,
) {
    for action in actions {
        match action {
            Action::Assign { target, value, .. } => {
                *target = substitute_expr_syns(target, syns, procs);
                *value = substitute_expr_syns(value, syns, procs);
            }
            Action::Assert { condition, .. } => {
                *condition = substitute_expr_syns(condition, syns, procs);
            }
            Action::Call { callee, .. } => {
                *callee = substitute_expr_syns(callee, syns, procs);
            }
            Action::Return {
                value: Some(value), ..
            } => {
                *value = substitute_expr_syns(value, syns, procs);
            }
            Action::Result { value, .. } => {
                *value = substitute_expr_syns(value, syns, procs);
            }
            Action::If {
                branches,
                else_actions,
                ..
            } => {
                for branch in branches {
                    branch.condition = substitute_expr_syns(&branch.condition, syns, procs);
                    substitute_actions_syns(&mut branch.body, syns, procs);
                }
                if let Some(actions) = else_actions {
                    substitute_actions_syns(actions, syns, procs);
                }
            }
            Action::Do { control, body, .. } => {
                match control {
                    crate::ast::DoControl::While(condition) => {
                        *condition = substitute_expr_syns(condition, syns, procs);
                    }
                    crate::ast::DoControl::For {
                        start, end, step, ..
                    } => {
                        *start = substitute_expr_syns(start, syns, procs);
                        *end = substitute_expr_syns(end, syns, procs);
                        if let Some(step) = step {
                            *step = substitute_expr_syns(step, syns, procs);
                        }
                    }
                    _ => {}
                }
                substitute_actions_syns(body, syns, procs);
            }
            Action::Case {
                selectors,
                alternatives,
                else_actions,
                ..
            } => {
                for selector in selectors {
                    *selector = substitute_expr_syns(selector, syns, procs);
                }
                for alternative in alternatives {
                    substitute_actions_syns(&mut alternative.body, syns, procs);
                }
                if let Some(actions) = else_actions {
                    substitute_actions_syns(actions, syns, procs);
                }
            }
            Action::Labelled { action, .. } => {
                substitute_actions_syns(std::slice::from_mut(action.as_mut()), syns, procs);
            }
            Action::Begin {
                declarations,
                actions,
                ..
            } => {
                substitute_declarations_syns(declarations, syns, procs);
                substitute_actions_syns(actions, syns, procs);
            }
            Action::Handled {
                action, handler, ..
            } => {
                substitute_actions_syns(std::slice::from_mut(action.as_mut()), syns, procs);
                substitute_handler_syns(handler, syns, procs);
            }
            Action::Module(module) => substitute_module_syns(module, syns, procs),
            _ => {}
        }
    }
}

fn substitute_handler_syns(
    handler: &mut crate::ast::Handler,
    syns: &HashMap<&str, &Expr>,
    procs: &HashMap<&str, &str>,
) {
    for alternative in &mut handler.alternatives {
        substitute_actions_syns(&mut alternative.actions, syns, procs);
    }
    if let Some(actions) = &mut handler.else_actions {
        substitute_actions_syns(actions, syns, procs);
    }
}

fn substitute_declarations_syns(
    declarations: &mut [Declaration],
    syns: &HashMap<&str, &Expr>,
    procs: &HashMap<&str, &str>,
) {
    for declaration in declarations {
        match declaration {
            Declaration::Var(vars) => {
                for var in vars {
                    if let Some(init) = &mut var.init {
                        init.value = substitute_expr_syns(&init.value, syns, procs);
                    }
                }
            }
            Declaration::Proc(proc_def) => {
                substitute_declarations_syns(&mut proc_def.declarations, syns, procs);
                substitute_actions_syns(&mut proc_def.actions, syns, procs);
                if let Some(handler) = &mut proc_def.handler {
                    substitute_handler_syns(handler, syns, procs);
                }
            }
            Declaration::Mode(mode_def) => {
                if let crate::ast::ModeDefinitionBody::Moreta {
                    declarations,
                    invariant,
                    ..
                } = &mut mode_def.body
                {
                    substitute_declarations_syns(declarations, syns, procs);
                    if let Some(invariant) = invariant {
                        *invariant = substitute_expr_syns(invariant, syns, procs);
                    }
                }
            }
            Declaration::SpecModule(module) => substitute_module_syns(module, syns, procs),
            _ => {}
        }
    }
}

fn substitute_module_syns(
    module: &mut Module,
    syns: &HashMap<&str, &Expr>,
    procs: &HashMap<&str, &str>,
) {
    substitute_declarations_syns(&mut module.declarations, syns, procs);
    for context in &mut module.contexts {
        substitute_declarations_syns(&mut context.declarations, syns, procs);
    }
    substitute_actions_syns(&mut module.actions, syns, procs);
    if let Some(handler) = &mut module.handler {
        substitute_handler_syns(handler, syns, procs);
    }
}

fn substitute_proc_syns(
    proc_def: &mut crate::ast::ProcDef,
    syns: &HashMap<&str, &Expr>,
    procs: &HashMap<&str, &str>,
) {
    substitute_declarations_syns(&mut proc_def.declarations, syns, procs);
    substitute_actions_syns(&mut proc_def.actions, syns, procs);
    if let Some(handler) = &mut proc_def.handler {
        substitute_handler_syns(handler, syns, procs);
    }
}

fn substitute_mode(
    mode: &mut ModeRef,
    substitutions: &HashMap<&str, &ModeRef>,
) -> Result<(), Error> {
    let Some(actual) = substitutions.get(mode.name.as_str()) else {
        return Ok(());
    };
    if !mode.arrays.is_empty() && !actual.arrays.is_empty() {
        return Err(Error::new(
            mode.span,
            "nested array modes are not implemented for generic substitution",
        ));
    }
    mode.name = actual.name.clone();
    if mode.arrays.is_empty() {
        mode.arrays = actual.arrays.clone();
    }
    mode.string_length = actual.string_length.or(mode.string_length);
    mode.varying = actual.varying || mode.varying;
    mode.access = actual.access || mode.access;
    mode.powerset = actual.powerset || mode.powerset;
    mode.access_indexed = actual.access_indexed || mode.access_indexed;
    mode.access_index = actual.access_index.or(mode.access_index);
    Ok(())
}

fn substitute_proc_modes(
    proc_def: &mut crate::ast::ProcDef,
    substitutions: &HashMap<&str, &ModeRef>,
) -> Result<(), Error> {
    for param in &mut proc_def.params {
        substitute_mode(&mut param.mode, substitutions)?;
    }
    if let Some(result) = &mut proc_def.result {
        substitute_mode(result, substitutions)?;
    }
    substitute_declaration_modes(&mut proc_def.declarations, substitutions)
}

fn substitute_process_modes(
    process_def: &mut crate::ast::ProcessDef,
    substitutions: &HashMap<&str, &ModeRef>,
) -> Result<(), Error> {
    for param in &mut process_def.params {
        substitute_mode(&mut param.mode, substitutions)?;
    }
    substitute_declaration_modes(&mut process_def.declarations, substitutions)
}

fn substitute_process_syns(
    process_def: &mut crate::ast::ProcessDef,
    syns: &HashMap<&str, &Expr>,
    procs: &HashMap<&str, &str>,
) {
    substitute_declarations_syns(&mut process_def.declarations, syns, procs);
    substitute_actions_syns(&mut process_def.actions, syns, procs);
    if let Some(handler) = &mut process_def.handler {
        substitute_handler_syns(handler, syns, procs);
    }
}

fn substitute_declaration_modes(
    declarations: &mut [Declaration],
    substitutions: &HashMap<&str, &ModeRef>,
) -> Result<(), Error> {
    for declaration in declarations {
        match declaration {
            Declaration::Var(vars) => {
                for var in vars {
                    substitute_mode(&mut var.mode, substitutions)?;
                }
            }
            Declaration::Process(process_def) => {
                substitute_declaration_modes(&mut process_def.declarations, substitutions)?;
            }
            Declaration::Signal(defs) => {
                for def in defs {
                    for mode in &mut def.modes {
                        substitute_mode(mode, substitutions)?;
                    }
                }
            }
            Declaration::GenericProcess(template) => {
                for param in &mut template.process.params {
                    substitute_mode(&mut param.mode, substitutions)?;
                }
                substitute_declaration_modes(&mut template.process.declarations, substitutions)?;
            }
            Declaration::GenericProcessInstance(_) => {}
            Declaration::Proc(proc_def) => substitute_proc_modes(proc_def, substitutions)?,
            Declaration::ProcSignature(signature) => {
                for param in &mut signature.params {
                    substitute_mode(&mut param.mode, substitutions)?;
                }
                if let Some(result) = &mut signature.result {
                    substitute_mode(result, substitutions)?;
                }
            }
            Declaration::Mode(mode_def) => match &mut mode_def.body {
                ModeDefinitionBody::Generic { formals, body, .. } => {
                    for formal in formals {
                        if let crate::ast::GenericFormal::Mode(_) = formal {
                            // Formal names are substituted when the template is expanded.
                        }
                    }
                    substitute_mode_definition_body(
                        body,
                        substitutions,
                        &HashMap::new(),
                        &HashMap::new(),
                    )?;
                }
                ModeDefinitionBody::GenericInstance { .. } => {}

                crate::ast::ModeDefinitionBody::Struct(fields) => {
                    for field in fields {
                        substitute_mode(&mut field.mode, substitutions)?;
                    }
                }
                crate::ast::ModeDefinitionBody::Alias(mode) => {
                    substitute_mode(mode, substitutions)?;
                }
                crate::ast::ModeDefinitionBody::Set(_) => {}
                crate::ast::ModeDefinitionBody::Moreta { declarations, .. } => {
                    substitute_declaration_modes(declarations, substitutions)?;
                }
            },
            Declaration::Visibility(_)
            | Declaration::SpecModule(_)
            | Declaration::GenericProc(_)
            | Declaration::GenericProcInstance(_) => {}
        }
    }
    Ok(())
}
