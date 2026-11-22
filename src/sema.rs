use crate::ast::*;
use crate::diagnostics::Diagnostic;
use crate::lexer::{SourceId, Span};
use crate::manifest::WidthProfile;
use crate::program::{self, ExportKind};
use crate::types::ProcType;
use crate::types::{self, IntKind, Type};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub span: Span,
    pub message: String,
}

impl Error {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            code: "E0400",
            span,
            message: message.into(),
        }
    }

    pub fn coded(code: &'static str, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            span,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyResolution {
    Conversion(IntKind),
    FloatConversion,
    Procedure(ProcType),
    ArrayIndex(Type),
    Builtin(crate::builtins::Builtin),
    TextIo {
        kind: crate::builtins::TextIo,
        format: Vec<crate::builtins::FormatOp>,
        indexed: bool,
    },
    Association {
        op: crate::builtins::AssociationOp,
        index: bool,
        target: bool,
    },
    SetElement {
        ty: Type,
        value: i128,
    },
    Method {
        proc_span: Span,
        result: Option<Type>,
        passing: Vec<Passing>,
    },
}

/// The checker's result: diagnostics plus expression annotations that the
/// interpreter consumes instead of re-inferring modes.
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub diagnostics: Vec<Diagnostic>,
    pub types: HashMap<Span, Type>,
    pub resolutions: HashMap<Span, ApplyResolution>,
    /// Concrete indices assigned by each entry of a labelled array tuple.
    pub labelled_tuples: HashMap<Span, Vec<Vec<i128>>>,
}

pub fn check(program: &Program, profile: WidthProfile) -> Result<(), Error> {
    check_with_externals(program, profile, &[])
}

pub fn check_with_externals(
    program: &Program,
    profile: WidthProfile,
    external: &[program::ExternalInterface],
) -> Result<(), Error> {
    let result = analyze_with_externals(program, profile, external);
    match result.diagnostics.into_iter().next() {
        Some(diagnostic) => {
            let span = diagnostic.span.unwrap_or_else(|| {
                program
                    .modules
                    .first()
                    .map(|module| module.span)
                    .unwrap_or(Span::new(SourceId(0), 0, 0))
            });
            Err(Error::coded(diagnostic.code, span, diagnostic.message))
        }
        None => Ok(()),
    }
}

pub fn analyze(program: &Program, profile: WidthProfile) -> CheckResult {
    analyze_with_externals(program, profile, &[])
}

pub fn check_all(program: &Program, profile: WidthProfile) -> Vec<Diagnostic> {
    analyze_with_externals(program, profile, &[]).diagnostics
}

pub fn check_all_with_externals(
    program: &Program,
    profile: WidthProfile,
    external: &[program::ExternalInterface],
) -> Vec<Diagnostic> {
    analyze_with_externals(program, profile, external).diagnostics
}

pub fn analyze_with_externals(
    program: &Program,
    profile: WidthProfile,
    external: &[program::ExternalInterface],
) -> CheckResult {
    let program = match program::normalize(program) {
        Ok(program) => program,
        Err(error) => {
            return CheckResult {
                diagnostics: vec![Diagnostic::new(error.code, Some(error.span), error.message)],
                types: HashMap::new(),
                resolutions: HashMap::new(),
                labelled_tuples: HashMap::new(),
            };
        }
    };
    let info = match program::analyze(&program) {
        Ok(info) => info,
        Err(error) => {
            return CheckResult {
                diagnostics: vec![Diagnostic::new(error.code, Some(error.span), error.message)],
                types: HashMap::new(),
                resolutions: HashMap::new(),
                labelled_tuples: HashMap::new(),
            };
        }
    };
    let module_modes: Vec<HashMap<String, Type>> = info
        .interfaces
        .iter()
        .map(|interface| interface.modes.clone())
        .collect();
    let mut modes = info.modes;
    let external_modes = match program::add_external_modes(&mut modes, external) {
        Ok(ids) => ids,
        Err(message) => return external_error(message),
    };
    let mut exports = info.exports;
    if let Err(message) = program::add_external_exports(&mut exports, external, &external_modes) {
        return external_error(message);
    }

    let mut checker = Checker {
        profile,
        scopes: Vec::new(),
        results: Vec::new(),
        exports,
        modes,
        module_modes,
        reaches: info.reaches,
        current_nested_module_name: None,
        module_floor: 0,
        current_module: 0,
        current_package: 0,
        current_package_name: None,
        current_module_name: None,
        labels: Vec::new(),
        diagnostics: Vec::new(),
        types: HashMap::new(),
        resolutions: HashMap::new(),
        exception_sets: Vec::new(),
        labelled_tuples: HashMap::new(),
    };
    for (index, module) in program.modules.iter().enumerate() {
        checker.current_module = index;
        checker.current_package = module.package;
        checker.current_package_name = module.package_name.clone();
        checker.current_module_name = module.name.clone();
        let result = if module.kind.is_spec() {
            checker.check_spec_module(module)
        } else {
            checker.check_module(module)
        };
        if let Err(error) = result {
            checker.push_diagnostic(error);
        }
    }

    CheckResult {
        diagnostics: checker.diagnostics,
        types: checker.types,
        resolutions: checker.resolutions,
        labelled_tuples: checker.labelled_tuples,
    }
}

fn external_error(message: String) -> CheckResult {
    CheckResult {
        diagnostics: vec![Diagnostic::new("E0600", None, message)],
        types: HashMap::new(),
        resolutions: HashMap::new(),
        labelled_tuples: HashMap::new(),
    }
}

fn map_program_error(error: program::Error) -> Error {
    Error::coded(error.code, error.span, error.message)
}

#[derive(Clone)]
struct ReachBinding {
    meta: crate::reach::ExportMeta,
    kind: ReachKind,
}

#[derive(Clone)]
enum ReachKind {
    Var(Type),
    Proc(ProcType),
    Mode(Type),
}

#[derive(Default, Clone)]
struct Scope {
    vars: HashMap<String, Type>,
    procs: HashMap<String, ProcType>,
    modes: HashMap<String, Type>,
    set_values: HashMap<String, (Type, i128)>,
    /// Names granted into this reach by directly enclosed modules.
    exports: HashMap<String, Vec<ReachBinding>>,
    readonly: HashSet<String>,
}

struct Checker {
    profile: WidthProfile,
    scopes: Vec<Scope>,
    results: Vec<Option<Type>>,
    exports: Vec<program::ExportEntry>,
    modes: crate::modes::ModeTable,
    module_modes: Vec<HashMap<String, Type>>,
    reaches: program::ReachTree,
    current_nested_module_name: Option<String>,
    /// Index of the current module's scope in `scopes`. Outer module scopes
    /// below this index are hidden except through `SEIZE`.
    module_floor: usize,
    current_module: usize,
    current_package: usize,
    current_package_name: Option<String>,
    current_module_name: Option<String>,
    labels: Vec<String>,
    diagnostics: Vec<Diagnostic>,
    types: HashMap<Span, Type>,
    resolutions: HashMap<Span, ApplyResolution>,
    /// Statically possible exceptions for the action list currently being
    /// checked. A new set is pushed around procedure/module bodies and handlers.
    exception_sets: Vec<HashSet<String>>,
    labelled_tuples: HashMap<Span, Vec<Vec<i128>>>,
}

impl Checker {
    fn push_diagnostic(&mut self, error: Error) {
        if self.diagnostics.iter().any(|diagnostic| {
            diagnostic.span == Some(error.span) && diagnostic.message == error.message
        }) {
            return;
        }
        self.diagnostics
            .push(Diagnostic::new(error.code, Some(error.span), error.message));
    }

    fn push_exception_set(&mut self) {
        self.exception_sets.push(HashSet::new());
    }

    fn take_exception_set(&mut self) -> HashSet<String> {
        self.exception_sets.pop().unwrap_or_default()
    }

    fn record_exception(&mut self, name: &str) {
        if let Some(set) = self.exception_sets.last_mut() {
            set.insert(name.to_owned());
        }
    }

    fn record_exceptions(&mut self, names: impl IntoIterator<Item = String>) {
        if let Some(set) = self.exception_sets.last_mut() {
            set.extend(names);
        }
    }

    fn check_module(&mut self, module: &Module) -> Result<(), Error> {
        let previous_floor = self.module_floor;
        self.scopes.push(Scope::default());
        self.module_floor = self.scopes.len() - 1;
        let result = self.check_module_body(module);
        self.scopes.pop();
        self.module_floor = previous_floor;
        result
    }

    fn check_spec_module(&mut self, module: &Module) -> Result<(), Error> {
        let previous_floor = self.module_floor;
        self.scopes.push(Scope::default());
        self.module_floor = self.scopes.len() - 1;
        let result = (|| {
            self.apply_seizes(&module.declarations)?;
            self.preload_module_modes(module)?;
            self.collect_declarations(&module.declarations)?;

            for declaration in &module.declarations {
                let Declaration::Var(vars) = declaration else {
                    continue;
                };
                for var in vars {
                    if var.init.is_some() {
                        return Err(Error::coded(
                            "E0406",
                            var.span,
                            "initialization is not allowed in a spec module",
                        ));
                    }
                }
            }

            Ok(())
        })();
        self.scopes.pop();
        self.module_floor = previous_floor;
        result
    }

    fn check_module_body(&mut self, module: &Module) -> Result<(), Error> {
        self.apply_seizes(&module.declarations)?;
        self.preload_module_modes(module)?;
        self.collect_declarations(&module.declarations)?;
        self.check_initializers(&module.declarations)?;
        self.check_proc_bodies(&module.declarations)?;
        self.seed_reach_exports(module);

        let has_module_label = module.name.is_some();
        if let Some(name) = &module.name {
            self.labels.push(name.clone());
        }
        self.push_exception_set();
        let result = self.check_actions(&module.actions);
        let incoming = self.take_exception_set();
        let result = result.and_then(|()| {
            let escaping = if let Some(handler) = &module.handler {
                self.check_handler_flow(handler, incoming)?
            } else {
                incoming
            };
            self.record_exceptions(escaping);
            Ok(())
        });
        if has_module_label {
            self.labels.pop();
        }
        result
    }

    fn seed_reach_exports(&mut self, module: &Module) {
        self.seed_reach_exports_at(module.span);
    }

    fn seed_reach_exports_at(&mut self, span: Span) {
        let Some(node) = self.reaches.get(span) else {
            return;
        };
        let exports = node.exports.clone();
        let scope = self.scopes.last_mut().expect("scope stack is never empty");
        for (name, entries) in exports {
            for entry in entries {
                let kind = match entry.kind {
                    program::ReachExportKind::Var(ty) => ReachKind::Var(ty),
                    program::ReachExportKind::Proc(signature) => ReachKind::Proc(signature),
                    program::ReachExportKind::Mode(ty) => ReachKind::Mode(ty),
                };
                scope
                    .exports
                    .entry(name.clone())
                    .or_default()
                    .push(ReachBinding {
                        meta: entry.meta,
                        kind,
                    });
            }
        }
    }

    fn check_begin(
        &mut self,
        declarations: &[Declaration],
        actions: &[Action],
        span: Span,
        handler: Option<&Handler>,
    ) -> Result<(), Error> {
        self.scopes.push(Scope::default());
        self.push_exception_set();
        let result = (|| {
            self.collect_declarations(declarations)?;
            reject_nested_procs(declarations)?;
            self.check_initializers(declarations)?;
            self.check_proc_bodies(declarations)?;
            self.seed_reach_exports_at(span);
            self.check_actions(actions)
        })();
        let incoming = self.take_exception_set();
        let result = result.and_then(|()| {
            let escaping = if let Some(handler) = handler {
                self.check_handler_flow(handler, incoming)?
            } else {
                incoming
            };
            self.record_exceptions(escaping);
            Ok(())
        });
        self.scopes.pop();
        result
    }

    /// Check a handler and return the exceptions that may escape it, given the
    /// exceptions possible in the action it is appended to.
    fn check_handler_flow(
        &mut self,
        handler: &Handler,
        incoming: HashSet<String>,
    ) -> Result<HashSet<String>, Error> {
        let mut names = HashSet::new();
        let mut caught = HashSet::new();
        for alternative in &handler.alternatives {
            for exception in &alternative.exceptions {
                if !names.insert(exception.clone()) {
                    return Err(Error::coded(
                        "E0402",
                        alternative.span,
                        format!("duplicate exception name `{exception}` in handler"),
                    ));
                }
                caught.insert(exception.clone());
            }
        }

        let mut escaping = if handler.else_actions.is_some() {
            HashSet::new()
        } else {
            incoming.difference(&caught).cloned().collect()
        };

        self.push_exception_set();
        let result = (|| {
            for alternative in &handler.alternatives {
                self.check_actions(&alternative.actions)?;
            }
            if let Some(actions) = &handler.else_actions {
                self.check_actions(actions)?;
            }
            Ok(())
        })();
        let from_handler = self.take_exception_set();
        result?;
        escaping.extend(from_handler);
        Ok(escaping)
    }

    fn check_nested_module(&mut self, module: &Module) -> Result<(), Error> {
        for declaration in &module.declarations {
            if let Declaration::ProcSignature(signature) = declaration {
                return Err(Error::coded(
                    "E0409",
                    signature.span,
                    "procedure signatures in nested modules are not supported yet",
                ));
            }
        }

        let parent = self.scopes.last().cloned().unwrap_or_default();
        self.check_nested_contexts(module, &parent)?;

        let previous_floor = self.module_floor;
        let previous_name =
            std::mem::replace(&mut self.current_nested_module_name, module.name.clone());
        self.scopes.push(Scope::default());
        self.module_floor = self.scopes.len() - 1;

        self.push_exception_set();
        let result = (|| {
            self.preload_module_modes(module)?;
            self.apply_nested_seizes(&module.declarations, &parent)?;
            self.collect_declarations(&module.declarations)?;
            self.check_initializers(&module.declarations)?;
            self.check_proc_bodies(&module.declarations)?;
            self.seed_reach_exports(module);

            let has_label = module.name.is_some();
            if let Some(name) = &module.name {
                self.labels.push(name.clone());
            }
            let result = self.check_actions(&module.actions);
            if has_label {
                self.labels.pop();
            }
            result
        })();
        let incoming = self.take_exception_set();
        let result = result.and_then(|()| {
            let escaping = if let Some(handler) = &module.handler {
                self.check_handler_flow(handler, incoming)?
            } else {
                incoming
            };
            self.record_exceptions(escaping);
            Ok(())
        });

        self.scopes.pop();
        self.module_floor = previous_floor;
        self.current_nested_module_name = previous_name;
        result
    }

    fn import_reach_export(
        &mut self,
        name: &str,
        export: &ReachBinding,
        span: Span,
    ) -> Result<(), Error> {
        match &export.kind {
            ReachKind::Var(ty) => self.declare_var(name, *ty, span),
            ReachKind::Proc(signature) => self.declare_proc(name, signature.clone(), span),
            ReachKind::Mode(ty) => self.declare_mode(name, *ty, span),
        }
    }

    fn check_nested_contexts(&self, module: &Module, parent: &Scope) -> Result<(), Error> {
        let mut declared_names = HashSet::new();
        for (context_index, context) in module.contexts.iter().enumerate() {
            let mut current_names = HashSet::new();
            for declaration in &context.declarations {
                match declaration {
                    Declaration::Var(list) => {
                        for var in list {
                            if !declared_names.insert(var.name.clone()) {
                                return Err(Error::coded(
                                    "E0402",
                                    var.span,
                                    format!("duplicate context declaration `{}`", var.name),
                                ));
                            }
                            current_names.insert(var.name.clone());
                            let declared = self.resolve_mode(&var.mode)?;
                            let parent_ty = parent.vars.get(&var.name).copied().or_else(|| {
                                parent.exports.get(&var.name).and_then(|exports| {
                                    match exports.as_slice() {
                                        [ReachBinding {
                                            kind: ReachKind::Var(ty),
                                            meta,
                                            ..
                                        }] if meta.prefix.is_empty() => Some(*ty),
                                        _ => None,
                                    }
                                })
                            });
                            match parent_ty {
                                Some(actual) if actual == declared => {}
                                Some(actual) => {
                                    return Err(Error::coded("E0400", 
                                        var.span,
                                        format!(
                                            "context declaration `{}` has mode {} but the enclosing reach has mode {}",
                                            var.name,
                                            declared.describe(),
                                            actual.describe()
                                        ),
                                    ));
                                }
                                None => {
                                    return Err(Error::coded("E0400", 
                                        var.span,
                                        format!(
                                            "context declaration `{}` has no matching name in the enclosing reach",
                                            var.name
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                    Declaration::ProcSignature(signature) => {
                        if !declared_names.insert(signature.name.clone()) {
                            return Err(Error::coded(
                                "E0402",
                                signature.span,
                                format!("duplicate context declaration `{}`", signature.name),
                            ));
                        }
                        current_names.insert(signature.name.clone());
                        let declared = self.signature_from_params(
                            &signature.params,
                            &signature.result,
                            &signature.exceptions,
                            signature.span,
                        )?;
                        let parent_sig = parent.procs.get(&signature.name).cloned().or_else(|| {
                            parent.exports.get(&signature.name).and_then(|exports| {
                                match exports.as_slice() {
                                    [ReachBinding {
                                        kind: ReachKind::Proc(signature),
                                        meta,
                                        ..
                                    }] if meta.prefix.is_empty() => Some(signature.clone()),
                                    _ => None,
                                }
                            })
                        });
                        match parent_sig {
                            Some(actual) if actual == declared => {}
                            Some(_) => {
                                return Err(Error::coded(
                                    "E0400",
                                    signature.span,
                                    format!(
                                        "context procedure `{}` does not match the enclosing reach",
                                        signature.name
                                    ),
                                ));
                            }
                            None => {
                                return Err(Error::coded("E0400", 
                                    signature.span,
                                    format!(
                                        "context procedure `{}` has no matching name in the enclosing reach",
                                        signature.name
                                    ),
                                ));
                            }
                        }
                    }
                    Declaration::Mode(_) | Declaration::Proc(_) => {
                        return Err(Error::coded(
                            "E0400",
                            context.span,
                            "unsupported declaration in a nested context",
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
                    return Err(Error::coded(
                        "E0400",
                        visibility.span,
                        "a context with no enclosing context cannot contain visibility statements",
                    ));
                }
                if visibility.prefix.is_some()
                    || !visibility.friends.is_empty()
                    || visibility.forbid.is_some()
                {
                    return Err(Error::coded(
                        "E0409",
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
                            return Err(Error::coded(
                                "E0406",
                                visibility.span,
                                format!("cannot GRANT `{name}` in a context: not declared here"),
                            ));
                        }
                    } else if !declared_names.contains(&name) {
                        return Err(Error::coded(
                            "E0406",
                            visibility.span,
                            format!("cannot SEIZE `{name}` in a context: no earlier declaration"),
                        ));
                    } else {
                        declared_names.insert(name);
                    }
                }
            }
        }
        Ok(())
    }

    fn apply_nested_seizes(
        &mut self,
        declarations: &[Declaration],
        parent: &Scope,
    ) -> Result<(), Error> {
        let package = self.current_package_name.clone();
        let module_name = self.current_nested_module_name.clone();
        for declaration in declarations {
            let Declaration::Visibility(visibility) = declaration else {
                continue;
            };
            if visibility.kind != VisibilityKind::Seize {
                continue;
            }
            let old_prefix = program::seize_old_prefix(&visibility.prefix)
                .map_err(|message| Error::coded("E0400", visibility.span, message))?;

            if visibility.all {
                if old_prefix.is_empty() {
                    for (name, ty) in &parent.vars {
                        self.declare_var(name, *ty, visibility.span)?;
                    }
                    for (name, signature) in &parent.procs {
                        self.declare_proc(name, signature.clone(), visibility.span)?;
                    }
                    for (name, ty) in &parent.modes {
                        self.declare_mode(name, *ty, visibility.span)?;
                    }
                }
                for (name, exports) in &parent.exports {
                    let matching: Vec<&ReachBinding> = exports
                        .iter()
                        .filter(|binding| {
                            binding.meta.matches(
                                old_prefix,
                                package.as_deref(),
                                module_name.as_deref(),
                            )
                        })
                        .collect();
                    match matching.as_slice() {
                        [] => {}
                        [binding] => self.import_reach_export(name, binding, visibility.span)?,
                        _ => {
                            return Err(Error::coded("E0406", 
                                visibility.span,
                                format!(
                                    "SEIZE ALL is ambiguous for `{name}`; several sibling modules grant it"
                                ),
                            ));
                        }
                    }
                }
                continue;
            }

            for name in &visibility.names {
                let mut source_prefix = old_prefix.to_vec();
                source_prefix.extend(name.prefix.iter().cloned());

                if source_prefix.is_empty() {
                    if let Some(ty) = parent.vars.get(&name.name) {
                        self.declare_var(&name.name, *ty, name.span)?;
                        continue;
                    }
                    if let Some(signature) = parent.procs.get(&name.name) {
                        self.declare_proc(&name.name, signature.clone(), name.span)?;
                        continue;
                    }
                    if let Some(ty) = parent.modes.get(&name.name) {
                        self.declare_mode(&name.name, *ty, name.span)?;
                        continue;
                    }
                }

                let Some(exports) = parent.exports.get(&name.name) else {
                    return Err(Error::coded(
                        "E0406",
                        name.span,
                        format!(
                            "cannot SEIZE `{}`: no such name in the enclosing reach",
                            name.qualified()
                        ),
                    ));
                };
                let selected = crate::reach::select(
                    exports.iter().map(|binding| (&binding.meta, &binding.kind)),
                    &source_prefix,
                    package.as_deref(),
                    module_name.as_deref(),
                );
                match selected {
                    Ok(kind) => {
                        let binding = ReachBinding {
                            meta: crate::reach::ExportMeta::default(),
                            kind: kind.clone(),
                        };
                        self.import_reach_export(&name.name, &binding, name.span)?;
                    }
                    Err(crate::reach::SelectError::NotFound) => {
                        return Err(Error::coded(
                            "E0406",
                            name.span,
                            format!(
                                "cannot SEIZE `{}`: no such name in the enclosing reach",
                                name.qualified()
                            ),
                        ));
                    }
                    Err(crate::reach::SelectError::Ambiguous) => {
                        return Err(Error::coded(
                            "E0406",
                            name.span,
                            format!(
                                "SEIZE `{}` is ambiguous; several sibling modules grant it",
                                name.qualified()
                            ),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn declare_mode(&mut self, name: &str, ty: Type, span: Span) -> Result<(), Error> {
        let scope = self.scopes.last_mut().expect("scope stack is never empty");
        if scope.vars.contains_key(name)
            || scope.procs.contains_key(name)
            || scope.modes.contains_key(name)
        {
            return Err(Error::coded(
                "E0402",
                span,
                format!("duplicate declaration of `{name}`"),
            ));
        }
        scope.modes.insert(name.to_owned(), ty);
        Ok(())
    }

    fn apply_seizes(&mut self, declarations: &[Declaration]) -> Result<(), Error> {
        for declaration in declarations {
            let Declaration::Visibility(visibility) = declaration else {
                continue;
            };
            if visibility.kind != VisibilityKind::Seize {
                continue;
            }

            if visibility.all {
                self.seize_all(visibility)?;
                continue;
            }

            for name in &visibility.names {
                let (source, local) = program::seize_name(name, &visibility.prefix)
                    .map_err(|message| Error::coded("E0400", name.span, message))?;
                let export = program::resolve_export(
                    &self.exports,
                    &source,
                    self.current_package_name.as_deref(),
                    self.current_module_name.as_deref(),
                )
                .map_err(|message| Error::coded("E0400", name.span, message))?
                .clone();

                if export.module == self.current_module {
                    return Err(Error::coded(
                        "E0406",
                        name.span,
                        format!("cannot SEIZE `{}` from the same module", name.qualified()),
                    ));
                }

                self.import_export(&local, &export, name.span)?;
            }
        }
        Ok(())
    }

    fn seize_all(&mut self, visibility: &Visibility) -> Result<(), Error> {
        let span = visibility.span;
        let old_prefix = program::seize_old_prefix(&visibility.prefix)
            .map_err(|message| Error::coded("E0400", span, message))?;
        let mut accessible: Vec<program::ExportEntry> = self
            .exports
            .iter()
            .filter(|export| export.module != self.current_module)
            .filter(|export| export.prefix == old_prefix)
            .filter(|export| {
                export.is_accessible_to(
                    self.current_package_name.as_deref(),
                    self.current_module_name.as_deref(),
                )
            })
            .cloned()
            .collect();
        accessible.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.package.cmp(&right.package))
                .then_with(|| left.module.cmp(&right.module))
        });

        let mut names = HashSet::new();
        for export in &accessible {
            if !names.insert(export.name.clone()) {
                return Err(Error::coded(
                    "E0406",
                    span,
                    format!(
                        "SEIZE ALL is ambiguous for `{}`; use a qualified SEIZE",
                        export.name
                    ),
                ));
            }
            self.import_export(&export.name, export, span)?;
        }
        Ok(())
    }

    fn import_export(
        &mut self,
        local_name: &str,
        export: &program::ExportEntry,
        span: Span,
    ) -> Result<(), Error> {
        match &export.kind {
            ExportKind::Var(ty) => self.declare_var(local_name, *ty, span),
            ExportKind::Proc(signature) => self.declare_proc(local_name, signature.clone(), span),
            ExportKind::Mode(id) => {
                let scope = self.scopes.last_mut().expect("scope stack is never empty");
                scope.modes.insert(local_name.to_owned(), Type::Struct(*id));
                Ok(())
            }
        }
    }

    fn collect_declarations(&mut self, declarations: &[Declaration]) -> Result<(), Error> {
        for declaration in declarations {
            match declaration {
                Declaration::Var(vars) => {
                    for var in vars {
                        let ty = self.resolve_mode(&var.mode)?;
                        self.declare_var(&var.name, ty, var.span)?;
                    }
                }
                Declaration::Proc(proc_def) => {
                    let signature = self.signature_from_params(
                        &proc_def.params,
                        &proc_def.result,
                        &proc_def.exceptions,
                        proc_def.span,
                    )?;
                    self.declare_proc(&proc_def.name, signature, proc_def.span)?;
                }
                Declaration::ProcSignature(signature) => {
                    let proc_sig = self.signature_from_params(
                        &signature.params,
                        &signature.result,
                        &signature.exceptions,
                        signature.span,
                    )?;
                    self.declare_proc(&signature.name, proc_sig, signature.span)?;
                }
                Declaration::Mode(mode_def) => {
                    if let ModeDefinitionBody::Moreta {
                        declarations,
                        invariant,
                        ..
                    } = &mode_def.body
                    {
                        let mode_ref = ModeRef {
                            name: mode_def.name.clone(),
                            arrays: Vec::new(),
                            string_length: None,
                            varying: false,
                            access: false,
                            powerset: false,
                            access_indexed: false,
                            access_index: None,
                            span: mode_def.span,
                        };
                        let Type::Moreta(id) = self.resolve_mode(&mode_ref)? else {
                            return Err(Error::coded(
                                "E0400",
                                mode_def.span,
                                "internal: module mode has wrong type",
                            ));
                        };
                        let mut component_scope = Scope::default();
                        for component in &self.modes.get_moreta(id).components {
                            match component {
                                crate::modes::MoretaComponent::Var { name, ty } => {
                                    component_scope.vars.insert(name.clone(), *ty);
                                }
                                crate::modes::MoretaComponent::Proc {
                                    name, signature, ..
                                } => {
                                    component_scope
                                        .procs
                                        .insert(name.clone(), signature.clone());
                                }
                            }
                        }
                        self.scopes.push(component_scope);
                        let result = (|| {
                            self.check_initializers(declarations)?;
                            if let Some(invariant) = invariant {
                                let ty = self.check_expr(invariant)?;
                                if ty != Type::Bool {
                                    return Err(Error::coded(
                                        "E0400",
                                        invariant.span(),
                                        "INVARIANT expression must be BOOL",
                                    ));
                                }
                            }
                            for declaration in declarations {
                                if let Declaration::Proc(proc_def) = declaration {
                                    self.check_proc(proc_def)?;
                                }
                            }
                            Ok(())
                        })();
                        self.scopes.pop();
                        result?;
                        continue;
                    }
                    let ModeDefinitionBody::Set(elements) = &mode_def.body else {
                        continue;
                    };
                    let mode_ref = ModeRef {
                        name: mode_def.name.clone(),
                        arrays: Vec::new(),
                        string_length: None,
                        varying: false,
                        access: false,
                        powerset: false,
                        access_indexed: false,
                        access_index: None,
                        span: mode_def.span,
                    };
                    let ty = self.resolve_mode(&mode_ref)?;
                    let Type::Set(id) = ty else {
                        return Err(Error::coded(
                            "E0400",
                            mode_def.span,
                            "internal: set mode has wrong type",
                        ));
                    };
                    for element in elements {
                        let value = self
                            .modes
                            .get_set(id)
                            .elements
                            .iter()
                            .find(|(name, _)| name == &element.name)
                            .map(|(_, value)| *value)
                            .ok_or_else(|| {
                                Error::coded("E0400", element.span, "internal: missing set element")
                            })?;
                        let scope = self.scopes.last_mut().expect("scope stack is never empty");
                        if scope.vars.contains_key(&element.name)
                            || scope.procs.contains_key(&element.name)
                            || scope
                                .set_values
                                .insert(element.name.clone(), (ty, value))
                                .is_some()
                        {
                            return Err(Error::coded(
                                "E0402",
                                element.span,
                                format!("duplicate declaration of `{}`", element.name),
                            ));
                        }
                    }
                }
                Declaration::Visibility(_)
                | Declaration::SpecModule(_)
                | Declaration::GenericProc(_)
                | Declaration::GenericProcInstance(_) => {}
            }
        }
        Ok(())
    }

    fn check_initializers(&mut self, declarations: &[Declaration]) -> Result<(), Error> {
        let mut first_error = None;

        for declaration in declarations {
            let Declaration::Var(vars) = declaration else {
                continue;
            };
            for var in vars {
                let result = (|| -> Result<(), Error> {
                    let ty = self.resolve_mode(&var.mode)?;
                    let Some(init) = &var.init else {
                        return Ok(());
                    };
                    let value_ty = self.check_expr_for_mode(&init.value, ty)?;
                    self.ensure_assignable(ty, value_ty, init.value.span())?;
                    self.check_constant_range(&init.value, ty)
                })();

                if let Err(error) = result {
                    self.push_diagnostic(error.clone());
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }

        first_error.map_or(Ok(()), Err)
    }

    fn check_proc_bodies(&mut self, declarations: &[Declaration]) -> Result<(), Error> {
        let mut first_error = None;

        for declaration in declarations {
            if let Declaration::Proc(proc_def) = declaration {
                if let Err(error) = self.check_proc(proc_def) {
                    self.push_diagnostic(error.clone());
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }

        first_error.map_or(Ok(()), Err)
    }

    fn check_proc(&mut self, proc_def: &ProcDef) -> Result<(), Error> {
        self.scopes.push(Scope::default());
        let result = self.check_proc_body(proc_def);
        self.scopes.pop();
        result
    }

    fn check_proc_body(&mut self, proc_def: &ProcDef) -> Result<(), Error> {
        let mut seen = HashSet::new();
        let setup = (|| {
            for param in &proc_def.params {
                if !seen.insert(param.name.as_str()) {
                    return Err(Error::coded(
                        "E0402",
                        param.span,
                        format!("duplicate parameter `{}`", param.name),
                    ));
                }
                let ty = self.resolve_mode(&param.mode)?;
                self.declare_var(&param.name, ty, param.span)?;
            }

            self.collect_declarations(&proc_def.declarations)?;
            reject_nested_procs(&proc_def.declarations)?;
            self.check_initializers(&proc_def.declarations)?;
            self.check_proc_bodies(&proc_def.declarations)?;

            match &proc_def.result {
                Some(mode) => self.resolve_mode(mode).map(Some),
                None => Ok(None),
            }
        })();

        let result_type = setup?;

        self.push_exception_set();
        self.results.push(result_type);
        let body = self.check_actions(&proc_def.actions);
        let incoming = self.take_exception_set();
        let escaped = body.and_then(|()| {
            if let Some(handler) = &proc_def.handler {
                self.check_handler_flow(handler, incoming)
            } else {
                Ok(incoming)
            }
        });
        self.results.pop();

        let escaped = escaped?;
        let declared: HashSet<&str> = proc_def.exceptions.iter().map(String::as_str).collect();
        let mut unlisted: Vec<&String> = escaped
            .iter()
            .filter(|exception| !declared.contains(exception.as_str()))
            .collect();
        unlisted.sort();
        if let Some(exception) = unlisted.first() {
            return Err(Error::coded(
                "E0410",
                proc_def.span,
                format!(
                    "exception `{exception}` may escape procedure `{}` but is not listed in EXCEPTIONS",
                    proc_def.name
                ),
            ));
        }
        Ok(())
    }

    fn declare_var(&mut self, name: &str, ty: Type, span: Span) -> Result<(), Error> {
        if let Type::Moreta(id) = ty {
            if self.modes.get_moreta(id).abstract_mode {
                return Err(Error::coded(
                    "E0400",
                    span,
                    "abstract module mode cannot be instantiated",
                ));
            }
        }
        let scope = self.scopes.last_mut().expect("scope stack is never empty");
        if scope.vars.contains_key(name)
            || scope.procs.contains_key(name)
            || scope.set_values.contains_key(name)
        {
            return Err(Error::coded(
                "E0402",
                span,
                format!("duplicate declaration of `{name}`"),
            ));
        }
        scope.vars.insert(name.to_owned(), ty);
        Ok(())
    }

    fn declare_proc(&mut self, name: &str, signature: ProcType, span: Span) -> Result<(), Error> {
        let scope = self.scopes.last_mut().expect("scope stack is never empty");
        if scope.vars.contains_key(name)
            || scope.procs.contains_key(name)
            || scope.set_values.contains_key(name)
        {
            return Err(Error::coded(
                "E0402",
                span,
                format!("duplicate declaration of `{name}`"),
            ));
        }
        scope.procs.insert(name.to_owned(), signature);
        Ok(())
    }

    fn preload_module_modes(&mut self, module: &Module) -> Result<(), Error> {
        let modes = self
            .reaches
            .get(module.span)
            .map(|node| node.interface.modes.clone())
            .unwrap_or_else(|| self.module_modes[self.current_module].clone());
        let scope = self.scopes.last_mut().expect("scope stack is never empty");
        for (name, ty) in modes {
            if scope.modes.contains_key(&name) {
                return Err(Error::coded(
                    "E0400",
                    module.span,
                    format!("mode `{name}` is already visible in this module"),
                ));
            }
            scope.modes.insert(name, ty);
        }
        Ok(())
    }

    fn signature_from_params(
        &self,
        params: &[Param],
        result: &Option<ModeRef>,
        exceptions: &[String],
        span: Span,
    ) -> Result<ProcType, Error> {
        let mut param_types = Vec::with_capacity(params.len());
        let mut passing = Vec::with_capacity(params.len());
        let mut seen = HashSet::new();
        for param in params {
            if !seen.insert(param.name.clone()) {
                return Err(Error::coded(
                    "E0402",
                    param.span,
                    format!("duplicate parameter `{}`", param.name),
                ));
            }
            param_types.push(self.resolve_mode(&param.mode)?);
            passing.push(param.passing);
        }
        let result = match result {
            Some(mode) => Some(self.resolve_mode(mode)?),
            None => None,
        };
        let mut exception_names = HashSet::new();
        for exception in exceptions {
            if !exception_names.insert(exception.as_str()) {
                return Err(Error::coded(
                    "E0402",
                    span,
                    format!("duplicate exception name `{exception}` in EXCEPTIONS list"),
                ));
            }
        }
        Ok(ProcType {
            params: param_types,
            passing,
            result,
            exceptions: Some(exceptions.to_vec()),
        })
    }

    fn resolve_mode(&self, mode: &ModeRef) -> Result<Type, Error> {
        if mode.powerset {
            let member = match crate::types::resolve_mode(&mode.name) {
                Some(Type::Int(kind)) => crate::types::PowerSetMember::Int(kind),
                Some(Type::Bool) => crate::types::PowerSetMember::Bool,
                Some(Type::Char) => crate::types::PowerSetMember::Char,
                Some(Type::WChar) => crate::types::PowerSetMember::WChar,
                _ => {
                    return Err(Error::coded(
                        "E0400",
                        mode.span,
                        "POWERSET member mode must be INT/SHORT_INT/LONG_INT, BOOL, CHAR, or WCHAR",
                    ));
                }
            };
            return Ok(Type::PowerSet(crate::types::PowerSetType { member }));
        }
        if mode.access {
            return Ok(Type::Access(crate::types::AccessType {
                indexed: mode.access_indexed,
                index: mode
                    .access_index
                    .map(|bounds| crate::types::ArrayDimension {
                        low: bounds.low,
                        high: bounds.high,
                    }),
            }));
        }
        if !mode.arrays.is_empty() {
            let element_mode = ModeRef {
                name: mode.name.clone(),
                arrays: Vec::new(),
                string_length: mode.string_length,
                varying: mode.varying,
                access: mode.access,
                powerset: mode.powerset,
                access_indexed: mode.access_indexed,
                access_index: mode.access_index,
                span: mode.span,
            };
            let base = self.resolve_mode(&element_mode)?;
            let Some(element) = crate::types::ArrayElement::from_type(base) else {
                return Err(Error::coded(
                    "E0400",
                    mode.span,
                    format!(
                        "array elements must be scalar or structure modes, found {}",
                        base.describe()
                    ),
                ));
            };
            return crate::types::array_type_from_bounds(&mode.arrays, element)
                .ok_or_else(|| Error::coded("E0400", mode.span, "unsupported array dimensions"));
        }
        if let Some(length) = mode.string_length {
            return Ok(match mode.name.as_str() {
                "WCHARS" => Type::WBoundedString {
                    length,
                    varying: mode.varying,
                },
                "BOOLS" => Type::BoundedBits {
                    length,
                    varying: mode.varying,
                },
                _ => Type::BoundedString {
                    length,
                    varying: mode.varying,
                },
            });
        }
        for scope in self.scopes[self.module_floor..].iter().rev() {
            if let Some(ty) = scope.modes.get(&mode.name) {
                return Ok(*ty);
            }
            if let Some(exports) = scope.exports.get(&mode.name) {
                if let [ReachBinding {
                    kind: ReachKind::Mode(ty),
                    meta,
                    ..
                }] = exports.as_slice()
                {
                    if meta.prefix.is_empty() {
                        return Ok(*ty);
                    }
                }
            }
        }
        program::resolve_mode(mode).map_err(map_program_error)
    }

    fn lookup_set_value(&self, name: &str) -> Option<(Type, i128)> {
        for scope in self.scopes[self.module_floor..].iter().rev() {
            if let Some(value) = scope.set_values.get(name) {
                return Some(*value);
            }
        }
        None
    }

    fn lookup_var(&self, name: &str) -> Option<Type> {
        for scope in self.scopes[self.module_floor..].iter().rev() {
            if let Some(ty) = scope.vars.get(name) {
                return Some(*ty);
            }
            if let Some(exports) = scope.exports.get(name) {
                if let [ReachBinding {
                    kind: ReachKind::Var(ty),
                    meta,
                    ..
                }] = exports.as_slice()
                {
                    if meta.prefix.is_empty() {
                        return Some(*ty);
                    }
                }
            }
        }
        None
    }

    fn lookup_proc(&self, name: &str) -> Option<ProcType> {
        for scope in self.scopes[self.module_floor..].iter().rev() {
            if let Some(signature) = scope.procs.get(name) {
                return Some(signature.clone());
            }
            if let Some(exports) = scope.exports.get(name) {
                if let [ReachBinding {
                    kind: ReachKind::Proc(signature),
                    meta,
                    ..
                }] = exports.as_slice()
                {
                    if meta.prefix.is_empty() {
                        return Some(signature.clone());
                    }
                }
            }
        }
        None
    }

    fn check_block(&mut self, actions: &[Action], span: Span) -> Result<(), Error> {
        self.scopes.push(Scope::default());
        self.seed_reach_exports_at(span);
        let result = self.check_actions(actions);
        self.scopes.pop();
        result
    }

    fn check_actions(&mut self, actions: &[Action]) -> Result<(), Error> {
        let mut first_error = None;

        for action in actions {
            if let Err(error) = self.check_action(action) {
                self.push_diagnostic(error.clone());
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }

        first_error.map_or(Ok(()), Err)
    }

    fn check_action(&mut self, action: &Action) -> Result<(), Error> {
        match action {
            Action::Assign {
                target,
                op,
                value,
                span,
            } => {
                let target_ty = self.location_type(target)?;
                self.types.insert(target.span(), target_ty);
                let value_ty = self.check_expr_for_mode(value, target_ty)?;

                if *op == AssignOp::Assign {
                    self.ensure_assignable(target_ty, value_ty, *span)?;
                } else {
                    let binary = binary_for_assign(*op);
                    let result_ty = self.check_binary_types(binary, target_ty, value_ty, *span)?;
                    self.ensure_assignable(target_ty, result_ty, *span)?;
                }

                self.check_constant_range(value, target_ty)
            }
            Action::Assert { condition, .. } => {
                self.record_exception("ASSERTFAIL");
                let ty = self.check_expr(condition)?;
                if ty != Type::Bool {
                    return Err(Error::coded(
                        "E0404",
                        condition.span(),
                        format!("ASSERT requires BOOL, found {}", ty.describe()),
                    ));
                }
                Ok(())
            }
            Action::Call { callee, span } => {
                if let Expr::MethodCall {
                    base,
                    method,
                    args,
                    span: call_span,
                } = callee
                {
                    let result = self.check_method_call(base, method, args, *call_span, false)?;
                    if result.is_none() {
                        self.types.insert(*call_span, Type::Bool);
                    }
                    Ok(())
                } else {
                    let Expr::Apply { name, args, .. } = callee else {
                        return Err(Error::coded("E0400", *span, "expected a procedure call"));
                    };
                    self.check_apply(name, args, *span, false)?;
                    Ok(())
                }
            }
            Action::Return { value, span } => self.check_return(value.as_ref(), *span),
            Action::Result { value, span } => self.check_result(value, *span),
            Action::If {
                branches,
                else_actions,
                else_span,
                ..
            } => {
                for branch in branches {
                    let ty = self.check_expr(&branch.condition)?;
                    if ty != Type::Bool {
                        return Err(Error::coded(
                            "E0400",
                            branch.condition.span(),
                            format!("IF condition must be BOOL, found {}", ty.describe()),
                        ));
                    }
                    self.check_block(&branch.body, branch.span)?;
                }
                if let (Some(actions), Some(span)) = (else_actions, else_span) {
                    self.check_block(actions, *span)?;
                }
                Ok(())
            }
            Action::Do {
                control,
                body,
                span,
            } => {
                match control {
                    DoControl::None | DoControl::ForEver => {}
                    DoControl::While(condition) => {
                        let ty = self.check_expr(condition)?;
                        if ty != Type::Bool {
                            return Err(Error::coded(
                                "E0407",
                                condition.span(),
                                format!("DO WHILE condition must be BOOL, found {}", ty.describe()),
                            ));
                        }
                    }
                    DoControl::For {
                        counter,
                        start,
                        end,
                        step,
                        ..
                    } => {
                        let start_ty = self.check_expr(start)?;
                        let end_ty = self.check_expr(end)?;
                        if !start_ty.is_int() || !end_ty.is_int() {
                            return Err(Error::coded(
                                "E0407",
                                *span,
                                "DO FOR bounds must be integers",
                            ));
                        }
                        let mut counter_ty = self.combine_int(start_ty, end_ty, *span)?;

                        if let Some(step_expr) = step {
                            let step_ty = self.check_expr(step_expr)?;
                            if !step_ty.is_int() {
                                return Err(Error::coded(
                                    "E0407",
                                    step_expr.span(),
                                    "DO FOR step must be an integer",
                                ));
                            }
                            counter_ty = self.combine_int(counter_ty, step_ty, step_expr.span())?;
                            if let Some(value) = constant_int(step_expr) {
                                if value <= 0 {
                                    return Err(Error::coded(
                                        "E0407",
                                        step_expr.span(),
                                        "DO FOR step must be positive",
                                    ));
                                }
                            }
                        }

                        self.scopes.push(Scope::default());
                        let result = (|| {
                            self.declare_var(counter, counter_ty, *span)?;
                            self.scopes
                                .last_mut()
                                .expect("scope stack is never empty")
                                .readonly
                                .insert(counter.clone());
                            self.seed_reach_exports_at(*span);
                            self.check_actions(body)
                        })();
                        self.scopes.pop();
                        return result;
                    }
                }
                self.check_block(body, *span)
            }
            Action::Exit {
                label: Some(name),
                span,
            } => {
                if !self.labels.iter().any(|label| label == name) {
                    return Err(Error::coded(
                        "E0407",
                        *span,
                        format!("unknown EXIT label `{name}`"),
                    ));
                }
                Ok(())
            }
            Action::Exit { label: None, .. } => Ok(()),
            Action::Case {
                selector,
                alternatives,
                else_actions,
                else_span,
                ..
            } => {
                let selector_ty = self.check_expr(selector)?;
                if !selector_ty.is_int() {
                    return Err(Error::coded(
                        "E0407",
                        selector.span(),
                        "CASE selector must be an integer in v0",
                    ));
                }

                for alternative in alternatives {
                    for label in &alternative.labels {
                        match label {
                            CaseLabel::Value(value) => {
                                self.check_case_label_value(*value, selector_ty, alternative.span)?;
                            }
                            CaseLabel::Range { low, high } => {
                                self.check_case_label_value(*low, selector_ty, alternative.span)?;
                                self.check_case_label_value(*high, selector_ty, alternative.span)?;
                            }
                            CaseLabel::Else | CaseLabel::Any => {}
                        }
                    }
                    self.check_block(&alternative.body, alternative.span)?;
                }

                if let (Some(actions), Some(span)) = (else_actions, else_span) {
                    self.check_block(actions, *span)?;
                }
                Ok(())
            }
            Action::Labelled { label, action, .. } => {
                if self.labels.iter().any(|existing| existing == label) {
                    return Err(Error::coded(
                        "E0402",
                        action.span(),
                        format!("duplicate label `{label}`"),
                    ));
                }
                self.labels.push(label.clone());
                let result = self.check_action(action);
                self.labels.pop();
                result
            }
            Action::Begin {
                declarations,
                actions,
                span,
            } => self.check_begin(declarations, actions, *span, None),
            Action::Handled {
                action, handler, ..
            } => {
                if let Action::Begin {
                    declarations,
                    actions,
                    span,
                } = action.as_ref()
                {
                    self.check_begin(declarations, actions, *span, Some(handler))
                } else {
                    self.push_exception_set();
                    let inner = self.check_action(action);
                    let incoming = self.take_exception_set();
                    inner?;
                    let escaping = self.check_handler_flow(handler, incoming)?;
                    self.record_exceptions(escaping);
                    Ok(())
                }
            }
            Action::Empty { .. } => Ok(()),
            Action::Cause { name, .. } => {
                self.record_exception(name);
                Ok(())
            }
            Action::Module(module) => self.check_nested_module(module),
        }
    }

    fn check_return(&mut self, value: Option<&Expr>, span: Span) -> Result<(), Error> {
        match self.results.last().copied() {
            None => Err(Error::coded("E0408", span, "RETURN outside a procedure")),
            Some(None) => {
                if value.is_some() {
                    Err(Error::coded(
                        "E0408",
                        span,
                        "RETURN with a value in a procedure that has no result",
                    ))
                } else {
                    Ok(())
                }
            }
            Some(Some(result_ty)) => {
                if let Some(value) = value {
                    let value_ty = self.check_expr_for_mode(value, result_ty)?;
                    self.ensure_assignable(result_ty, value_ty, value.span())?;
                }
                Ok(())
            }
        }
    }

    fn check_result(&mut self, value: &Expr, span: Span) -> Result<(), Error> {
        match self.results.last().copied() {
            Some(Some(result_ty)) => {
                let value_ty = self.check_expr_for_mode(value, result_ty)?;
                self.ensure_assignable(result_ty, value_ty, span)
            }
            _ => Err(Error::coded(
                "E0408",
                span,
                "RESULT outside a value procedure",
            )),
        }
    }

    fn location_type(&mut self, target: &Expr) -> Result<Type, Error> {
        match target {
            Expr::Field { base, field, span } => {
                if let Some(root) = field_root(target) {
                    if self.is_readonly(root) {
                        return Err(Error::coded(
                            "E0407",
                            *span,
                            format!("`{root}` is a loop counter and is not a location"),
                        ));
                    }
                }
                if !self.is_field_location(target) {
                    return Err(Error::coded(
                        "E0400",
                        *span,
                        "assignment target must be a variable, array element, or field of one",
                    ));
                }
                let ty = self.check_field(base, field, *span)?;
                Ok(ty)
            }
            Expr::ArraySlice {
                name,
                first,
                last,
                up,
                span,
            } => {
                if self.is_readonly(name) {
                    return Err(Error::coded(
                        "E0407",
                        *span,
                        format!("`{name}` is a loop counter and is not a location"),
                    ));
                }
                self.check_slice(name, first, last, *up, *span)
            }
            Expr::Apply { name, args, span } => {
                let Some(array) = self.lookup_var(name) else {
                    return Err(Error::coded(
                        "E0400",
                        *span,
                        "assignment target must be a name, field, or array element",
                    ));
                };
                if !matches!(array, Type::Array(_) | Type::MultiArray(_)) {
                    return Err(Error::coded(
                        "E0400",
                        *span,
                        "assignment target must be a name, field, or array element",
                    ));
                }
                self.check_array_index(name, args, array, *span)
            }
            Expr::Name { name, span } => {
                if self.is_readonly(name) {
                    return Err(Error::coded(
                        "E0407",
                        *span,
                        format!("`{name}` is a loop counter and is not a location"),
                    ));
                }
                self.lookup_var(name).ok_or_else(|| {
                    Error::coded("E0403", *span, format!("unknown variable `{name}`"))
                })
            }
            _ => Err(Error::coded(
                "E0400",
                target.span(),
                "assignment target must be a name or field",
            )),
        }
    }

    fn check_slice(
        &mut self,
        name: &str,
        first: &Expr,
        last: &Expr,
        up: bool,
        span: Span,
    ) -> Result<Type, Error> {
        let array = match self.lookup_var(name) {
            Some(Type::Array(array)) => array,
            Some(Type::MultiArray(_)) => {
                return Err(Error::coded(
                    "E0409",
                    span,
                    "array slices of multidimensional arrays are not implemented yet",
                ));
            }
            _ => {
                return Err(Error::coded(
                    "E0400",
                    span,
                    format!("array slice requires an array, but `{name}` is not one"),
                ));
            }
        };
        let first_ty = self.check_expr(first)?;
        let last_ty = self.check_expr(last)?;
        if !first_ty.is_int() || !last_ty.is_int() {
            return Err(Error::coded(
                "E0400",
                span,
                "array slice bounds must be integers",
            ));
        }
        let first_value = constant_int(first);
        let last_value = constant_int(last);
        let (Some(first_value), Some(last_value)) = (first_value, last_value) else {
            if up && matches!(last_value, Some(size) if size < 1) {
                return Err(Error::coded(
                    "E0400",
                    last.span(),
                    "array slice size must be positive",
                ));
            }
            return Ok(Type::DynamicArray {
                low: array.low,
                element: array.element,
            });
        };
        let (lower, upper) = if up {
            if last_value < 1 {
                return Err(Error::coded(
                    "E0400",
                    last.span(),
                    "array slice size must be positive",
                ));
            }
            let Some(upper) = first_value.checked_add(last_value - 1) else {
                return Err(Error::coded("E0400", span, "array slice bounds overflow"));
            };
            (first_value, upper)
        } else {
            (first_value, last_value)
        };
        if lower > upper || lower < array.low || upper > array.high {
            return Err(Error::coded(
                "E0400",
                span,
                format!(
                    "array slice `{lower}:{upper}` is outside the array bounds `{}:{}`",
                    array.low, array.high
                ),
            ));
        }
        let count = upper - lower + 1;
        let Some(high) = array.low.checked_add(count - 1) else {
            return Err(Error::coded("E0400", span, "array slice bounds overflow"));
        };
        Ok(Type::Array(crate::types::ArrayType {
            low: array.low,
            high,
            element: array.element,
        }))
    }

    fn check_method_call(
        &mut self,
        base: &Expr,
        method: &str,
        args: &[Expr],
        span: Span,
        require_result: bool,
    ) -> Result<Option<Type>, Error> {
        let Expr::Name {
            span: name_span, ..
        } = base
        else {
            return Err(Error::coded(
                "E0400",
                base.span(),
                "module-mode method calls currently require a named object",
            ));
        };
        let base_ty = self.check_expr(base)?;
        let Type::Moreta(id) = base_ty else {
            return Err(Error::coded(
                "E0400",
                *name_span,
                "method call requires a module-mode object",
            ));
        };
        let Some((signature, proc_span)) = self.modes.moreta_proc(id, method) else {
            return Err(Error::coded(
                "E0400",
                span,
                format!("unknown procedure component `{method}`"),
            ));
        };
        if signature.params.len() != args.len() {
            return Err(Error::coded(
                "E0400",
                span,
                format!(
                    "procedure component `{method}` expects {} argument(s), found {}",
                    signature.params.len(),
                    args.len()
                ),
            ));
        }
        for (arg, (param_ty, passing)) in args
            .iter()
            .zip(signature.params.iter().zip(&signature.passing))
        {
            match passing {
                Passing::In => {
                    let arg_ty = self.check_expr_for_mode(arg, *param_ty)?;
                    self.ensure_assignable(*param_ty, arg_ty, arg.span())?;
                }
                Passing::Out | Passing::InOut => {
                    let target_ty = self.location_type(arg)?;
                    self.types.insert(arg.span(), target_ty);
                    if target_ty != *param_ty {
                        return Err(Error::coded(
                            "E0404",
                            arg.span(),
                            format!(
                                "argument for {passing:?} parameter must have mode {}, found {}",
                                param_ty.describe(),
                                target_ty.describe()
                            ),
                        ));
                    }
                }
                Passing::Loc => {
                    if !matches!(arg, Expr::Name { .. }) {
                        return Err(Error::coded(
                            "E0409",
                            arg.span(),
                            "LOC arguments currently require a simple variable",
                        ));
                    }
                    let target_ty = self.location_type(arg)?;
                    self.types.insert(arg.span(), target_ty);
                    if target_ty != *param_ty {
                        return Err(Error::coded(
                            "E0404",
                            arg.span(),
                            format!(
                                "LOC argument must have mode {}, found {}",
                                param_ty.describe(),
                                target_ty.describe()
                            ),
                        ));
                    }
                }
            }
        }
        self.resolutions.insert(
            span,
            ApplyResolution::Method {
                proc_span,
                result: signature.result,
                passing: signature.passing.clone(),
            },
        );
        if require_result && signature.result.is_none() {
            return Err(Error::coded(
                "E0400",
                span,
                format!("procedure component `{method}` does not return a value"),
            ));
        }
        Ok(signature.result)
    }

    fn check_field(&mut self, base: &Expr, field: &str, span: Span) -> Result<Type, Error> {
        let base_ty = self.check_expr(base)?;
        if let Type::Moreta(id) = base_ty {
            let Some(ty) = self.modes.moreta_var(id, field) else {
                return Err(Error::coded(
                    "E0403",
                    span,
                    format!(
                        "unknown variable component `{field}` in module mode `{}`",
                        self.modes.get_moreta(id).name
                    ),
                ));
            };
            return Ok(ty);
        }

        let Type::Struct(id) = base_ty else {
            return Err(Error::coded(
                "E0400",
                span,
                format!(
                    "field selection requires a structure or module mode, found {}",
                    base_ty.describe()
                ),
            ));
        };

        let mode = self.modes.get(id);
        let Some((_, info)) = mode
            .fields
            .iter()
            .enumerate()
            .find(|(_, info)| info.name == field)
        else {
            return Err(Error::coded(
                "E0400",
                span,
                format!("unknown field `{field}` in structure mode `{}`", mode.name),
            ));
        };

        let in_defining_module = match &self.current_nested_module_name {
            Some(current) => mode.module_name.as_deref() == Some(current.as_str()),
            None => mode.module == self.current_module,
        };
        if !in_defining_module && mode.forbidden.contains(field) {
            return Err(Error::coded(
                "E0400",
                span,
                format!("field `{field}` is FORBIDden outside its defining module"),
            ));
        }

        Ok(info.ty)
    }

    fn is_field_location(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Name { .. } => true,
            Expr::Field { base, .. } => self.is_field_location(base),
            Expr::Apply { name, args, .. } => {
                args.len() == 1 && matches!(self.lookup_var(name), Some(Type::Array(_)))
            }
            _ => false,
        }
    }

    fn is_readonly(&self, name: &str) -> bool {
        for scope in self.scopes[self.module_floor..].iter().rev() {
            if scope.vars.contains_key(name) {
                return scope.readonly.contains(name);
            }
        }
        false
    }

    fn check_expr(&mut self, expr: &Expr) -> Result<Type, Error> {
        let ty = self.check_expr_inner(expr)?;
        self.types.insert(expr.span(), ty);
        Ok(ty)
    }

    fn check_expr_inner(&mut self, expr: &Expr) -> Result<Type, Error> {
        match expr {
            Expr::Float { .. } => Ok(Type::Float),
            Expr::Integer { value, span, .. } => {
                let long = IntKind::Long;
                let Some((low, high)) = types::int_bounds(long, self.profile) else {
                    return Err(Error::coded(
                        "E0400",
                        *span,
                        "integer widths above 127 are not supported",
                    ));
                };
                if !(*value >= low && *value <= high) {
                    return Err(Error::coded(
                        "E0405",
                        *span,
                        format!("integer literal `{value}` is outside the range of &INT"),
                    ));
                }
                Ok(Type::IntLiteral)
            }
            Expr::Bool { .. } => Ok(Type::Bool),
            Expr::Char { value, span } => {
                if (*value as u32) > 0xFF {
                    return Err(Error::coded(
                        "E0405",
                        *span,
                        "character literal is outside the CHAR range",
                    ));
                }
                Ok(Type::Char)
            }
            Expr::String { value, span } => {
                if value.chars().any(|ch| (ch as u32) > 0xFF) {
                    return Err(Error::coded(
                        "E0405",
                        *span,
                        "character string literal contains a character outside the CHAR range",
                    ));
                }
                Ok(Type::String)
            }
            Expr::WideChar { value, span } => {
                if (*value as u32) > 0xFFFF {
                    return Err(Error::coded(
                        "E0405",
                        *span,
                        "wide character literal is outside the UCS-2 range",
                    ));
                }
                Ok(Type::WChar)
            }
            Expr::WideString { value, span } => {
                if value.chars().any(|ch| (ch as u32) > 0xFFFF) {
                    return Err(Error::coded(
                        "E0405",
                        *span,
                        "wide character string literal contains a character outside the UCS-2 range",
                    ));
                }
                Ok(Type::WString)
            }
            Expr::Bits { .. } => Ok(Type::Bits),
            Expr::ArrayTuple { elements, .. } => {
                for element in elements {
                    self.check_expr(&element.value)?;
                    if let Some(end) = &element.range_end {
                        self.check_expr(end)?;
                    }
                }
                Ok(Type::ArrayTuple)
            }
            Expr::Name { name, span } => {
                if let Some((ty, value)) = self.lookup_set_value(name) {
                    self.resolutions
                        .insert(*span, ApplyResolution::SetElement { ty, value });
                    Ok(ty)
                } else if let Some(ty) = self.lookup_var(name) {
                    Ok(ty)
                } else if self.lookup_proc(name).is_some() {
                    Err(Error::coded(
                        "E0400",
                        *span,
                        format!("procedure `{name}` must be called with `()`"),
                    ))
                } else {
                    Err(Error::coded(
                        "E0403",
                        *span,
                        format!("unknown name `{name}`"),
                    ))
                }
            }
            Expr::Apply { name, args, span } => match self.check_apply(name, args, *span, true)? {
                Some(ty) => Ok(ty),
                None => Err(Error::coded(
                    "E0400",
                    *span,
                    format!("procedure `{name}` does not return a value"),
                )),
            },
            Expr::Unary { op, operand, span } => {
                let ty = self.check_expr(operand)?;
                match op {
                    UnaryOp::Neg => {
                        if ty.is_int() || ty == Type::Float {
                            Ok(ty)
                        } else {
                            Err(Error::coded(
                                "E0404",
                                *span,
                                format!("unary `-` requires an integer, found {}", ty.describe()),
                            ))
                        }
                    }
                    UnaryOp::Not => {
                        if ty == Type::Bool {
                            Ok(Type::Bool)
                        } else if is_bits_type(ty) {
                            Ok(ty)
                        } else {
                            Err(Error::coded(
                                "E0404",
                                *span,
                                format!(
                                    "NOT requires BOOL or a bit string, found {}",
                                    ty.describe()
                                ),
                            ))
                        }
                    }
                }
            }
            Expr::Binary {
                op,
                left,
                right,
                span,
            } => {
                let mut left_ty = if matches!(left.as_ref(), Expr::ArrayTuple { .. }) {
                    Type::ArrayTuple
                } else {
                    self.check_expr(left)?
                };
                let mut right_ty = if matches!(right.as_ref(), Expr::ArrayTuple { .. }) {
                    Type::ArrayTuple
                } else {
                    self.check_expr(right)?
                };
                if matches!(op, BinaryOp::Eq | BinaryOp::Ne) {
                    if left_ty == Type::ArrayTuple
                        && matches!(right_ty, Type::Array(_) | Type::PowerSet(_))
                    {
                        left_ty = self.check_expr_for_mode(left, right_ty)?;
                    } else if right_ty == Type::ArrayTuple
                        && matches!(left_ty, Type::Array(_) | Type::PowerSet(_))
                    {
                        right_ty = self.check_expr_for_mode(right, left_ty)?;
                    }
                }
                self.check_binary_types(*op, left_ty, right_ty, *span)
            }
            Expr::Field { base, field, span } => self.check_field(base, field, *span),
            Expr::MethodCall {
                base,
                method,
                args,
                span,
            } => self
                .check_method_call(base, method, args, *span, true)
                .map(|result| result.expect("require_result method always has a type")),
            Expr::ArraySlice {
                name,
                first,
                last,
                up,
                span,
            } => self.check_slice(name, first, last, *up, *span),
        }
    }

    fn check_binary_types(
        &self,
        op: BinaryOp,
        left: Type,
        right: Type,
        span: Span,
    ) -> Result<Type, Error> {
        match op {
            BinaryOp::And | BinaryOp::AndIf | BinaryOp::Or | BinaryOp::OrIf | BinaryOp::Xor => {
                if matches!(op, BinaryOp::And | BinaryOp::Or | BinaryOp::Xor)
                    && bits_compatible(left, right)
                {
                    Ok(left)
                } else {
                    self.expect_bool_pair(left, right, span)?;
                    Ok(Type::Bool)
                }
            }
            BinaryOp::Eq | BinaryOp::Ne => {
                if left.is_int() && right.is_int() {
                    self.combine_int(left, right, span)?;
                    Ok(Type::Bool)
                } else if (left == right
                    && matches!(
                        left,
                        Type::Bool
                            | Type::Char
                            | Type::WChar
                            | Type::Struct(_)
                            | Type::Set(_)
                            | Type::Float
                    ))
                    || strings_compatible(left, right)
                    || bits_compatible(left, right)
                    || (matches!(left, Type::PowerSet(_)) && left == right)
                    || arrays_compatible(left, right)
                {
                    Ok(Type::Bool)
                } else {
                    Err(Error::coded(
                        "E0404",
                        span,
                        format!(
                            "cannot compare {} with {}",
                            left.describe(),
                            right.describe()
                        ),
                    ))
                }
            }
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                if left.is_int() && right.is_int() {
                    self.combine_int(left, right, span)?;
                    Ok(Type::Bool)
                } else if ((left == Type::Char && right == Type::Char)
                    || (left == Type::WChar && right == Type::WChar)
                    || (left == Type::Float && right == Type::Float)
                    || (matches!(left, Type::PowerSet(_)) && left == right))
                    || strings_compatible(left, right)
                    || bits_compatible(left, right)
                {
                    Ok(Type::Bool)
                } else {
                    Err(Error::coded(
                        "E0404",
                        span,
                        format!(
                            "cannot order {} against {}",
                            left.describe(),
                            right.describe()
                        ),
                    ))
                }
            }
            BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Mod
            | BinaryOp::Rem
            | BinaryOp::Pow => {
                if left.is_int() && right.is_int() {
                    self.combine_int(left, right, span)
                } else if matches!((left, right), (Type::PowerSet(a), Type::PowerSet(b)) if a == b)
                    && matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul)
                {
                    Ok(left)
                } else if left == Type::Float
                    && right == Type::Float
                    && matches!(
                        op,
                        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div
                    )
                {
                    Ok(Type::Float)
                } else {
                    Err(Error::coded("E0404", 
                        span,
                        format!(
                            "arithmetic requires integers or matching FLOAT operands, found {} and {}",
                            left.describe(),
                            right.describe()
                        ),
                    ))
                }
            }
            BinaryOp::In => {
                let Type::PowerSet(set) = right else {
                    return Err(Error::coded(
                        "E0400",
                        span,
                        "the right operand of IN must be a powerset",
                    ));
                };
                if !member_compatible(set.member, left) {
                    return Err(Error::coded(
                        "E0400",
                        span,
                        format!("{} is not a member of this powerset", left.describe()),
                    ));
                }
                Ok(Type::Bool)
            }
            BinaryOp::Concat => {
                if strings_compatible(left, right) {
                    Ok(if is_wide_string_type(left) {
                        Type::WString
                    } else {
                        Type::String
                    })
                } else if bits_compatible(left, right) {
                    Ok(Type::Bits)
                } else {
                    Err(Error::coded(
                        "E0400",
                        span,
                        format!(
                            "`//` requires character strings, found {} and {}",
                            left.describe(),
                            right.describe()
                        ),
                    ))
                }
            }
        }
    }

    fn expect_bool_pair(&self, left: Type, right: Type, span: Span) -> Result<(), Error> {
        if left == Type::Bool && right == Type::Bool {
            Ok(())
        } else {
            Err(Error::coded(
                "E0404",
                span,
                format!(
                    "boolean operator requires BOOL, found {} and {}",
                    left.describe(),
                    right.describe()
                ),
            ))
        }
    }

    fn combine_int(&self, left: Type, right: Type, span: Span) -> Result<Type, Error> {
        match (left, right) {
            (Type::IntLiteral, Type::IntLiteral) => Ok(Type::IntLiteral),
            (Type::Int(kind), Type::IntLiteral) | (Type::IntLiteral, Type::Int(kind)) => {
                Ok(Type::Int(kind))
            }
            (Type::Int(left_kind), Type::Int(right_kind)) if left_kind == right_kind => {
                Ok(Type::Int(left_kind))
            }
            (Type::Int(left_kind), Type::Int(right_kind)) => Err(Error::coded(
                "E0404",
                span,
                format!(
                    "integer mode mismatch: {} and {}; use an explicit conversion",
                    left_kind.name(),
                    right_kind.name()
                ),
            )),
            _ => Err(Error::coded(
                "E0404",
                span,
                format!(
                    "mixing {} with {} is not allowed",
                    left.describe(),
                    right.describe()
                ),
            )),
        }
    }

    fn check_apply(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
        require_result: bool,
    ) -> Result<Option<Type>, Error> {
        if let Some(op) = crate::builtins::lookup_association(name) {
            use crate::builtins::AssociationOp;
            let association_arg = |checker: &mut Checker, arg: &Expr| -> Result<(), Error> {
                let Expr::Name { span, .. } = arg else {
                    return Err(Error::coded(
                        "E0400",
                        arg.span(),
                        "association/access argument must be a location",
                    ));
                };
                let ty = checker.check_expr(arg)?;
                if ty != Type::Association {
                    return Err(Error::coded(
                        "E0400",
                        *span,
                        "expected an ASSOCIATION location",
                    ));
                }
                Ok(())
            };
            let access_type =
                |checker: &mut Checker, arg: &Expr| -> Result<crate::types::AccessType, Error> {
                    let Expr::Name { span, .. } = arg else {
                        return Err(Error::coded(
                            "E0400",
                            arg.span(),
                            "association/access argument must be a location",
                        ));
                    };
                    let ty = checker.check_expr(arg)?;
                    let Type::Access(access) = ty else {
                        return Err(Error::coded("E0400", *span, "expected an ACCESS location"));
                    };
                    Ok(access)
                };
            let mut has_index = false;
            let mut has_target = false;
            let result = match op {
                AssociationOp::Associate => {
                    let [association, path] = args else {
                        return Err(Error::coded(
                            "E0400",
                            span,
                            "ASSOCIATE takes an association and a path",
                        ));
                    };
                    association_arg(self, association)?;
                    let path_ty = self.check_expr(path)?;
                    if !is_narrow_string_type(path_ty) {
                        return Err(Error::coded(
                            "E0400",
                            path.span(),
                            "ASSOCIATE path must be CHARS",
                        ));
                    }
                    None
                }
                AssociationOp::IsAssociated
                | AssociationOp::Existing
                | AssociationOp::Readable
                | AssociationOp::Writeable
                | AssociationOp::Indexable
                | AssociationOp::Sequencible
                | AssociationOp::Variable => {
                    let [association] = args else {
                        return Err(Error::coded(
                            "E0400",
                            span,
                            format!("{name} takes an association location"),
                        ));
                    };
                    association_arg(self, association)?;
                    Some(Type::Bool)
                }
                AssociationOp::Modify => {
                    let Some(association) = args.first() else {
                        return Err(Error::coded(
                            "E0400",
                            span,
                            "MODIFY takes an association location",
                        ));
                    };
                    association_arg(self, association)?;
                    if args.len() > 1 {
                        return Err(Error::coded(
                            "E0409",
                            span,
                            "MODIFY parameters are not implemented yet",
                        ));
                    }
                    None
                }
                AssociationOp::Dissociate | AssociationOp::Create | AssociationOp::Delete => {
                    let [association] = args else {
                        return Err(Error::coded(
                            "E0400",
                            span,
                            format!("{name} takes an association location"),
                        ));
                    };
                    association_arg(self, association)?;
                    None
                }
                AssociationOp::Connect => {
                    if args.len() != 2 && args.len() != 3 {
                        return Err(Error::coded(
                            "E0400",
                            span,
                            "CONNECT takes an access, an association, and an optional index",
                        ));
                    }
                    let access = access_type(self, &args[0])?;
                    association_arg(self, &args[1])?;
                    if args.len() == 3 {
                        if !access.indexed {
                            return Err(Error::coded(
                                "E0400",
                                span,
                                "CONNECT index requires an indexed ACCESS mode",
                            ));
                        }
                        let index_ty = self.check_expr(&args[2])?;
                        if !index_ty.is_int() {
                            return Err(Error::coded(
                                "E0400",
                                args[2].span(),
                                "CONNECT index must be an integer",
                            ));
                        }
                        has_index = true;
                    }
                    None
                }
                AssociationOp::Disconnect => {
                    let [access] = args else {
                        return Err(Error::coded(
                            "E0400",
                            span,
                            "DISCONNECT takes an access location",
                        ));
                    };
                    access_type(self, access)?;
                    None
                }
                AssociationOp::ReadRecord => {
                    let Some(access) = args.first() else {
                        return Err(Error::coded(
                            "E0400",
                            span,
                            "READRECORD takes an access location",
                        ));
                    };
                    let access = access_type(self, access)?;
                    if access.indexed {
                        match args {
                            [_, _] => {
                                let second_ty = self.check_expr(&args[1])?;
                                if second_ty.is_int() {
                                    has_index = true;
                                    Some(Type::String)
                                } else if is_narrow_string_type(second_ty)
                                    && matches!(args[1], Expr::Name { .. })
                                {
                                    has_target = true;
                                    None
                                } else {
                                    return Err(Error::coded(
                                        "E0400",
                                        args[1].span(),
                                        "READRECORD argument must be an index or CHARS location",
                                    ));
                                }
                            }
                            [_, _, _] => {
                                let index_ty = self.check_expr(&args[1])?;
                                if !index_ty.is_int() {
                                    return Err(Error::coded(
                                        "E0400",
                                        args[1].span(),
                                        "READRECORD index must be an integer",
                                    ));
                                }
                                let Expr::Name { .. } = &args[2] else {
                                    return Err(Error::coded(
                                        "E0400",
                                        args[2].span(),
                                        "READRECORD target must be a CHARS location",
                                    ));
                                };
                                let target_ty = self.check_expr(&args[2])?;
                                if !is_narrow_string_type(target_ty) {
                                    return Err(Error::coded(
                                        "E0400",
                                        args[2].span(),
                                        "READRECORD target must be a CHARS location",
                                    ));
                                }
                                has_index = true;
                                has_target = true;
                                None
                            }
                            _ => {
                                return Err(Error::coded("E0400", 
                                    span,
                                    "READRECORD takes an access, optional index, and optional target",
                                ));
                            }
                        }
                    } else {
                        match args {
                            [_] => Some(Type::String),
                            [_, target] => {
                                let Expr::Name { .. } = target else {
                                    return Err(Error::coded(
                                        "E0400",
                                        target.span(),
                                        "READRECORD target must be a CHARS location",
                                    ));
                                };
                                let target_ty = self.check_expr(target)?;
                                if !is_narrow_string_type(target_ty) {
                                    return Err(Error::coded(
                                        "E0400",
                                        target.span(),
                                        "READRECORD target must be a CHARS location",
                                    ));
                                }
                                has_target = true;
                                None
                            }
                            _ => {
                                return Err(Error::coded(
                                    "E0400",
                                    span,
                                    "READRECORD takes an access location and optional target",
                                ));
                            }
                        }
                    }
                }
                AssociationOp::WriteRecord => {
                    if args.len() != 2 && args.len() != 3 {
                        return Err(Error::coded(
                            "E0400",
                            span,
                            "WRITERECORD takes an access, an optional index, and a value",
                        ));
                    }
                    let access = access_type(self, &args[0])?;
                    let value = if args.len() == 3 {
                        if !access.indexed {
                            return Err(Error::coded(
                                "E0400",
                                span,
                                "WRITERECORD index requires an indexed ACCESS mode",
                            ));
                        }
                        let index_ty = self.check_expr(&args[1])?;
                        if !index_ty.is_int() {
                            return Err(Error::coded(
                                "E0400",
                                args[1].span(),
                                "WRITERECORD index must be an integer",
                            ));
                        }
                        has_index = true;
                        &args[2]
                    } else {
                        &args[1]
                    };
                    let value_ty = self.check_expr(value)?;
                    if !is_narrow_string_type(value_ty) {
                        return Err(Error::coded(
                            "E0400",
                            value.span(),
                            "WRITERECORD value must be CHARS",
                        ));
                    }
                    None
                }
            };
            self.resolutions.insert(
                span,
                ApplyResolution::Association {
                    op,
                    index: has_index,
                    target: has_target,
                },
            );
            return Ok(result);
        }
        if let Some(kind) = match name {
            "WRITETEXT" => Some(crate::builtins::TextIo::Write),
            "READTEXT" => Some(crate::builtins::TextIo::Read),
            _ => None,
        } {
            let (target, rest) = args.split_first().ok_or_else(|| {
                Error::coded("E0400", span, format!("`{name}` requires a text argument"))
            })?;
            let Expr::Name {
                name: target_name,
                span: target_span,
                ..
            } = target
            else {
                return Err(Error::coded(
                    "E0400",
                    target.span(),
                    "the text argument must be a CHARS location",
                ));
            };
            let target_ty = self.check_expr(target)?;
            if !is_varying_string(target_ty) {
                return Err(Error::coded("E0400", 
                    *target_span,
                    format!(
                        "text location `{target_name}` must have a varying CHARS mode (CHARS or CHARS(n) VARYING)"
                    ),
                ));
            }
            let indexed = if rest.len() >= 2 {
                let second_ty = self.check_expr(&rest[0])?;
                second_ty.is_int()
            } else {
                false
            };
            let (index, format, io_args) = if indexed {
                if rest.len() < 2 {
                    return Err(Error::coded(
                        "E0400",
                        span,
                        format!("`{name}` requires a format after the text index"),
                    ));
                }
                (Some(&rest[0]), &rest[1], &rest[2..])
            } else {
                let Some(format) = rest.first() else {
                    return Err(Error::coded(
                        "E0400",
                        span,
                        format!("`{name}` requires a format argument"),
                    ));
                };
                (None, format, &rest[1..])
            };
            if let Some(index) = index {
                let index_ty = self.check_expr(index)?;
                if !index_ty.is_int() {
                    return Err(Error::coded(
                        "E0400",
                        index.span(),
                        format!(
                            "text index must be an integer, found {}",
                            index_ty.describe()
                        ),
                    ));
                }
            }
            let format_ty = self.check_expr(format)?;
            if format_ty != Type::String {
                return Err(Error::coded(
                    "E0400",
                    format.span(),
                    format!("format text must be CHARS, found {}", format_ty.describe()),
                ));
            }
            let Expr::String { value, .. } = format else {
                return Err(Error::coded(
                    "E0400",
                    format.span(),
                    "format text must be a literal in the current subset",
                ));
            };
            let ops = crate::builtins::parse_format(value)
                .map_err(|message| Error::coded("E0400", format.span(), message))?;
            let items = crate::builtins::format_items(&ops);
            if items.len() != io_args.len() {
                return Err(Error::coded(
                    "E0400",
                    span,
                    format!(
                        "`{name}` requires {} io list argument(s), found {}",
                        items.len(),
                        io_args.len()
                    ),
                ));
            }
            for (item, argument) in items.iter().zip(io_args) {
                match item {
                    crate::builtins::FormatItem::Width => {
                        let ty = self.check_expr(argument)?;
                        if !ty.is_int() {
                            return Err(Error::coded(
                                "E0400",
                                argument.span(),
                                "variable format width must be an integer",
                            ));
                        }
                    }
                    crate::builtins::FormatItem::Conversion(code) => {
                        if matches!(kind, crate::builtins::TextIo::Read)
                            && !matches!(argument, Expr::Name { .. })
                        {
                            return Err(Error::coded(
                                "E0400",
                                argument.span(),
                                "READTEXT conversion arguments must be locations",
                            ));
                        }
                        let ty = self.check_expr(argument)?;
                        let valid = match code {
                            crate::builtins::ConversionCode::Binary
                            | crate::builtins::ConversionCode::Octal
                            | crate::builtins::ConversionCode::Hex => ty.is_int(),
                            crate::builtins::ConversionCode::Default => {
                                ty.is_int()
                                    || ty == Type::Float
                                    || matches!(ty, Type::Bool | Type::Char)
                                    || is_string_type(ty)
                            }
                            crate::builtins::ConversionCode::Scientific => ty == Type::Float,
                        };
                        if !valid {
                            return Err(Error::coded(
                                "E0400",
                                argument.span(),
                                format!(
                                    "format conversion is not compatible with {}",
                                    ty.describe()
                                ),
                            ));
                        }
                    }
                }
            }
            self.resolutions.insert(
                span,
                ApplyResolution::TextIo {
                    kind,
                    format: ops,
                    indexed: index.is_some(),
                },
            );
            return Ok(None);
        }

        if let Some(builtin) = crate::builtins::lookup(name) {
            let result = match builtin {
                crate::builtins::Builtin::Write | crate::builtins::Builtin::WriteLine => {
                    if args.len() > 1 {
                        return Err(Error::coded(
                            "E0400",
                            span,
                            format!("`{name}` takes at most one argument"),
                        ));
                    }
                    if let Some(arg) = args.first() {
                        let ty = self.check_expr(arg)?;
                        if !is_string_type(ty) {
                            return Err(Error::coded(
                                "E0400",
                                arg.span(),
                                format!(
                                    "`{name}` requires a character string, found {}",
                                    ty.describe()
                                ),
                            ));
                        }
                    }
                    None
                }
                crate::builtins::Builtin::ReadLine => {
                    if !args.is_empty() {
                        return Err(Error::coded("E0400", span, "READLINE takes no arguments"));
                    }
                    Some(Type::String)
                }
            };
            self.resolutions
                .insert(span, ApplyResolution::Builtin(builtin));
            return Ok(result);
        }

        if let Some(array @ (Type::Array(_) | Type::MultiArray(_))) = self.lookup_var(name) {
            return self.check_array_index(name, args, array, span).map(Some);
        }

        if let Some(mode) = types::resolve_mode(name) {
            if mode == Type::Float {
                if args.len() != 1 {
                    return Err(Error::coded(
                        "E0400",
                        span,
                        "conversion to FLOAT takes exactly one argument",
                    ));
                }
                let arg_ty = self.check_expr(&args[0])?;
                if !arg_ty.is_int() && arg_ty != Type::Float {
                    return Err(Error::coded(
                        "E0404",
                        args[0].span(),
                        format!(
                            "conversion to FLOAT requires an integer or FLOAT, found {}",
                            arg_ty.describe()
                        ),
                    ));
                }
                self.resolutions
                    .insert(span, ApplyResolution::FloatConversion);
                return Ok(Some(Type::Float));
            }
            let Type::Int(kind) = mode else {
                return Err(Error::coded(
                    "E0409",
                    span,
                    format!("conversion to {} is not supported yet", mode.describe()),
                ));
            };
            if args.len() != 1 {
                return Err(Error::coded(
                    "E0400",
                    span,
                    format!("conversion to {} takes exactly one argument", kind.name()),
                ));
            }
            let arg_ty = self.check_expr(&args[0])?;
            if !arg_ty.is_int() {
                return Err(Error::coded(
                    "E0404",
                    args[0].span(),
                    format!(
                        "conversion to {} requires an integer, found {}",
                        kind.name(),
                        arg_ty.describe()
                    ),
                ));
            }
            self.resolutions
                .insert(span, ApplyResolution::Conversion(kind));
            return Ok(Some(Type::Int(kind)));
        }

        let Some(signature) = self.lookup_proc(name) else {
            return Err(Error::coded(
                "E0403",
                span,
                format!("unknown name `{name}`"),
            ));
        };
        self.resolutions
            .insert(span, ApplyResolution::Procedure(signature.clone()));
        if let Some(exceptions) = &signature.exceptions {
            self.record_exceptions(exceptions.iter().cloned());
        }

        if signature.params.len() != args.len() {
            return Err(Error::coded(
                "E0400",
                span,
                format!(
                    "procedure `{name}` expects {} argument(s), found {}",
                    signature.params.len(),
                    args.len()
                ),
            ));
        }

        for (arg, (param_ty, passing)) in args
            .iter()
            .zip(signature.params.iter().zip(&signature.passing))
        {
            match passing {
                Passing::In => {
                    let arg_ty = self.check_expr_for_mode(arg, *param_ty)?;
                    self.ensure_assignable(*param_ty, arg_ty, arg.span())?;
                }
                Passing::Out | Passing::InOut => {
                    let target_ty = self.location_type(arg)?;
                    self.types.insert(arg.span(), target_ty);
                    if target_ty != *param_ty {
                        return Err(Error::coded(
                            "E0404",
                            arg.span(),
                            format!(
                                "argument for {passing:?} parameter must have mode {}, found {}",
                                param_ty.describe(),
                                target_ty.describe()
                            ),
                        ));
                    }
                }
                Passing::Loc => {
                    if !matches!(arg, Expr::Name { .. }) {
                        return Err(Error::coded(
                            "E0409",
                            arg.span(),
                            "LOC arguments currently require a simple variable",
                        ));
                    }
                    let target_ty = self.location_type(arg)?;
                    self.types.insert(arg.span(), target_ty);
                    if target_ty != *param_ty {
                        return Err(Error::coded(
                            "E0404",
                            arg.span(),
                            format!(
                                "LOC argument must have mode {}, found {}",
                                param_ty.describe(),
                                target_ty.describe()
                            ),
                        ));
                    }
                }
            }
        }

        if require_result && signature.result.is_none() {
            return Err(Error::coded(
                "E0400",
                span,
                format!("procedure `{name}` does not return a value"),
            ));
        }

        Ok(signature.result)
    }

    #[allow(clippy::too_many_arguments)]
    fn tuple_label_index(
        &self,
        value: i128,
        array: crate::types::ArrayType,
        span: Span,
        explicit: &mut [bool],
        entry_index: usize,
        entry_indices: &mut [Vec<i128>],
    ) -> Result<(), Error> {
        if value < array.low || value > array.high {
            return Err(Error::coded(
                "E0400",
                span,
                format!(
                    "array tuple label `{value}` is outside the array bounds `{}:{}`",
                    array.low, array.high
                ),
            ));
        }
        let offset = (value - array.low) as usize;
        if explicit[offset] {
            return Err(Error::coded(
                "E0400",
                span,
                format!("array tuple label `{value}` is covered more than once"),
            ));
        }
        explicit[offset] = true;
        entry_indices[entry_index].push(value);
        Ok(())
    }

    fn check_array_index(
        &mut self,
        name: &str,
        args: &[Expr],
        array: Type,
        span: Span,
    ) -> Result<Type, Error> {
        let (rank, element) = match array {
            Type::Array(array) => (1, array.element),
            Type::MultiArray(array) => (array.rank(), array.element),
            _ => {
                return Err(Error::coded(
                    "E0400",
                    span,
                    format!("`{name}` is not an array"),
                ));
            }
        };
        if args.len() != rank {
            let message = if rank == 1 {
                format!("array `{name}` requires exactly one index")
            } else {
                format!("array `{name}` requires {rank} indices")
            };
            return Err(Error::coded("E0400", span, message));
        }
        for arg in args {
            let index_ty = self.check_expr(arg)?;
            if !index_ty.is_int() {
                return Err(Error::coded(
                    "E0400",
                    arg.span(),
                    format!(
                        "array index must be an integer, found {}",
                        index_ty.describe()
                    ),
                ));
            }
        }
        self.resolutions
            .insert(span, ApplyResolution::ArrayIndex(array));
        Ok(element.ty())
    }

    /// Check a value expression in a context where the target mode is known.
    /// This is what gives array tuples their element and bounds modes.
    fn check_expr_for_mode(&mut self, value: &Expr, target: Type) -> Result<Type, Error> {
        let Expr::ArrayTuple { elements, span } = value else {
            return self.check_expr(value);
        };
        if let Type::PowerSet(set) = target {
            let member = powerset_member_type(set.member);
            for item in elements {
                if item.labels.is_some() {
                    return Err(Error::coded(
                        "E0400",
                        *span,
                        "powerset tuples do not accept labels",
                    ));
                }
                let ty = self.check_expr(&item.value)?;
                self.ensure_assignable(member, ty, item.value.span())?;
                if let Some(end) = &item.range_end {
                    let end_ty = self.check_expr(end)?;
                    self.ensure_assignable(member, end_ty, end.span())?;
                }
            }
            self.types.insert(*span, target);
            return Ok(target);
        }
        if elements.iter().any(|item| item.range_end.is_some()) {
            return Err(Error::coded(
                "E0400",
                *span,
                "ranges are only allowed in powerset tuples",
            ));
        }
        let Type::Array(array) = target else {
            return Err(Error::coded(
                "E0400",
                *span,
                format!(
                    "array literal requires a one-dimensional array target mode, found {}",
                    target.describe()
                ),
            ));
        };
        let element = array.element.ty();
        let expected = (array.high - array.low + 1).max(0) as usize;
        let labelled = elements.iter().any(|element| element.labels.is_some());
        if !labelled {
            if elements.len() != expected {
                return Err(Error::coded(
                    "E0400",
                    *span,
                    format!(
                        "array literal has {} element(s), but the target array has {expected}",
                        elements.len()
                    ),
                ));
            }
            for item in elements {
                let item_ty = self.check_expr(&item.value)?;
                self.ensure_assignable(element, item_ty, item.value.span())?;
                self.check_constant_range(&item.value, element)?;
            }
        } else {
            if elements.iter().any(|element| element.labels.is_none()) {
                return Err(Error::coded(
                    "E0400",
                    *span,
                    "array tuple entries must be all labelled or all unlabelled",
                ));
            }
            let mut explicit = vec![false; expected];
            let mut entry_indices = vec![Vec::new(); elements.len()];
            let mut explicit_count = 0;
            let mut else_entry = None;
            let mut any_entry = None;
            for (entry_index, item) in elements.iter().enumerate() {
                let labels = item.labels.as_ref().expect("labelled tuple entry");
                let item_ty = self.check_expr(&item.value)?;
                self.ensure_assignable(element, item_ty, item.value.span())?;
                self.check_constant_range(&item.value, element)?;
                for label in labels {
                    match label {
                        CaseLabel::Value(value) => {
                            self.tuple_label_index(
                                *value,
                                array,
                                *span,
                                &mut explicit,
                                entry_index,
                                &mut entry_indices,
                            )?;
                            explicit_count += 1;
                        }
                        CaseLabel::Range { low, high } => {
                            for value in *low..=*high {
                                self.tuple_label_index(
                                    value,
                                    array,
                                    *span,
                                    &mut explicit,
                                    entry_index,
                                    &mut entry_indices,
                                )?;
                                explicit_count += 1;
                            }
                        }
                        CaseLabel::Else => {
                            if else_entry.replace(entry_index).is_some() {
                                return Err(Error::coded(
                                    "E0402",
                                    *span,
                                    "duplicate ELSE label in array tuple",
                                ));
                            }
                        }
                        CaseLabel::Any => {
                            if any_entry.replace(entry_index).is_some() {
                                return Err(Error::coded(
                                    "E0402",
                                    *span,
                                    "duplicate * label in array tuple",
                                ));
                            }
                        }
                    }
                }
            }
            if let Some(any_entry) = any_entry {
                if explicit_count > 0 || else_entry.is_some() || elements.len() != 1 {
                    return Err(Error::coded(
                        "E0400",
                        *span,
                        "the * array-tuple label must be the only label",
                    ));
                }
                entry_indices[any_entry] = (array.low..=array.high).collect();
            } else if let Some(else_entry) = else_entry {
                let missing: Vec<i128> = (array.low..=array.high)
                    .filter(|index| !explicit[(index - array.low) as usize])
                    .collect();
                entry_indices[else_entry] = missing;
            }
            if explicit_count == 0 && any_entry.is_none() && else_entry.is_none() {
                return Err(Error::coded("E0400", *span, "array tuple has no labels"));
            }
            let missing: Vec<i128> = (array.low..=array.high)
                .filter(|index| !explicit[(index - array.low) as usize])
                .collect();
            if any_entry.is_none() && else_entry.is_none() && !missing.is_empty() {
                return Err(Error::coded(
                    "E0400",
                    *span,
                    format!("array tuple does not cover index `{}`", missing[0]),
                ));
            }
            if let Some(else_entry) = else_entry {
                entry_indices[else_entry] = (array.low..=array.high)
                    .filter(|index| !explicit[(index - array.low) as usize])
                    .collect();
            }
            self.labelled_tuples.insert(*span, entry_indices);
        }
        let ty = Type::Array(array);
        self.types.insert(*span, ty);
        Ok(ty)
    }

    fn ensure_assignable(&self, target: Type, value: Type, span: Span) -> Result<(), Error> {
        if matches!(target, Type::Moreta(_)) {
            return Err(Error::coded(
                "E0400",
                span,
                "module mode objects are not assignable",
            ));
        }
        let ok = match target {
            Type::Int(target_kind) => match value {
                Type::Int(value_kind) => value_kind == target_kind,
                Type::IntLiteral => true,
                _ => false,
            },
            Type::Bool => value == Type::Bool,
            Type::Float => value == Type::Float,
            Type::Char => value == Type::Char,
            Type::String => is_narrow_string_type(value),
            Type::BoundedString { .. } => is_narrow_string_type(value),
            Type::WChar => value == Type::WChar,
            Type::WString => is_wide_string_type(value),
            Type::WBoundedString { .. } => is_wide_string_type(value),
            Type::Bits => is_bits_type(value),
            Type::BoundedBits { .. } => is_bits_type(value),
            Type::Struct(id) => value == Type::Struct(id),
            Type::Set(id) => value == Type::Set(id),
            Type::Moreta(id) => value == Type::Moreta(id),
            Type::Association => value == Type::Association,
            Type::Access(access) => value == Type::Access(access),
            Type::PowerSet(set) => value == Type::PowerSet(set),
            Type::Array(array) => match value {
                Type::Array(value_array) => value_array == array,
                Type::DynamicArray { low, element } => low == array.low && element == array.element,
                _ => false,
            },
            Type::MultiArray(array) => value == Type::MultiArray(array),
            Type::DynamicArray { low, element } => match value {
                Type::Array(array) => array.low == low && array.element == element,
                Type::DynamicArray {
                    low: value_low,
                    element: value_element,
                } => value_low == low && value_element == element,
                _ => false,
            },
            Type::IntLiteral | Type::ArrayTuple => false,
        };

        if ok {
            Ok(())
        } else {
            Err(Error::coded(
                "E0404",
                span,
                format!(
                    "cannot assign {} to {}",
                    value.describe(),
                    target.describe()
                ),
            ))
        }
    }

    fn check_case_label_value(
        &self,
        value: i128,
        selector_ty: Type,
        span: Span,
    ) -> Result<(), Error> {
        let fits = match selector_ty {
            Type::Int(kind) => types::contains(kind, value, self.profile),
            Type::IntLiteral => types::contains(IntKind::Long, value, self.profile),
            _ => false,
        };

        if fits {
            Ok(())
        } else {
            Err(Error::coded(
                "E0405",
                span,
                format!("CASE label `{value}` is outside the range of the selector"),
            ))
        }
    }

    fn check_constant_range(&self, expr: &Expr, target: Type) -> Result<(), Error> {
        let Type::Int(kind) = target else {
            return Ok(());
        };
        let Some(value) = constant_int(expr) else {
            return Ok(());
        };
        if !types::contains(kind, value, self.profile) {
            return Err(Error::coded(
                "E0405",
                expr.span(),
                format!("value `{value}` is outside the range of {}", kind.name()),
            ));
        }
        Ok(())
    }
}

fn field_root(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name { name, .. } => Some(name),
        Expr::Field { base, .. } => field_root(base),
        _ => None,
    }
}

fn reject_nested_procs(declarations: &[Declaration]) -> Result<(), Error> {
    for declaration in declarations {
        if let Declaration::Proc(proc_def) = declaration {
            return Err(Error::coded(
                "E0409",
                proc_def.span,
                "procedure definitions inside procedures are not implemented yet",
            ));
        }
    }
    Ok(())
}

fn binary_for_assign(op: AssignOp) -> BinaryOp {
    match op {
        AssignOp::Assign => BinaryOp::Eq,
        AssignOp::Add => BinaryOp::Add,
        AssignOp::Sub => BinaryOp::Sub,
        AssignOp::Mul => BinaryOp::Mul,
        AssignOp::Div => BinaryOp::Div,
        AssignOp::Concat => BinaryOp::Concat,
    }
}

fn powerset_member_type(member: crate::types::PowerSetMember) -> Type {
    match member {
        crate::types::PowerSetMember::Int(kind) => Type::Int(kind),
        crate::types::PowerSetMember::Bool => Type::Bool,
        crate::types::PowerSetMember::Char => Type::Char,
        crate::types::PowerSetMember::WChar => Type::WChar,
    }
}

fn member_compatible(member: crate::types::PowerSetMember, ty: Type) -> bool {
    match member {
        crate::types::PowerSetMember::Int(_) => ty.is_int(),
        crate::types::PowerSetMember::Bool => ty == Type::Bool,
        crate::types::PowerSetMember::Char => ty == Type::Char,
        crate::types::PowerSetMember::WChar => ty == Type::WChar,
    }
}

fn is_narrow_string_type(ty: Type) -> bool {
    matches!(ty, Type::String | Type::BoundedString { .. })
}

fn is_wide_string_type(ty: Type) -> bool {
    matches!(ty, Type::WString | Type::WBoundedString { .. })
}

fn is_string_type(ty: Type) -> bool {
    is_narrow_string_type(ty) || is_wide_string_type(ty)
}

fn is_bits_type(ty: Type) -> bool {
    matches!(ty, Type::Bits | Type::BoundedBits { .. })
}

fn bits_compatible(left: Type, right: Type) -> bool {
    is_bits_type(left) && is_bits_type(right)
}

fn strings_compatible(left: Type, right: Type) -> bool {
    (is_narrow_string_type(left) && is_narrow_string_type(right))
        || (is_wide_string_type(left) && is_wide_string_type(right))
}

fn is_varying_string(ty: Type) -> bool {
    matches!(
        ty,
        Type::String
            | Type::WString
            | Type::BoundedString { varying: true, .. }
            | Type::WBoundedString { varying: true, .. }
    )
}

fn arrays_compatible(left: Type, right: Type) -> bool {
    match (left, right) {
        (Type::Array(left), Type::Array(right)) => left == right,
        (Type::Array(array), Type::DynamicArray { low, element })
        | (Type::DynamicArray { low, element }, Type::Array(array)) => {
            array.low == low && array.element == element
        }
        (Type::MultiArray(left), Type::MultiArray(right)) => left == right,
        (
            Type::DynamicArray {
                low: left_low,
                element: left_element,
            },
            Type::DynamicArray {
                low: right_low,
                element: right_element,
            },
        ) => left_low == right_low && left_element == right_element,
        _ => false,
    }
}

pub(crate) fn constant_int(expr: &Expr) -> Option<i128> {
    match expr {
        Expr::Integer { value, .. } => Some(*value),
        Expr::Unary {
            op: UnaryOp::Neg,
            operand,
            ..
        } => constant_int(operand).and_then(i128::checked_neg),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;
    use crate::manifest::WidthProfile;
    use crate::parser;

    fn check_source(source: &str) -> Result<(), Error> {
        let tokens = lexer::tokenize(source).unwrap();
        let program = parser::parse(&tokens).unwrap();
        check(&program, WidthProfile::default())
    }

    fn check_all_source(source: &str) -> Vec<Diagnostic> {
        let tokens = lexer::tokenize(source).unwrap();
        let program = parser::parse(&tokens).unwrap();
        check_all(&program, WidthProfile::default())
    }

    #[test]
    fn collects_multiple_action_errors() {
        let diagnostics = check_all_source(
            r#"
M: MODULE
  DCL a SHORT_INT;
  DCL b LONG_INT;
  a := b;
  a := b;
END M;
"#,
        );

        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics
            .iter()
            .all(|diagnostic| diagnostic.message.contains("cannot assign LONG_INT")));
    }

    #[test]
    fn hello_example_checks() {
        check_source(include_str!("../examples/hello/src/main.ch")).unwrap();
    }

    #[test]
    fn records_expression_types_and_apply_resolutions() {
        let tokens = lexer::tokenize(include_str!("../examples/hello/src/main.ch")).unwrap();
        let program = parser::parse(&tokens).unwrap();
        let result = analyze(&program, WidthProfile::default());

        assert!(result.diagnostics.is_empty());
        assert!(!result.types.is_empty());
        assert!(result
            .resolutions
            .values()
            .any(|resolution| matches!(resolution, ApplyResolution::Conversion(_))));
        assert!(result
            .resolutions
            .values()
            .any(|resolution| matches!(resolution, ApplyResolution::Procedure(_))));
    }

    #[test]
    fn rejects_mode_mismatch() {
        let error = check_source(
            r#"
M: MODULE
  DCL a SHORT_INT;
  DCL b LONG_INT;
  a := b;
END M;
"#,
        )
        .unwrap_err();
        assert!(error
            .message
            .contains("cannot assign LONG_INT to SHORT_INT"));
    }

    #[test]
    fn rejects_assigning_bool_to_integer() {
        let error = check_source(
            r#"
M: MODULE
  DCL x INT;
  x := TRUE;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("cannot assign BOOL to INT"));
    }

    #[test]
    fn rejects_unknown_mode() {
        let error = check_source(
            r#"
M: MODULE
  DCL x NOPE;
END M;
"#,
        )
        .unwrap_err();
        assert_eq!(error.code, "E0401");
        assert!(error.message.contains("unknown mode `NOPE`"));
    }

    #[test]
    fn rejects_literal_out_of_range() {
        let error = check_source(
            r#"
M: MODULE
  DCL x SHORT_INT INIT := 100_000;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("outside the range of SHORT_INT"));
    }

    #[test]
    fn seizes_granted_variables_and_procedures() {
        check_source(
            r#"
A: MODULE
  GRANT x, F;
  DCL x INT INIT := 41;
  F: PROC () RETURNS (INT);
    RETURN (x + 1);
  END F;
END A;

B: MODULE
  SEIZE x, F;
  ASSERT x = 41;
  ASSERT F() = 42;
  x := x + 1;
  ASSERT x = 42;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn rejects_seize_of_ungranted_name() {
        let error = check_source(
            r#"
A: MODULE
  DCL secret INT;
END A;

B: MODULE
  SEIZE secret;
END B;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("no visible module grants it"));
    }

    #[test]
    fn allows_duplicate_simple_exports_across_modules() {
        check_source(
            r#"
A: MODULE
  GRANT x;
  DCL x INT;
END A;

B: MODULE
  GRANT x;
  DCL x INT;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn rejects_duplicate_grant_within_a_module() {
        let error = check_source(
            r#"
A: MODULE
  GRANT x;
  GRANT x;
  DCL x INT;
END A;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("granted more than once"));
    }

    #[test]
    fn seizes_a_qualified_export() {
        check_source(
            r#"
A: MODULE
  GRANT x;
  DCL x INT INIT := 1;
END A;

B: MODULE
  GRANT x;
  DCL x INT INIT := 2;
END B;

C: MODULE
  SEIZE A!x;
  ASSERT x = 1;
END C;
"#,
        )
        .unwrap();
    }

    #[test]
    fn forbid_blocks_field_selection_outside_owner() {
        let error = check_source(
            r#"
A: MODULE
  NEWMODE S = STRUCT(x INT, hidden INT);
  GRANT S FORBID (hidden);
  DCL local S;
  local.hidden := 1;
END A;

B: MODULE
  SEIZE S;
  DCL remote S;
  remote.x := 2;
  remote.hidden := 3;
END B;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("FORBIDden"));
    }

    #[test]
    fn forbid_all_blocks_every_field_outside_owner() {
        let error = check_source(
            r#"
A: MODULE
  NEWMODE S = STRUCT(x INT);
  GRANT S FORBID ALL;
END A;

B: MODULE
  SEIZE S;
  DCL remote S;
  remote.x := 1;
END B;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("FORBIDden"));
    }

    #[test]
    fn structure_fields_are_selectable_without_forbid() {
        check_source(
            r#"
A: MODULE
  NEWMODE S = STRUCT(x INT, y BOOL);
  GRANT S;
END A;

B: MODULE
  SEIZE S;
  DCL remote S;
  remote.x := 1;
  ASSERT remote.x = 1;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn forbid_field_must_exist() {
        let error = check_source(
            r#"
A: MODULE
  NEWMODE S = STRUCT(x INT);
  GRANT S FORBID (missing);
END A;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("not a field"));
    }

    #[test]
    fn nested_module_seizes_parent_names() {
        check_source(
            r#"
M: MODULE
  DCL x INT INIT := 1;
  Helper: MODULE
    SEIZE x;
    ASSERT x = 1;
  END Helper;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_module_cannot_see_parent_names_without_seize() {
        let error = check_source(
            r#"
M: MODULE
  DCL x INT INIT := 1;
  Helper: MODULE
    ASSERT x = 1;
  END Helper;
END M;
"#,
        )
        .unwrap_err();
        assert!(
            error.message.contains("unknown name `x`"),
            "{}",
            error.message
        );
    }

    #[test]
    fn nested_module_locals_do_not_escape() {
        let error = check_source(
            r#"
M: MODULE
  Helper: MODULE
    DCL y INT;
  END Helper;
  y := 1;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("unknown"), "{}", error.message);
    }

    #[test]
    fn nested_module_seize_all_imports_parent_reach() {
        check_source(
            r#"
M: MODULE
  DCL x INT INIT := 1;
  DCL y INT INIT := 2;
  Helper: MODULE
    SEIZE ALL;
    ASSERT x + y = 3;
  END Helper;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_context_checks_the_parent_reach() {
        check_source(
            r#"
M: MODULE
  DCL x INT INIT := 1;
  CONTEXT
    DCL x INT;
  FOR
  Helper: MODULE
    SEIZE x;
  END Helper;
END M;
"#,
        )
        .unwrap();

        let error = check_source(
            r#"
M: MODULE
  DCL x INT INIT := 1;
  CONTEXT
    DCL x BOOL;
  FOR
  Helper: MODULE
    SEIZE x;
  END Helper;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("has mode BOOL"), "{}", error.message);
    }

    #[test]
    fn modules_inside_a_begin_share_the_block_reach() {
        check_source(
            r#"
M: MODULE
  BEGIN
    A: MODULE
      GRANT x;
      DCL x INT INIT := 1;
    END A;
    B: MODULE
      SEIZE x;
      ASSERT x = 1;
    END B;
  END;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn modules_inside_an_if_branch_share_the_branch_reach() {
        check_source(
            r#"
M: MODULE
  DCL flag BOOL INIT := TRUE;
  IF flag THEN
    A: MODULE
      GRANT x;
      DCL x INT INIT := 1;
    END A;
    B: MODULE
      SEIZE x;
      ASSERT x = 1;
    END B;
  FI;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn block_grants_do_not_escape_the_block() {
        let error = check_source(
            r#"
M: MODULE
  SEIZE x;
  BEGIN
    A: MODULE
      GRANT x;
      DCL x INT INIT := 1;
    END A;
  END;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("SEIZE"), "{}", error.message);
    }

    #[test]
    fn nested_grant_prefixes_are_seizable() {
        check_source(
            r#"
M: MODULE
  A: MODULE
    GRANT x PREFIXED P;
    DCL x INT INIT := 1;
  END A;
  B: MODULE
    SEIZE x PREFIXED P;
    ASSERT x = 1;
  END B;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_grant_to_friends_is_enforced() {
        check_source(
            r#"
M: MODULE
  A: MODULE
    GRANT x TO B;
    DCL x INT INIT := 1;
  END A;
  B: MODULE
    SEIZE x;
    ASSERT x = 1;
  END B;
END M;
"#,
        )
        .unwrap();

        let error = check_source(
            r#"
M: MODULE
  A: MODULE
    GRANT x TO B;
    DCL x INT INIT := 1;
  END A;
  C: MODULE
    SEIZE x;
  END C;
END M;
"#,
        )
        .unwrap_err();
        assert!(
            error.message.contains("cannot SEIZE `x`"),
            "{}",
            error.message
        );
    }

    #[test]
    fn nested_grant_forbid_is_enforced() {
        let error = check_source(
            r#"
M: MODULE
  A: MODULE
    NEWMODE S = STRUCT(x INT, hidden BOOL);
    GRANT S FORBID (hidden);
  END A;
  B: MODULE
    SEIZE S;
    DCL s S;
    s.hidden := TRUE;
  END B;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("FORBIDden"), "{}", error.message);
    }

    #[test]
    fn nested_grants_are_visible_to_siblings() {
        check_source(
            r#"
M: MODULE
  A: MODULE
    GRANT x;
    DCL x INT INIT := 1;
  END A;

  B: MODULE
    SEIZE x;
    ASSERT x = 1;
  END B;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_grants_are_order_independent_for_checking() {
        check_source(
            r#"
M: MODULE
  B: MODULE
    SEIZE x;
    ASSERT x = 1;
  END B;

  A: MODULE
    GRANT x;
    DCL x INT INIT := 1;
  END A;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn parent_can_regrant_a_child_grant() {
        check_source(
            r#"
M: MODULE
  GRANT x;
  A: MODULE
    GRANT x;
    DCL x INT INIT := 1;
  END A;
END M;

N: MODULE
  SEIZE x;
  ASSERT x = 1;
END N;
"#,
        )
        .unwrap();
    }

    #[test]
    fn parent_regrant_of_a_grandchild_grant_works() {
        check_source(
            r#"
M: MODULE
  GRANT x;
  A: MODULE
    GRANT x;
    B: MODULE
      GRANT x;
      DCL x INT INIT := 1;
    END B;
  END A;
END M;

N: MODULE
  SEIZE x;
  ASSERT x = 1;
END N;
"#,
        )
        .unwrap();
    }

    #[test]
    fn parent_reach_can_use_a_child_grant() {
        check_source(
            r#"
M: MODULE
  A: MODULE
    GRANT x;
    DCL x INT INIT := 1;
  END A;
  ASSERT x = 1;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_parent_can_use_a_grandchild_grant() {
        check_source(
            r#"
M: MODULE
  A: MODULE
    B: MODULE
      GRANT x;
      DCL x INT INIT := 1;
    END B;
    ASSERT x = 1;
  END A;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn duplicate_sibling_grants_are_ambiguous_to_seize() {
        let error = check_source(
            r#"
M: MODULE
  A: MODULE
    GRANT x;
    DCL x INT INIT := 1;
  END A;

  B: MODULE
    GRANT x;
    DCL x INT INIT := 2;
  END B;

  C: MODULE
    SEIZE x;
  END C;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("ambiguous"), "{}", error.message);
    }

    #[test]
    fn nested_grants_do_not_escape_to_the_outer_reach() {
        let error = check_source(
            r#"
M: MODULE
  A: MODULE
    GRANT x;
    DCL x INT INIT := 1;
  END A;
END M;

Outside: MODULE
  SEIZE x;
END Outside;
"#,
        )
        .unwrap_err();
        assert!(
            error.message.contains("cannot SEIZE `x`"),
            "{}",
            error.message
        );
    }

    #[test]
    fn nested_context_can_describe_a_sibling_grant() {
        check_source(
            r#"
M: MODULE
  A: MODULE
    GRANT x;
    DCL x INT INIT := 1;
  END A;

  CONTEXT
    DCL x INT;
  FOR
  B: MODULE
    SEIZE x;
  END B;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn nested_module_cannot_grant_a_seized_name() {
        let error = check_source(
            r#"
M: MODULE
  DCL x INT INIT := 1;
  A: MODULE
    SEIZE x;
    GRANT x;
  END A;
END M;
"#,
        )
        .unwrap_err();
        assert!(
            error
                .message
                .contains("not a declaration or child grant in this reach"),
            "{}",
            error.message
        );
    }

    #[test]
    fn nested_module_allows_local_definitions() {
        check_source(
            r#"
M: MODULE
  Helper: MODULE
    DCL x INT INIT := 1;
    P: PROC () RETURNS (INT);
      RETURN x;
    END P;
    ASSERT P() = 1;
  END Helper;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn later_contexts_may_seize() {
        check_source(
            r#"
A: MODULE
  GRANT x;
  DCL x INT INIT := 1;
END A;

CONTEXT
  DCL x INT;
FOR
CONTEXT
  SEIZE x;
FOR
M: MODULE
  SEIZE x;
  ASSERT x = 1;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn later_contexts_may_grant_their_own_declarations() {
        check_source(
            r#"
A: MODULE
  GRANT y;
  DCL y INT INIT := 1;
END A;

CONTEXT
FOR
CONTEXT
  DCL y INT;
  GRANT y;
FOR
M: MODULE
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn later_context_seize_requires_an_earlier_declaration() {
        let error = check_source(
            r#"
CONTEXT
  DCL x INT;
FOR
CONTEXT
  SEIZE y;
FOR
M: MODULE
END M;
"#,
        )
        .unwrap_err();
        assert!(
            error.message.contains("no earlier declaration"),
            "{}",
            error.message
        );
    }

    #[test]
    fn context_declarations_match_the_enclosing_reach() {
        check_source(
            r#"
A: MODULE
  GRANT x, F;
  DCL x INT INIT := 1;
  F: PROC (a INT) RETURNS (INT);
    RETURN a + x;
  END F;
END A;

CONTEXT
  DCL x INT;
  F: PROC (a INT) RETURNS (INT) END;
FOR
B: MODULE
  SEIZE x, F;
  ASSERT F(2) = 3;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn context_declaration_requires_a_grant() {
        let error = check_source(
            r#"
CONTEXT
  DCL missing INT;
FOR
B: MODULE
END B;
"#,
        )
        .unwrap_err();
        assert!(
            error.message.contains("no matching grant"),
            "{}",
            error.message
        );
    }

    #[test]
    fn context_declaration_mode_must_match() {
        let error = check_source(
            r#"
A: MODULE
  GRANT x;
  DCL x INT;
END A;

CONTEXT
  DCL x BOOL;
FOR
B: MODULE
  SEIZE x;
END B;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("has mode BOOL"), "{}", error.message);
    }

    #[test]
    fn context_declaration_is_subject_to_to_restrictions() {
        let error = check_source(
            r#"
A: MODULE
  GRANT x TO SomeoneElse;
  DCL x INT;
END A;

CONTEXT
  DCL x INT;
FOR
B: MODULE
  SEIZE x;
END B;
"#,
        )
        .unwrap_err();
        assert!(
            error.message.contains("no matching grant"),
            "{}",
            error.message
        );
    }

    #[test]
    fn text_io_requires_a_chars_location() {
        let error = check_source(
            r#"
M: MODULE
  DCL n INT;
  WRITETEXT(n, "x");
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("CHARS"), "{}", error.message);
    }

    #[test]
    fn unhandled_cause_must_be_listed_in_proc_exceptions() {
        let error = check_source(
            r#"
M: MODULE
  P: PROC ();
    CAUSE fail;
  END P;
END M;
"#,
        )
        .unwrap_err();
        assert_eq!(error.code, "E0410");
        assert!(error.message.contains("`fail`"), "{}", error.message);
        assert!(error.message.contains("EXCEPTIONS"), "{}", error.message);
    }

    #[test]
    fn proc_exception_list_allows_cause() {
        check_source(
            r#"
M: MODULE
  P: PROC () EXCEPTIONS (fail);
    CAUSE fail;
  END P;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn proc_handler_catches_cause_without_an_exception_list() {
        check_source(
            r#"
M: MODULE
  P: PROC ();
    CAUSE fail;
  END
  ON (fail):
    ;
  END P;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn called_procedure_exceptions_flow_to_the_caller() {
        let error = check_source(
            r#"
M: MODULE
  P: PROC () EXCEPTIONS (boom);
    CAUSE boom;
  END P;
  Q: PROC ();
    P();
  END Q;
END M;
"#,
        )
        .unwrap_err();
        assert_eq!(error.code, "E0410");
        assert!(error.message.contains("`boom`"), "{}", error.message);
        assert!(error.message.contains("`Q`"), "{}", error.message);

        check_source(
            r#"
M: MODULE
  P: PROC () EXCEPTIONS (boom);
    CAUSE boom;
  END P;
  Q: PROC () EXCEPTIONS (boom);
    P();
  END Q;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn assert_requires_assertfail_in_proc_exceptions() {
        let error = check_source(
            r#"
M: MODULE
  P: PROC ();
    ASSERT TRUE;
  END P;
END M;
"#,
        )
        .unwrap_err();
        assert_eq!(error.code, "E0410");
        assert!(error.message.contains("ASSERTFAIL"), "{}", error.message);

        check_source(
            r#"
M: MODULE
  P: PROC () EXCEPTIONS (ASSERTFAIL);
    ASSERT TRUE;
  END P;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn action_handler_catches_exception_inside_a_proc() {
        check_source(
            r#"
M: MODULE
  P: PROC ();
    BEGIN
      CAUSE fail;
    END
    ON (fail):
      ;
    END;
  END P;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn duplicate_exception_names_in_a_handler_are_rejected() {
        let error = check_source(
            r#"
M: MODULE
  CAUSE boom;
END
ON (boom, boom):
  ASSERT TRUE;
END;
"#,
        )
        .unwrap_err();
        assert!(
            error.message.contains("duplicate exception"),
            "{}",
            error.message
        );
    }

    #[test]
    fn array_index_requires_an_integer() {
        let error = check_source(
            r#"
M: MODULE
  DCL a ARRAY (1:2) INT;
  a(TRUE) := 1;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("array index"), "{}", error.message);
    }

    #[test]
    fn nested_array_elements_are_rejected() {
        let error = check_source(
            r#"
M: MODULE
  SYNMODE Row = ARRAY (1:2) INT;
  DCL a ARRAY (1:2) Row;
END M;
"#,
        )
        .unwrap_err();
        assert!(
            error.message.contains("array elements"),
            "{}",
            error.message
        );
    }

    #[test]
    fn arrays_of_structures_are_accepted() {
        check_source(
            r#"
M: MODULE
  NEWMODE S = STRUCT(x INT);
  DCL a ARRAY (1:2) S;
END M;
"#,
        )
        .unwrap();
    }

    #[test]
    fn structure_equality_requires_the_same_mode() {
        check_source(
            r#"
M: MODULE
  NEWMODE S = STRUCT(x INT, y BOOL);
  DCL a S, b S;
  ASSERT a = b;
  ASSERT a /= b;
END M;
"#,
        )
        .unwrap();

        let error = check_source(
            r#"
M: MODULE
  NEWMODE S = STRUCT(x INT);
  NEWMODE T = STRUCT(x INT);
  DCL a S, b T;
  ASSERT a = b;
END M;
"#,
        )
        .unwrap_err();
        assert!(
            error.message.contains("cannot compare"),
            "{}",
            error.message
        );
    }

    #[test]
    fn seizes_prefixed_export() {
        check_source(
            r#"
A: MODULE
  GRANT a PREFIXED P;
  DCL a INT INIT := 1;
END A;

B: MODULE
  SEIZE P!a;
  ASSERT a = 1;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn seize_prefixed_clause_imports_simple_name() {
        check_source(
            r#"
A: MODULE
  GRANT a PREFIXED P;
  DCL a INT INIT := 1;
END A;

B: MODULE
  SEIZE a PREFIXED P;
  ASSERT a = 1;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn seize_all_prefixed_imports_only_matching_names() {
        let error = check_source(
            r#"
A: MODULE
  GRANT a PREFIXED P;
  GRANT b;
  DCL a INT INIT := 1;
  DCL b INT INIT := 2;
END A;

B: MODULE
  SEIZE ALL PREFIXED P;
  ASSERT a = 1;
  ASSERT b = 2;
END B;
"#,
        )
        .unwrap_err();
        assert!(
            error.message.contains("unknown name `b`"),
            "{}",
            error.message
        );
    }

    #[test]
    fn seize_all_ignores_prefixed_names() {
        let error = check_source(
            r#"
A: MODULE
  GRANT a PREFIXED P;
  DCL a INT INIT := 1;
END A;

B: MODULE
  SEIZE ALL;
  ASSERT a = 1;
END B;
"#,
        )
        .unwrap_err();
        assert!(
            error.message.contains("unknown name `a`"),
            "{}",
            error.message
        );
    }

    #[test]
    fn seize_rejects_new_prefix() {
        let error = check_source(
            r#"
A: MODULE
  GRANT a;
  DCL a INT;
END A;

B: MODULE
  SEIZE ( -> Q ) ! a;
END B;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("non-empty new prefix"));
    }

    #[test]
    fn grant_non_empty_old_prefix_requires_a_source_name() {
        let error = check_source(
            r#"
A: MODULE
  GRANT ( P -> Q ) ! a;
  DCL a INT;
END A;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("prefixed names are not declared"));
    }

    #[test]
    fn prefixed_export_is_not_visible_by_simple_name() {
        let error = check_source(
            r#"
A: MODULE
  GRANT a PREFIXED P;
  DCL a INT INIT := 1;
END A;

B: MODULE
  SEIZE a;
END B;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("no visible module grants it"));
    }

    #[test]
    fn grant_all_with_prefix() {
        check_source(
            r#"
A: MODULE
  GRANT ALL PREFIXED P;
  DCL a INT INIT := 1;
END A;

B: MODULE
  SEIZE P!a;
  ASSERT a = 1;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn rejects_ambiguous_simple_seize() {
        let error = check_source(
            r#"
A: MODULE
  GRANT x;
  DCL x INT;
END A;

B: MODULE
  GRANT x;
  DCL x INT;
END B;

C: MODULE
  SEIZE x;
END C;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("ambiguous"));
    }

    #[test]
    fn grant_all_and_seize_all() {
        check_source(
            r#"
A: MODULE
  GRANT ALL;
  DCL a INT INIT := 1;
  DCL b INT INIT := 2;
END A;

B: MODULE
  SEIZE ALL;
  ASSERT a + b = 3;
END B;
"#,
        )
        .unwrap();
    }

    #[test]
    fn grant_to_restricts_seize() {
        let error = check_source(
            r#"
A: MODULE
  GRANT x TO Client;
  DCL x INT;
END A;

Client: MODULE
  SEIZE x;
END Client;

Other: MODULE
  SEIZE x;
END Other;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("no visible module grants it"));
    }

    #[test]
    fn rejects_grant_of_undeclared_name() {
        let error = check_source(
            r#"
A: MODULE
  GRANT missing;
END A;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("not declared in this module"));
    }

    #[test]
    fn rejects_seize_from_same_module() {
        let error = check_source(
            r#"
A: MODULE
  GRANT x;
  SEIZE x;
  DCL x INT;
END A;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("from the same module"));
    }

    #[test]
    fn private_names_stay_private() {
        let error = check_source(
            r#"
A: MODULE
  DCL secret INT;
END A;

B: MODULE
  ASSERT secret = 1;
END B;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("unknown name `secret`"));
    }

    #[test]
    fn spec_and_body_interfaces_match() {
        check_source(
            r#"
Lib: SPEC MODULE
  GRANT x, F;
  DCL x INT;
  F: PROC () RETURNS (INT) END;
END Lib;

Lib: MODULE
  DCL x INT INIT := 41;
  F: PROC () RETURNS (INT);
    RETURN (x + 1);
  END F;
END Lib;

Client: MODULE
  SEIZE x, F;
  ASSERT x = 41;
  ASSERT F() = 42;
END Client;
"#,
        )
        .unwrap();
    }

    #[test]
    fn spec_requires_a_body() {
        let error = check_source(
            r#"
Lib: SPEC MODULE
  GRANT F;
  F: PROC () END;
END Lib;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("has no body"));
    }

    #[test]
    fn spec_signature_must_match_body() {
        let error = check_source(
            r#"
Lib: SPEC MODULE
  F: PROC (a INT) RETURNS (INT) END;
END Lib;

Lib: MODULE
  F: PROC (a INT) RETURNS (BOOL);
    RETURN (TRUE);
  END F;
END Lib;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("different signature"));
    }

    #[test]
    fn body_with_spec_cannot_grant() {
        let error = check_source(
            r#"
Lib: SPEC MODULE
  F: PROC () END;
END Lib;

Lib: MODULE
  GRANT F;
  F: PROC ();
  END F;
END Lib;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("GRANT must be in the spec module"));
    }

    #[test]
    fn spec_module_rejects_initializers() {
        let error = check_source(
            r#"
Lib: SPEC MODULE
  DCL x INT INIT := 1;
  F: PROC () END;
END Lib;

Lib: MODULE
  DCL x INT;
  F: PROC ();
  END F;
END Lib;
"#,
        )
        .unwrap_err();
        assert!(error
            .message
            .contains("initialization is not allowed in a spec module"));
    }

    #[test]
    fn do_for_step_must_be_positive() {
        let error = check_source(
            r#"
M: MODULE
  DO FOR i := 1 BY 0 TO 3;
  OD;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("DO FOR step must be positive"));
    }

    #[test]
    fn rejects_unknown_exit_label() {
        let error = check_source(
            r#"
M: MODULE
  EXIT Nowhere;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("unknown EXIT label `Nowhere`"));
    }

    #[test]
    fn case_selector_must_be_integer() {
        let error = check_source(
            r#"
M: MODULE
  CASE TRUE OF
    (1): ;
  ESAC;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("CASE selector must be an integer"));
    }

    #[test]
    fn case_label_must_fit_selector() {
        let error = check_source(
            r#"
M: MODULE
  DCL x SHORT_INT;
  CASE x OF
    (100_000): ;
  ESAC;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("outside the range of the selector"));
    }

    #[test]
    fn loop_counter_is_not_assignable() {
        let error = check_source(
            r#"
M: MODULE
  DO FOR i := 1 TO 3;
    i := 99;
  OD;
END M;
"#,
        )
        .unwrap_err();
        assert!(error.message.contains("loop counter and is not a location"));
    }
}
