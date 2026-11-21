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
        let message = message.into();
        Self {
            code: classify(&message),
            span,
            message,
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

fn classify(message: &str) -> &'static str {
    if message.contains("not implemented yet") || message.contains("not supported yet") {
        "E0409"
    } else if message.contains("unknown mode") {
        "E0401"
    } else if message.contains("duplicate") {
        "E0402"
    } else if message.contains("unknown variable") || message.contains("unknown name") {
        "E0403"
    } else if message.contains("cannot assign")
        || message.contains("mixing")
        || message.contains("mismatch")
        || message.contains("cannot compare")
        || message.contains("cannot order")
        || message.contains("arithmetic requires")
        || message.contains("requires BOOL")
        || message.contains("requires an integer")
    {
        "E0404"
    } else if message.contains("outside the range") || message.contains("CASE label") {
        "E0405"
    } else if message.contains("GRANT") || message.contains("SEIZE") || message.contains("spec") {
        "E0406"
    } else if message.contains("CASE")
        || message.contains("DO FOR")
        || message.contains("DO WHILE")
        || message.contains("EXIT")
        || message.contains("loop counter")
    {
        "E0407"
    } else if message.contains("RETURN") || message.contains("RESULT") {
        "E0408"
    } else {
        "E0400"
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyResolution {
    Conversion(IntKind),
    Procedure,
}

/// The checker's result: diagnostics plus expression annotations that the
/// interpreter consumes instead of re-inferring modes.
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub diagnostics: Vec<Diagnostic>,
    pub types: HashMap<Span, Type>,
    pub resolutions: HashMap<Span, ApplyResolution>,
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
    let info = match program::analyze(program) {
        Ok(info) => info,
        Err(error) => {
            return CheckResult {
                diagnostics: vec![Diagnostic::new(error.code, Some(error.span), error.message)],
                types: HashMap::new(),
                resolutions: HashMap::new(),
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
        module_modes_by_span: info.module_modes_by_span,
        module_floor: 0,
        current_module: 0,
        current_package: 0,
        current_package_name: None,
        current_module_name: None,
        labels: Vec::new(),
        diagnostics: Vec::new(),
        types: HashMap::new(),
        resolutions: HashMap::new(),
    };
    for (index, module) in program.modules.iter().enumerate() {
        checker.current_module = index;
        checker.current_package = module.package;
        checker.current_package_name = module.package_name.clone();
        checker.current_module_name = module.name.clone();
        let result = if module.kind == ModuleKind::Spec {
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
    }
}

fn external_error(message: String) -> CheckResult {
    CheckResult {
        diagnostics: vec![Diagnostic::new("E0600", None, message)],
        types: HashMap::new(),
        resolutions: HashMap::new(),
    }
}

fn map_program_error(error: program::Error) -> Error {
    Error::coded(error.code, error.span, error.message)
}

#[derive(Clone)]
enum ReachExport {
    Var(Type),
    Proc(ProcType),
    Mode(Type),
}

#[derive(Default, Clone)]
struct Scope {
    vars: HashMap<String, Type>,
    procs: HashMap<String, ProcType>,
    modes: HashMap<String, Type>,
    /// Names granted into this reach by directly enclosed modules.
    exports: HashMap<String, Vec<ReachExport>>,
    readonly: HashSet<String>,
}

struct Checker {
    profile: WidthProfile,
    scopes: Vec<Scope>,
    results: Vec<Option<Type>>,
    exports: Vec<program::ExportEntry>,
    modes: crate::modes::ModeTable,
    module_modes: Vec<HashMap<String, Type>>,
    module_modes_by_span: HashMap<Span, HashMap<String, Type>>,
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
                        return Err(Error::new(
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
        self.preload_child_grants(&module.actions)?;

        let has_module_label = module.name.is_some();
        if let Some(name) = &module.name {
            self.labels.push(name.clone());
        }
        let result = self.check_actions(&module.actions);
        if has_module_label {
            self.labels.pop();
        }
        result
    }

    fn check_nested_module(&mut self, module: &Module) -> Result<(), Error> {
        for declaration in &module.declarations {
            match declaration {
                Declaration::ProcSignature(signature) => {
                    return Err(Error::new(
                        signature.span,
                        "procedure signatures in nested modules are not supported yet",
                    ));
                }
                _ => {}
            }
        }

        let parent = self.scopes.last().cloned().unwrap_or_default();
        self.check_nested_contexts(module, &parent)?;

        let previous_floor = self.module_floor;
        self.scopes.push(Scope::default());
        self.module_floor = self.scopes.len() - 1;

        let result = (|| {
            self.preload_module_modes(module)?;
            self.apply_nested_seizes(&module.declarations, &parent)?;
            self.collect_declarations(&module.declarations)?;
            self.check_initializers(&module.declarations)?;
            self.check_proc_bodies(&module.declarations)?;
            self.preload_child_grants(&module.actions)?;
            self.check_nested_grants(module)?;

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

        self.scopes.pop();
        self.module_floor = previous_floor;
        result
    }

    fn check_nested_grants(&mut self, module: &Module) -> Result<(), Error> {
        let mut grantable: HashMap<String, ReachExport> = HashMap::new();
        for declaration in &module.declarations {
            match declaration {
                Declaration::Var(list) => {
                    for var in list {
                        let ty = self.resolve_mode(&var.mode)?;
                        grantable.insert(var.name.clone(), ReachExport::Var(ty));
                    }
                }
                Declaration::Proc(proc_def) => {
                    let signature =
                        self.signature_from_params(&proc_def.params, &proc_def.result)?;
                    grantable.insert(proc_def.name.clone(), ReachExport::Proc(signature));
                }
                Declaration::Mode(mode_def) => {
                    let ty = self.resolve_mode(&ModeRef {
                        name: mode_def.name.clone(),
                        span: mode_def.span,
                    })?;
                    grantable.insert(mode_def.name.clone(), ReachExport::Mode(ty));
                }
                _ => {}
            }
        }
        let scope = self.scopes.last().expect("scope stack is never empty");
        for (name, exports) in &scope.exports {
            if let [export] = exports.as_slice() {
                grantable.insert(name.clone(), export.clone());
            }
        }

        let mut granted = HashSet::new();
        for declaration in &module.declarations {
            let Declaration::Visibility(visibility) = declaration else {
                continue;
            };
            if visibility.kind != VisibilityKind::Grant {
                continue;
            }
            if visibility.prefix.is_some() {
                return Err(Error::new(
                    visibility.span,
                    "prefix clauses in a nested GRANT are not supported yet",
                ));
            }
            if !visibility.friends.is_empty() {
                return Err(Error::new(
                    visibility.span,
                    "TO restrictions in a nested GRANT are not supported yet",
                ));
            }
            if visibility.forbid.is_some() {
                return Err(Error::new(
                    visibility.span,
                    "FORBID in a nested GRANT are not supported yet",
                ));
            }

            let names: Vec<(String, Span)> = if visibility.all {
                grantable
                    .keys()
                    .map(|name| (name.clone(), visibility.span))
                    .collect()
            } else {
                visibility
                    .names
                    .iter()
                    .map(|name| (name.name.clone(), name.span))
                    .collect()
            };

            for (name, span) in names {
                if !granted.insert(name.clone()) {
                    return Err(Error::new(
                        span,
                        format!("`{name}` is granted more than once"),
                    ));
                }
                if !grantable.contains_key(&name) {
                    return Err(Error::new(
                        span,
                        format!(
                            "cannot GRANT `{name}`: not a declaration or child grant in this reach"
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Collect the names a module grants into its parent reach, recursively
    /// accounting for grants from directly enclosed modules.
    fn collect_module_grants(
        &mut self,
        module: &Module,
        inherited_modes: &HashMap<String, Type>,
    ) -> Result<HashMap<String, ReachExport>, Error> {
        let mut local_modes = inherited_modes.clone();
        if let Some(modes) = self.module_modes_by_span.get(&module.span) {
            local_modes.extend(modes.clone());
        }

        let mut grantable: HashMap<String, ReachExport> = HashMap::new();
        for declaration in &module.declarations {
            match declaration {
                Declaration::Var(list) => {
                    for var in list {
                        let ty = program::resolve_mode_with(&var.mode, &local_modes)
                            .map_err(map_program_error)?;
                        grantable.insert(var.name.clone(), ReachExport::Var(ty));
                    }
                }
                Declaration::Proc(proc_def) => {
                    let signature = program::signature_from_params_with(
                        &proc_def.params,
                        &proc_def.result,
                        &local_modes,
                    )
                    .map_err(map_program_error)?;
                    grantable.insert(proc_def.name.clone(), ReachExport::Proc(signature));
                }
                Declaration::Mode(mode_def) => {
                    if let Some(ty) = local_modes.get(&mode_def.name) {
                        grantable.insert(mode_def.name.clone(), ReachExport::Mode(*ty));
                    }
                }
                _ => {}
            }
        }

        for action in &module.actions {
            let Action::Module(child) = action else {
                continue;
            };
            for (name, export) in self.collect_module_grants(child, &local_modes)? {
                grantable.entry(name).or_insert(export);
            }
        }

        let mut exports = HashMap::new();
        let mut granted = HashSet::new();
        for declaration in &module.declarations {
            let Declaration::Visibility(visibility) = declaration else {
                continue;
            };
            if visibility.kind != VisibilityKind::Grant {
                continue;
            }
            if visibility.prefix.is_some()
                || !visibility.friends.is_empty()
                || visibility.forbid.is_some()
            {
                continue;
            }
            let names: Vec<String> = if visibility.all {
                grantable.keys().cloned().collect()
            } else {
                visibility
                    .names
                    .iter()
                    .map(|name| name.name.clone())
                    .collect()
            };
            for name in names {
                if !granted.insert(name.clone()) {
                    return Err(Error::new(
                        visibility.span,
                        format!("`{name}` is granted more than once"),
                    ));
                }
                let Some(export) = grantable.get(&name).cloned() else {
                    return Err(Error::new(
                        visibility.span,
                        format!(
                            "cannot GRANT `{name}`: not a declaration or child grant in this reach"
                        ),
                    ));
                };
                exports.insert(name, export);
            }
        }
        Ok(exports)
    }

    fn preload_child_grants(&mut self, actions: &[Action]) -> Result<(), Error> {
        let mut base_modes = self.module_modes[self.current_module].clone();
        for scope in &self.scopes[self.module_floor..] {
            for (name, ty) in &scope.modes {
                base_modes.insert(name.clone(), *ty);
            }
        }
        for action in actions {
            let Action::Module(child) = action else {
                continue;
            };
            for (name, export) in self.collect_module_grants(child, &base_modes)? {
                let scope = self.scopes.last_mut().expect("scope stack is never empty");
                scope.exports.entry(name).or_default().push(export);
            }
        }
        Ok(())
    }

    fn single_reach_export<'e>(
        name: &str,
        exports: &'e [ReachExport],
        span: Span,
    ) -> Result<&'e ReachExport, Error> {
        match exports {
            [export] => Ok(export),
            _ => Err(Error::new(
                span,
                format!("SEIZE `{name}` is ambiguous; several sibling modules grant it"),
            )),
        }
    }

    fn import_reach_export(
        &mut self,
        name: &str,
        export: &ReachExport,
        span: Span,
    ) -> Result<(), Error> {
        match export {
            ReachExport::Var(ty) => self.declare_var(name, *ty, span),
            ReachExport::Proc(signature) => self.declare_proc(name, signature.clone(), span),
            ReachExport::Mode(ty) => self.declare_mode(name, *ty, span),
        }
    }

    fn check_nested_contexts(&self, module: &Module, parent: &Scope) -> Result<(), Error> {
        let mut names = HashSet::new();
        for context in &module.contexts {
            for declaration in &context.declarations {
                match declaration {
                    Declaration::Var(list) => {
                        for var in list {
                            if !names.insert(var.name.clone()) {
                                return Err(Error::new(
                                    var.span,
                                    format!("duplicate context declaration `{}`", var.name),
                                ));
                            }
                            let declared = self.resolve_mode(&var.mode)?;
                            let parent_ty = parent.vars.get(&var.name).copied().or_else(|| {
                                parent.exports.get(&var.name).and_then(|exports| {
                                    match exports.as_slice() {
                                        [ReachExport::Var(ty)] => Some(*ty),
                                        _ => None,
                                    }
                                })
                            });
                            match parent_ty {
                                Some(actual) if actual == declared => {}
                                Some(actual) => {
                                    return Err(Error::new(
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
                                    return Err(Error::new(
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
                        if !names.insert(signature.name.clone()) {
                            return Err(Error::new(
                                signature.span,
                                format!("duplicate context declaration `{}`", signature.name),
                            ));
                        }
                        let declared =
                            self.signature_from_params(&signature.params, &signature.result)?;
                        let parent_sig = parent.procs.get(&signature.name).cloned().or_else(|| {
                            parent.exports.get(&signature.name).and_then(|exports| {
                                match exports.as_slice() {
                                    [ReachExport::Proc(signature)] => Some(signature.clone()),
                                    _ => None,
                                }
                            })
                        });
                        match parent_sig {
                            Some(actual) if actual == declared => {}
                            Some(_) => {
                                return Err(Error::new(
                                    signature.span,
                                    format!(
                                        "context procedure `{}` does not match the enclosing reach",
                                        signature.name
                                    ),
                                ));
                            }
                            None => {
                                return Err(Error::new(
                                    signature.span,
                                    format!(
                                        "context procedure `{}` has no matching name in the enclosing reach",
                                        signature.name
                                    ),
                                ));
                            }
                        }
                    }
                    Declaration::Mode(_) | Declaration::Visibility(_) | Declaration::Proc(_) => {
                        return Err(Error::new(
                            context.span,
                            "unsupported declaration in a nested context",
                        ));
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
        for declaration in declarations {
            let Declaration::Visibility(visibility) = declaration else {
                continue;
            };
            if visibility.kind != VisibilityKind::Seize {
                continue;
            }
            if visibility.prefix.is_some() {
                return Err(Error::new(
                    visibility.span,
                    "prefix clauses in a nested SEIZE are not supported yet",
                ));
            }

            if visibility.all {
                for (name, ty) in &parent.vars {
                    self.declare_var(name, *ty, visibility.span)?;
                }
                for (name, signature) in &parent.procs {
                    self.declare_proc(name, signature.clone(), visibility.span)?;
                }
                for (name, ty) in &parent.modes {
                    self.declare_mode(name, *ty, visibility.span)?;
                }
                for (name, exports) in &parent.exports {
                    let export = Self::single_reach_export(name, exports, visibility.span)?;
                    self.import_reach_export(name, export, visibility.span)?;
                }
                continue;
            }

            for name in &visibility.names {
                if !name.prefix.is_empty() {
                    return Err(Error::new(
                        name.span,
                        "qualified names in a nested SEIZE are not supported yet",
                    ));
                }
                if let Some(ty) = parent.vars.get(&name.name) {
                    self.declare_var(&name.name, *ty, name.span)?;
                } else if let Some(signature) = parent.procs.get(&name.name) {
                    self.declare_proc(&name.name, signature.clone(), name.span)?;
                } else if let Some(ty) = parent.modes.get(&name.name) {
                    self.declare_mode(&name.name, *ty, name.span)?;
                } else if let Some(exports) = parent.exports.get(&name.name) {
                    let export = Self::single_reach_export(&name.name, exports, name.span)?;
                    self.import_reach_export(&name.name, export, name.span)?;
                } else {
                    return Err(Error::new(
                        name.span,
                        format!(
                            "cannot SEIZE `{}`: no such name in the enclosing reach",
                            name.qualified()
                        ),
                    ));
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
            return Err(Error::new(
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
                    .map_err(|message| Error::new(name.span, message))?;
                let export = program::resolve_export(
                    &self.exports,
                    &source,
                    self.current_package_name.as_deref(),
                    self.current_module_name.as_deref(),
                )
                .map_err(|message| Error::new(name.span, message))?
                .clone();

                if export.module == self.current_module {
                    return Err(Error::new(
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
            .map_err(|message| Error::new(span, message))?;
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
                return Err(Error::new(
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
                    let signature =
                        self.signature_from_params(&proc_def.params, &proc_def.result)?;
                    self.declare_proc(&proc_def.name, signature, proc_def.span)?;
                }
                Declaration::ProcSignature(signature) => {
                    let proc_sig =
                        self.signature_from_params(&signature.params, &signature.result)?;
                    self.declare_proc(&signature.name, proc_sig, signature.span)?;
                }
                Declaration::Mode(_) => {}
                Declaration::Visibility(_) => {}
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
                    let value_ty = self.check_expr(&init.value)?;
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
        for param in &proc_def.params {
            if !seen.insert(param.name.as_str()) {
                return Err(Error::new(
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

        let result_type = match &proc_def.result {
            Some(mode) => Some(self.resolve_mode(mode)?),
            None => None,
        };

        self.results.push(result_type);
        let result = self.check_actions(&proc_def.actions);
        self.results.pop();
        result
    }

    fn declare_var(&mut self, name: &str, ty: Type, span: Span) -> Result<(), Error> {
        let scope = self.scopes.last_mut().expect("scope stack is never empty");
        if scope.vars.contains_key(name) || scope.procs.contains_key(name) {
            return Err(Error::new(
                span,
                format!("duplicate declaration of `{name}`"),
            ));
        }
        scope.vars.insert(name.to_owned(), ty);
        Ok(())
    }

    fn declare_proc(&mut self, name: &str, signature: ProcType, span: Span) -> Result<(), Error> {
        let scope = self.scopes.last_mut().expect("scope stack is never empty");
        if scope.vars.contains_key(name) || scope.procs.contains_key(name) {
            return Err(Error::new(
                span,
                format!("duplicate declaration of `{name}`"),
            ));
        }
        scope.procs.insert(name.to_owned(), signature);
        Ok(())
    }

    fn preload_module_modes(&mut self, module: &Module) -> Result<(), Error> {
        let modes = self
            .module_modes_by_span
            .get(&module.span)
            .cloned()
            .unwrap_or_else(|| self.module_modes[self.current_module].clone());
        let scope = self.scopes.last_mut().expect("scope stack is never empty");
        for (name, ty) in modes {
            if scope.modes.contains_key(&name) {
                return Err(Error::new(
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
    ) -> Result<ProcType, Error> {
        let mut param_types = Vec::with_capacity(params.len());
        let mut seen = HashSet::new();
        for param in params {
            if !seen.insert(param.name.clone()) {
                return Err(Error::new(
                    param.span,
                    format!("duplicate parameter `{}`", param.name),
                ));
            }
            param_types.push(self.resolve_mode(&param.mode)?);
        }
        let result = match result {
            Some(mode) => Some(self.resolve_mode(mode)?),
            None => None,
        };
        Ok(ProcType {
            params: param_types,
            result,
        })
    }

    fn resolve_mode(&self, mode: &ModeRef) -> Result<Type, Error> {
        for scope in self.scopes[self.module_floor..].iter().rev() {
            if let Some(ty) = scope.modes.get(&mode.name) {
                return Ok(*ty);
            }
            if let Some(exports) = scope.exports.get(&mode.name) {
                if let [ReachExport::Mode(ty)] = exports.as_slice() {
                    return Ok(*ty);
                }
            }
        }
        program::resolve_mode(mode).map_err(map_program_error)
    }

    fn lookup_var(&self, name: &str) -> Option<Type> {
        for scope in self.scopes[self.module_floor..].iter().rev() {
            if let Some(ty) = scope.vars.get(name) {
                return Some(*ty);
            }
            if let Some(exports) = scope.exports.get(name) {
                if let [ReachExport::Var(ty)] = exports.as_slice() {
                    return Some(*ty);
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
                if let [ReachExport::Proc(signature)] = exports.as_slice() {
                    return Some(signature.clone());
                }
            }
        }
        None
    }

    fn check_block(&mut self, actions: &[Action]) -> Result<(), Error> {
        self.scopes.push(Scope::default());
        let result = (|| {
            self.preload_child_grants(actions)?;
            self.check_actions(actions)
        })();
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
                let value_ty = self.check_expr(value)?;

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
                let ty = self.check_expr(condition)?;
                if ty != Type::Bool {
                    return Err(Error::new(
                        condition.span(),
                        format!("ASSERT requires BOOL, found {}", ty.describe()),
                    ));
                }
                Ok(())
            }
            Action::Call { callee, span } => {
                let Expr::Apply { name, args, .. } = callee else {
                    return Err(Error::new(*span, "expected a procedure call"));
                };
                self.check_apply(name, args, *span, false)?;
                Ok(())
            }
            Action::Return { value, span } => self.check_return(value.as_ref(), *span),
            Action::Result { value, span } => self.check_result(value, *span),
            Action::If {
                branches,
                else_actions,
                ..
            } => {
                for branch in branches {
                    let ty = self.check_expr(&branch.condition)?;
                    if ty != Type::Bool {
                        return Err(Error::new(
                            branch.condition.span(),
                            format!("IF condition must be BOOL, found {}", ty.describe()),
                        ));
                    }
                    self.check_block(&branch.body)?;
                }
                if let Some(actions) = else_actions {
                    self.check_block(actions)?;
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
                            return Err(Error::new(
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
                            return Err(Error::new(*span, "DO FOR bounds must be integers"));
                        }
                        let mut counter_ty = self.combine_int(start_ty, end_ty, *span)?;

                        if let Some(step_expr) = step {
                            let step_ty = self.check_expr(step_expr)?;
                            if !step_ty.is_int() {
                                return Err(Error::new(
                                    step_expr.span(),
                                    "DO FOR step must be an integer",
                                ));
                            }
                            counter_ty = self.combine_int(counter_ty, step_ty, step_expr.span())?;
                            if let Some(value) = constant_int(step_expr) {
                                if value <= 0 {
                                    return Err(Error::new(
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
                            self.preload_child_grants(body)?;
                            self.check_actions(body)
                        })();
                        self.scopes.pop();
                        return result;
                    }
                }
                self.check_block(body)
            }
            Action::Exit {
                label: Some(name),
                span,
            } => {
                if !self.labels.iter().any(|label| label == name) {
                    return Err(Error::new(*span, format!("unknown EXIT label `{name}`")));
                }
                Ok(())
            }
            Action::Exit { label: None, .. } => Ok(()),
            Action::Case {
                selector,
                alternatives,
                else_actions,
                ..
            } => {
                let selector_ty = self.check_expr(selector)?;
                if !selector_ty.is_int() {
                    return Err(Error::new(
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
                    self.check_block(&alternative.body)?;
                }

                if let Some(actions) = else_actions {
                    self.check_block(actions)?;
                }
                Ok(())
            }
            Action::Labelled { label, action, .. } => {
                if self.labels.iter().any(|existing| existing == label) {
                    return Err(Error::new(
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
                ..
            } => {
                self.scopes.push(Scope::default());
                let result = (|| {
                    self.collect_declarations(declarations)?;
                    reject_nested_procs(declarations)?;
                    self.check_initializers(declarations)?;
                    self.check_proc_bodies(declarations)?;
                    self.preload_child_grants(actions)?;
                    self.check_actions(actions)
                })();
                self.scopes.pop();
                result
            }
            Action::Empty { .. } => Ok(()),
            Action::Module(module) => self.check_nested_module(module),
        }
    }

    fn check_return(&mut self, value: Option<&Expr>, span: Span) -> Result<(), Error> {
        match self.results.last().copied() {
            None => Err(Error::new(span, "RETURN outside a procedure")),
            Some(None) => {
                if value.is_some() {
                    Err(Error::new(
                        span,
                        "RETURN with a value in a procedure that has no result",
                    ))
                } else {
                    Ok(())
                }
            }
            Some(Some(result_ty)) => {
                if let Some(value) = value {
                    let value_ty = self.check_expr(value)?;
                    self.ensure_assignable(result_ty, value_ty, value.span())?;
                }
                Ok(())
            }
        }
    }

    fn check_result(&mut self, value: &Expr, span: Span) -> Result<(), Error> {
        match self.results.last().copied() {
            Some(Some(result_ty)) => {
                let value_ty = self.check_expr(value)?;
                self.ensure_assignable(result_ty, value_ty, span)
            }
            _ => Err(Error::new(span, "RESULT outside a value procedure")),
        }
    }

    fn location_type(&mut self, target: &Expr) -> Result<Type, Error> {
        match target {
            Expr::Field { base, field, span } => {
                let Some(root) = field_root(target) else {
                    return Err(Error::new(
                        *span,
                        "assignment target must be a variable or a field of a variable",
                    ));
                };
                if self.is_readonly(root) {
                    return Err(Error::new(
                        *span,
                        format!("`{root}` is a loop counter and is not a location"),
                    ));
                }
                let ty = self.check_field(base, field, *span)?;
                Ok(ty)
            }
            Expr::Name { name, span } => {
                if self.is_readonly(name) {
                    return Err(Error::new(
                        *span,
                        format!("`{name}` is a loop counter and is not a location"),
                    ));
                }
                self.lookup_var(name)
                    .ok_or_else(|| Error::new(*span, format!("unknown variable `{name}`")))
            }
            _ => Err(Error::new(
                target.span(),
                "assignment target must be a name or field",
            )),
        }
    }

    fn check_field(&mut self, base: &Expr, field: &str, span: Span) -> Result<Type, Error> {
        let base_ty = self.check_expr(base)?;
        let Type::Struct(id) = base_ty else {
            return Err(Error::new(
                span,
                format!(
                    "field selection requires a structure, found {}",
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
            return Err(Error::new(
                span,
                format!("unknown field `{field}` in structure mode `{}`", mode.name),
            ));
        };

        if mode.module != self.current_module && mode.forbidden.contains(field) {
            return Err(Error::new(
                span,
                format!("field `{field}` is FORBIDden outside its defining module"),
            ));
        }

        Ok(info.ty)
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
            Expr::Integer { value, span, .. } => {
                let long = IntKind::Long;
                let Some((low, high)) = types::int_bounds(long, self.profile) else {
                    return Err(Error::new(
                        *span,
                        "integer widths above 127 are not supported",
                    ));
                };
                if !(*value >= low && *value <= high) {
                    return Err(Error::new(
                        *span,
                        format!("integer literal `{value}` is outside the range of &INT"),
                    ));
                }
                Ok(Type::IntLiteral)
            }
            Expr::Bool { .. } => Ok(Type::Bool),
            Expr::Char { .. } => Ok(Type::Char),
            Expr::String { .. } => Ok(Type::String),
            Expr::Name { name, span } => {
                if let Some(ty) = self.lookup_var(name) {
                    Ok(ty)
                } else if self.lookup_proc(name).is_some() {
                    Err(Error::new(
                        *span,
                        format!("procedure `{name}` must be called with `()`"),
                    ))
                } else {
                    Err(Error::new(*span, format!("unknown name `{name}`")))
                }
            }
            Expr::Apply { name, args, span } => match self.check_apply(name, args, *span, true)? {
                Some(ty) => Ok(ty),
                None => Err(Error::new(
                    *span,
                    format!("procedure `{name}` does not return a value"),
                )),
            },
            Expr::Unary { op, operand, span } => {
                let ty = self.check_expr(operand)?;
                match op {
                    UnaryOp::Neg => {
                        if ty.is_int() {
                            Ok(ty)
                        } else {
                            Err(Error::new(
                                *span,
                                format!("unary `-` requires an integer, found {}", ty.describe()),
                            ))
                        }
                    }
                    UnaryOp::Not => {
                        if ty == Type::Bool {
                            Ok(Type::Bool)
                        } else {
                            Err(Error::new(
                                *span,
                                format!("NOT requires BOOL, found {}", ty.describe()),
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
                let left_ty = self.check_expr(left)?;
                let right_ty = self.check_expr(right)?;
                self.check_binary_types(*op, left_ty, right_ty, *span)
            }
            Expr::Field { base, field, span } => self.check_field(base, field, *span),
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
                self.expect_bool_pair(left, right, span)?;
                Ok(Type::Bool)
            }
            BinaryOp::Eq | BinaryOp::Ne => {
                if left.is_int() && right.is_int() {
                    self.combine_int(left, right, span)?;
                    Ok(Type::Bool)
                } else if left == right
                    && matches!(
                        left,
                        Type::Bool | Type::Char | Type::String | Type::Struct(_)
                    )
                {
                    Ok(Type::Bool)
                } else {
                    Err(Error::new(
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
                } else if (left == Type::Char && right == Type::Char)
                    || (left == Type::String && right == Type::String)
                {
                    Ok(Type::Bool)
                } else {
                    Err(Error::new(
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
                } else {
                    Err(Error::new(
                        span,
                        format!(
                            "arithmetic requires integers, found {} and {}",
                            left.describe(),
                            right.describe()
                        ),
                    ))
                }
            }
            BinaryOp::Concat => {
                if left == Type::String && right == Type::String {
                    Ok(Type::String)
                } else {
                    Err(Error::new(
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
            Err(Error::new(
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
            (Type::Int(left_kind), Type::Int(right_kind)) => Err(Error::new(
                span,
                format!(
                    "integer mode mismatch: {} and {}; use an explicit conversion",
                    left_kind.name(),
                    right_kind.name()
                ),
            )),
            _ => Err(Error::new(
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
        if let Some(mode) = types::resolve_mode(name) {
            let Type::Int(kind) = mode else {
                return Err(Error::new(
                    span,
                    format!("conversion to {} is not supported yet", mode.describe()),
                ));
            };
            if args.len() != 1 {
                return Err(Error::new(
                    span,
                    format!("conversion to {} takes exactly one argument", kind.name()),
                ));
            }
            let arg_ty = self.check_expr(&args[0])?;
            if !arg_ty.is_int() {
                return Err(Error::new(
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
            return Err(Error::new(span, format!("unknown name `{name}`")));
        };
        self.resolutions.insert(span, ApplyResolution::Procedure);

        if signature.params.len() != args.len() {
            return Err(Error::new(
                span,
                format!(
                    "procedure `{name}` expects {} argument(s), found {}",
                    signature.params.len(),
                    args.len()
                ),
            ));
        }

        for (arg, param_ty) in args.iter().zip(&signature.params) {
            let arg_ty = self.check_expr(arg)?;
            self.ensure_assignable(*param_ty, arg_ty, arg.span())?;
        }

        if require_result && signature.result.is_none() {
            return Err(Error::new(
                span,
                format!("procedure `{name}` does not return a value"),
            ));
        }

        Ok(signature.result)
    }

    fn ensure_assignable(&self, target: Type, value: Type, span: Span) -> Result<(), Error> {
        let ok = match target {
            Type::Int(target_kind) => match value {
                Type::Int(value_kind) => value_kind == target_kind,
                Type::IntLiteral => true,
                _ => false,
            },
            Type::Bool => value == Type::Bool,
            Type::Char => value == Type::Char,
            Type::String => value == Type::String,
            Type::Struct(id) => value == Type::Struct(id),
            Type::IntLiteral => false,
        };

        if ok {
            Ok(())
        } else {
            Err(Error::new(
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
            Err(Error::new(
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
            return Err(Error::new(
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
            return Err(Error::new(
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

fn constant_int(expr: &Expr) -> Option<i128> {
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
            .any(|resolution| matches!(resolution, ApplyResolution::Procedure)));
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
