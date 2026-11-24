use crate::ast::Program;
use crate::diagnostics;
use crate::grant;
use crate::interp;
use crate::lexer;
use crate::manifest::{Build, Manifest, Package};
use crate::parser;
use crate::program;
use crate::sema;
use crate::source::SourceMap;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run<I>(args: I) -> i32
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();

    match args.next().as_deref() {
        None | Some("-h") | Some("--help") => {
            print_help();
            0
        }
        Some("-V") | Some("--version") => {
            println!("z200c {VERSION}");
            0
        }
        Some("check") => {
            let target = args.next().unwrap_or_else(|| ".".to_owned());
            check(Path::new(&target))
        }
        Some("run") => {
            let target = args.next().unwrap_or_else(|| ".".to_owned());
            run_package(Path::new(&target))
        }
        Some("compile") => {
            let target = args.next().unwrap_or_else(|| ".".to_owned());
            compile_package(Path::new(&target))
        }
        Some("grant") | Some("build") => {
            let target = args.next().unwrap_or_else(|| ".".to_owned());
            grant_package(Path::new(&target))
        }
        Some(command) => {
            eprintln!("error: unknown command `{command}`");
            eprintln!();
            print_help();
            2
        }
    }
}

fn check(target: &Path) -> i32 {
    match check_impl(target) {
        Ok(summary) => {
            println!("{summary}");
            0
        }
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
}

fn run_package(target: &Path) -> i32 {
    match run_impl(target) {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
}

fn compile_package(target: &Path) -> i32 {
    match compile_impl(target) {
        Ok(path) => {
            println!("wrote {}", path.display());
            0
        }
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
}

fn compile_impl(target: &Path) -> Result<PathBuf, String> {
    let package = load_package(target, false)?;
    let check = sema::analyze_with_externals(
        &package.program,
        crate::manifest::WidthProfile::default(),
        &package.external,
    );
    if !check.diagnostics.is_empty() {
        return Err(diagnostics::render(&package.source_map, &check.diagnostics));
    }
    let info = program::analyze(&package.program).map_err(|error| {
        package
            .source_map
            .format_diagnostic(error.code, error.span, &error.message)
    })?;
    let module_modes: Vec<_> = info
        .interfaces
        .iter()
        .map(|interface| interface.modes.clone())
        .collect();
    let c_source = crate::backend::generate(
        &package.program,
        &check,
        &package.manifest.package.name,
        crate::manifest::WidthProfile::default(),
        &info.modes,
        &module_modes,
    )?;
    let out_dir = package.package_dir.join("target").join("z200c");
    std::fs::create_dir_all(&out_dir).map_err(|error| {
        format!(
            "error[E0600]: cannot create output directory {}: {error}",
            out_dir.display()
        )
    })?;
    let c_path = out_dir.join(format!("{}.c", package.manifest.package.name));
    std::fs::write(&c_path, c_source)
        .map_err(|error| format!("error[E0600]: cannot write {}: {error}", c_path.display()))?;
    let binary = out_dir.join(format!(
        "{}{}",
        package.manifest.package.name,
        std::env::consts::EXE_SUFFIX
    ));
    let compiler = std::env::var("Z200C_CC")
        .or_else(|_| std::env::var("CC"))
        .unwrap_or_else(|_| {
            if cfg!(windows) {
                "gcc".to_owned()
            } else {
                "cc".to_owned()
            }
        });
    let output = std::process::Command::new(&compiler)
        .arg("-std=c11")
        .arg("-O2")
        .arg("-Wall")
        .arg("-Wextra")
        .arg("-o")
        .arg(&binary)
        .arg(&c_path)
        .output()
        .map_err(|error| format!("error[E0600]: cannot run C compiler `{compiler}`: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "error[E0601]: C compilation failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(binary)
}

fn grant_package(target: &Path) -> i32 {
    match grant_impl(target) {
        Ok(path) => {
            println!("wrote {}", path.display());
            0
        }
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
}

fn grant_impl(target: &Path) -> Result<PathBuf, String> {
    let package = load_package(target, false)?;

    let diagnostics = sema::check_all_with_externals(
        &package.program,
        crate::manifest::WidthProfile::default(),
        &package.external,
    );
    if !diagnostics.is_empty() {
        return Err(diagnostics::render(&package.source_map, &diagnostics));
    }

    let mut grant_file = grant::GrantFile::from_package(&package.manifest, &package.program)
        .map_err(|error| {
            package
                .source_map
                .format_diagnostic(error.code, error.span, &error.message)
        })?;
    grant_file.package_dir = Some(
        std::fs::canonicalize(&package.package_dir)
            .unwrap_or_else(|_| package.package_dir.clone())
            .display()
            .to_string(),
    );
    let text = grant_file
        .to_toml()
        .map_err(|error| format!("error[E0600]: {error}"))?;

    let out_dir = package.package_dir.join("target").join("z200c");
    std::fs::create_dir_all(&out_dir).map_err(|error| {
        format!(
            "error[E0600]: cannot create output directory {}: {error}",
            out_dir.display()
        )
    })?;

    let out_path = out_dir.join(format!("{}.grt.toml", package.manifest.package.name));
    std::fs::write(&out_path, text).map_err(|error| {
        format!(
            "error[E0600]: cannot write grant file {}: {error}",
            out_path.display()
        )
    })?;

    Ok(out_path)
}

fn check_impl(target: &Path) -> Result<String, String> {
    let package = load_package(target, false)?;

    let diagnostics = sema::check_all_with_externals(
        &package.program,
        crate::manifest::WidthProfile::default(),
        &package.external,
    );
    if !diagnostics.is_empty() {
        return Err(diagnostics::render(&package.source_map, &diagnostics));
    }

    Ok(format!(
        "checked {} (widths {})\n  {}: {} tokens, {} module(s)",
        package.manifest.package.name,
        crate::manifest::WidthProfile::default(),
        package.entry_path.display(),
        package.token_count,
        package.program.modules.len()
    ))
}

fn run_impl(target: &Path) -> Result<(), String> {
    let package = load_package(target, true)?;

    let check = sema::analyze_with_externals(
        &package.program,
        crate::manifest::WidthProfile::default(),
        &package.external,
    );
    if !check.diagnostics.is_empty() {
        return Err(diagnostics::render(&package.source_map, &check.diagnostics));
    }

    interp::run_checked(
        &package.program,
        crate::manifest::WidthProfile::default(),
        &check,
        &package.external,
    )
    .map_err(|error| {
        package
            .source_map
            .format_diagnostic(error.code, error.span, &error.message)
    })
}

struct LoadedPackage {
    manifest: Manifest,
    package_dir: PathBuf,
    entry_path: PathBuf,
    source_map: SourceMap,
    token_count: usize,
    program: Program,
    external: Vec<program::ExternalInterface>,
}

struct Loader {
    source_map: SourceMap,
    modules: Vec<crate::ast::Module>,
    package_dirs: Vec<std::path::PathBuf>,
    package_pieces: Vec<std::collections::HashMap<String, std::path::PathBuf>>,
    piece_visiting: HashSet<std::path::PathBuf>,
    token_count: usize,
    external: Vec<program::ExternalInterface>,
    visiting: HashSet<PathBuf>,
    loaded: HashSet<PathBuf>,
    next_package: usize,
    link_grant_sources: bool,
}

fn load_package(target: &Path, link_grant_sources: bool) -> Result<LoadedPackage, String> {
    if target.is_file() && target.extension() == Some(OsStr::new("ch")) {
        return load_script(target);
    }
    let manifest_path = resolve_manifest(target)?;
    let manifest =
        Manifest::load(&manifest_path).map_err(|error| format!("error[E0300]: {error}"))?;

    let package_dir = manifest_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let entry_path = package_dir.join(&manifest.build.entry);

    let mut loader = Loader {
        source_map: SourceMap::default(),
        modules: Vec::new(),
        package_dirs: Vec::new(),
        package_pieces: Vec::new(),
        piece_visiting: HashSet::new(),
        token_count: 0,
        external: Vec::new(),
        visiting: HashSet::new(),
        loaded: HashSet::new(),
        next_package: 0,
        link_grant_sources,
    };
    loader.load_package(&manifest_path)?;

    Ok(LoadedPackage {
        manifest,
        package_dir,
        entry_path,
        source_map: loader.source_map,
        token_count: loader.token_count,
        program: Program {
            modules: loader.modules,
            remotes: Vec::new(),
            generic_modules: Vec::new(),
            generic_module_instances: Vec::new(),
        },
        external: loader.external,
    })
}

fn load_script(target: &Path) -> Result<LoadedPackage, String> {
    let name = target
        .file_stem()
        .and_then(OsStr::to_str)
        .filter(|name| !name.is_empty())
        .unwrap_or("script")
        .to_owned();
    let package_dir = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let entry_name = target
        .file_name()
        .map(PathBuf::from)
        .ok_or_else(|| format!("error[E0300]: invalid script path {}", target.display()))?;

    let manifest = Manifest {
        package: Package { name: name.clone() },
        build: Build {
            entry: entry_name,
            sources: Some(Vec::new()),
        },
        dependencies: std::collections::HashMap::new(),
        pieces: std::collections::HashMap::new(),
    };

    let mut loader = Loader {
        source_map: SourceMap::default(),
        modules: Vec::new(),
        package_dirs: vec![package_dir.clone()],
        package_pieces: vec![std::collections::HashMap::new()],
        piece_visiting: HashSet::new(),
        token_count: 0,
        external: Vec::new(),
        visiting: HashSet::new(),
        loaded: HashSet::new(),
        next_package: 1,
        link_grant_sources: false,
    };
    loader.load_source(target, 0, &name)?;

    Ok(LoadedPackage {
        manifest,
        package_dir,
        entry_path: target.to_path_buf(),
        source_map: loader.source_map,
        token_count: loader.token_count,
        program: Program {
            modules: loader.modules,
            remotes: Vec::new(),
            generic_modules: Vec::new(),
            generic_module_instances: Vec::new(),
        },
        external: loader.external,
    })
}

impl Loader {
    fn load_package(&mut self, manifest_path: &Path) -> Result<(), String> {
        let canonical =
            std::fs::canonicalize(manifest_path).unwrap_or_else(|_| manifest_path.to_path_buf());

        if self.loaded.contains(&canonical) {
            return Ok(());
        }
        if !self.visiting.insert(canonical.clone()) {
            return Err(format!(
                "error[E0300]: dependency cycle involving {}",
                manifest_path.display()
            ));
        }

        let result = self.load_package_inner(manifest_path);
        self.visiting.remove(&canonical);
        if result.is_ok() {
            self.loaded.insert(canonical);
        }
        result
    }

    fn load_package_inner(&mut self, manifest_path: &Path) -> Result<(), String> {
        let manifest =
            Manifest::load(manifest_path).map_err(|error| format!("error[E0300]: {error}"))?;

        let package = self.next_package;
        self.next_package += 1;
        let package_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
        self.package_dirs.push(package_dir.to_path_buf());
        self.package_pieces.push(manifest.pieces.clone());

        for (name, path) in &manifest.dependencies {
            let dependency = if path.is_absolute() {
                path.clone()
            } else {
                package_dir.join(path)
            };
            if !dependency.exists() {
                return Err(format!(
                    "error[E0600]: dependency `{name}` does not exist: {}",
                    dependency.display()
                ));
            }

            let dependency_manifest = if dependency.is_dir() {
                Some(dependency.join("Chill.toml"))
            } else if dependency.file_name() == Some(OsStr::new("Chill.toml")) {
                Some(dependency.clone())
            } else {
                None
            };

            if let Some(dependency_manifest) = dependency_manifest {
                self.load_package(&dependency_manifest)?;
            } else {
                self.load_grant_dependency(name, &dependency)?;
            }
        }

        let source_paths = match &manifest.build.sources {
            Some(sources) => sources
                .iter()
                .map(|source| package_dir.join(source))
                .collect::<Vec<_>>(),
            None => discover_sources(package_dir, &manifest.build.entry)?,
        };
        for source in source_paths {
            self.load_source(&source, package, &manifest.package.name)?;
        }
        self.load_source(
            &package_dir.join(&manifest.build.entry),
            package,
            &manifest.package.name,
        )
    }

    fn load_source(
        &mut self,
        path: &Path,
        package: usize,
        package_name: &str,
    ) -> Result<(), String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("error[E0300]: cannot read {}: {error}", path.display()))?;
        let source_id = self.source_map.add(path.to_path_buf(), text);

        let tokens = {
            let source = &self
                .source_map
                .get(source_id)
                .expect("source was just added")
                .text;
            lexer::tokenize_with_source(source, source_id)
                .map_err(|errors| self.source_map.format_lex_errors(&errors))?
        };
        self.token_count += tokens.len();

        let program = parser::parse(&tokens).map_err(|error| {
            self.source_map
                .format_diagnostic(error.code, error.span, &error.message)
        })?;
        let mut modules = self.expand_program(program, path, package, package_name)?;
        for module in &mut modules {
            assign_package(module, package, package_name);
        }
        self.modules.extend(modules);
        Ok(())
    }

    fn expand_program(
        &mut self,
        program: Program,
        containing: &Path,
        package: usize,
        package_name: &str,
    ) -> Result<Vec<crate::ast::Module>, String> {
        let program = crate::program::normalize(&program).map_err(|error| {
            self.source_map
                .format_diagnostic(error.code, error.span, &error.message)
        })?;
        let mut modules = program.modules;
        for module in &mut modules {
            self.expand_module_contexts(module, containing, package)?;
        }
        for remote in program.remotes {
            modules.extend(self.expand_remote(remote, containing, package, package_name)?);
        }
        Ok(modules)
    }

    fn expand_module_contexts(
        &mut self,
        module: &mut crate::ast::Module,
        containing: &Path,
        package: usize,
    ) -> Result<(), String> {
        let contexts = std::mem::take(&mut module.contexts);
        module.contexts =
            self.expand_contexts(contexts, containing, package, module.name.as_deref())?;
        self.expand_action_contexts(&mut module.actions, containing, package)
    }

    fn expand_action_contexts(
        &mut self,
        actions: &mut [crate::ast::Action],
        containing: &Path,
        package: usize,
    ) -> Result<(), String> {
        for action in actions {
            match action {
                crate::ast::Action::Module(module) => {
                    self.expand_module_contexts(module, containing, package)?;
                }
                crate::ast::Action::If {
                    branches,
                    else_actions,
                    ..
                } => {
                    for branch in branches {
                        self.expand_action_contexts(&mut branch.body, containing, package)?;
                    }
                    if let Some(actions) = else_actions {
                        self.expand_action_contexts(actions, containing, package)?;
                    }
                }
                crate::ast::Action::Do { body, .. } => {
                    self.expand_action_contexts(body, containing, package)?;
                }
                crate::ast::Action::Case {
                    alternatives,
                    else_actions,
                    ..
                } => {
                    for alternative in alternatives {
                        self.expand_action_contexts(&mut alternative.body, containing, package)?;
                    }
                    if let Some(actions) = else_actions {
                        self.expand_action_contexts(actions, containing, package)?;
                    }
                }
                crate::ast::Action::Labelled { action, .. } => {
                    self.expand_action_contexts(
                        std::slice::from_mut(action.as_mut()),
                        containing,
                        package,
                    )?;
                }
                crate::ast::Action::Begin { actions, .. } => {
                    self.expand_action_contexts(actions, containing, package)?;
                }
                crate::ast::Action::Handled { action, .. } => {
                    self.expand_action_contexts(
                        std::slice::from_mut(action.as_mut()),
                        containing,
                        package,
                    )?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn expand_contexts(
        &mut self,
        contexts: Vec<crate::ast::Context>,
        containing: &Path,
        package: usize,
        unit_name: Option<&str>,
    ) -> Result<Vec<crate::ast::Context>, String> {
        let mut result = Vec::with_capacity(contexts.len());
        for context in contexts {
            let Some(designator) = context.remote.clone() else {
                result.push(context);
                continue;
            };

            let resolver = crate::pieces::PieceResolver::new(
                &self.package_dirs[package],
                &self.package_pieces[package],
            );
            let canonical = resolver
                .resolve_checked(&designator, unit_name, containing, &mut self.piece_visiting)
                .map_err(|error| format!("error[E0700]: {error}"))?;

            let expanded = (|| {
                let mut referenced = self.parse_context_source_file(&canonical)?;
                referenced = self.expand_contexts(referenced, &canonical, package, unit_name)?;
                let Some(last) = referenced.last_mut() else {
                    return Err(format!(
                        "error[E0700]: piece {} is an empty context list",
                        canonical.display()
                    ));
                };
                last.declarations.extend(context.declarations);
                Ok(referenced)
            })();

            self.piece_visiting.remove(&canonical);
            result.extend(expanded?);
        }
        Ok(result)
    }

    fn parse_context_source_file(
        &mut self,
        path: &Path,
    ) -> Result<Vec<crate::ast::Context>, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("error[E0700]: cannot read {}: {error}", path.display()))?;
        let source_id = self.source_map.add(path.to_path_buf(), text);
        let tokens = {
            let source = &self
                .source_map
                .get(source_id)
                .expect("source was just added")
                .text;
            lexer::tokenize_with_source(source, source_id)
                .map_err(|errors| self.source_map.format_lex_errors(&errors))?
        };
        self.token_count += tokens.len();
        parser::parse_context_piece(&tokens).map_err(|error| {
            format!(
                "error[E0700]: {}: {}",
                path.display(),
                self.source_map
                    .format_diagnostic(error.code, error.span, &error.message)
            )
        })
    }

    fn expand_context_module(
        &mut self,
        remote: crate::ast::RemoteUnit,
        containing: &Path,
        package: usize,
    ) -> Result<Vec<crate::ast::Module>, String> {
        let resolver = crate::pieces::PieceResolver::new(
            &self.package_dirs[package],
            &self.package_pieces[package],
        );
        let canonical = resolver
            .resolve_checked(
                &remote.designator,
                remote.name.as_deref(),
                containing,
                &mut self.piece_visiting,
            )
            .map_err(|error| format!("error[E0700]: {error}"))?;

        let result = (|| {
            let mut contexts = self.parse_context_source_file(&canonical)?;
            contexts =
                self.expand_contexts(contexts, &canonical, package, remote.name.as_deref())?;
            let declaration = crate::ast::Declaration::Visibility(crate::ast::Visibility {
                kind: crate::ast::VisibilityKind::Seize,
                names: Vec::new(),
                all: true,
                friends: Vec::new(),
                prefix: None,
                forbid: None,
                span: remote.span,
            });
            Ok(vec![crate::ast::Module {
                name: None,
                kind: crate::ast::ModuleKind::Body,
                package: 0,
                package_name: None,
                contexts,
                declarations: vec![declaration],
                actions: Vec::new(),
                handler: None,
                span: remote.span,
            }])
        })();

        self.piece_visiting.remove(&canonical);
        result
    }

    fn expand_remote(
        &mut self,
        remote: crate::ast::RemoteUnit,
        containing: &Path,
        package: usize,
        package_name: &str,
    ) -> Result<Vec<crate::ast::Module>, String> {
        if remote.kind == crate::ast::RemoteKind::ContextModule {
            return self.expand_context_module(remote, containing, package);
        }

        let resolver = crate::pieces::PieceResolver::new(
            &self.package_dirs[package],
            &self.package_pieces[package],
        );
        let canonical = resolver
            .resolve_checked(
                &remote.designator,
                remote.name.as_deref(),
                containing,
                &mut self.piece_visiting,
            )
            .map_err(|error| format!("error[E0700]: {error}"))?;

        let result = (|| {
            let (piece, _) = self.parse_source_file(&canonical)?;
            if !piece.remotes.is_empty() {
                return Err(format!(
                    "error[E0700]: piece {} is itself a remote program unit",
                    canonical.display()
                ));
            }
            if piece.modules.len() != 1 {
                return Err(format!(
                    "error[E0700]: piece {} must contain exactly one program unit",
                    canonical.display()
                ));
            }

            let mut modules = piece.modules;
            self.expand_module_contexts(&mut modules[0], &canonical, package)?;
            let module = &modules[0];

            match remote.kind {
                crate::ast::RemoteKind::Module if !module.kind.is_body() => {
                    return Err(format!(
                        "error[E0700]: piece {} is not a module or region",
                        canonical.display()
                    ));
                }
                crate::ast::RemoteKind::SpecModule if !module.kind.is_spec() => {
                    return Err(format!(
                        "error[E0700]: piece {} is not a spec module or spec region",
                        canonical.display()
                    ));
                }
                _ => {}
            }
            if let Some(name) = &remote.name {
                if module.name.as_deref() != Some(name.as_str()) {
                    return Err(format!(
                        "error[E0700]: piece {} defines `{}`, not `{name}`",
                        canonical.display(),
                        module.name.as_deref().unwrap_or("<unnamed>")
                    ));
                }
            }

            let _ = package_name;
            Ok(modules)
        })();

        self.piece_visiting.remove(&canonical);
        result
    }

    fn parse_source_file(
        &mut self,
        path: &Path,
    ) -> Result<(Program, crate::lexer::SourceId), String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("error[E0700]: cannot read {}: {error}", path.display()))?;
        let source_id = self.source_map.add(path.to_path_buf(), text);
        let tokens = {
            let source = &self
                .source_map
                .get(source_id)
                .expect("source was just added")
                .text;
            lexer::tokenize_with_source(source, source_id)
                .map_err(|errors| self.source_map.format_lex_errors(&errors))?
        };
        self.token_count += tokens.len();
        let program = parser::parse(&tokens).map_err(|error| {
            self.source_map
                .format_diagnostic(error.code, error.span, &error.message)
        })?;
        Ok((program, source_id))
    }

    fn load_grant_dependency(&mut self, name: &str, path: &Path) -> Result<(), String> {
        let text = std::fs::read_to_string(path).map_err(|error| {
            format!(
                "error[E0600]: cannot read dependency `{name}` grant file {}: {error}",
                path.display()
            )
        })?;
        let grant_file = grant::GrantFile::from_toml(&text)
            .map_err(|error| format!("error[E0600]: {}: {error}", path.display()))?;

        if self.link_grant_sources {
            if let Some(package_dir) = &grant_file.package_dir {
                let package_dir = PathBuf::from(package_dir);
                let package_dir = if package_dir.is_absolute() {
                    package_dir
                } else {
                    path.parent()
                        .unwrap_or_else(|| Path::new("."))
                        .join(package_dir)
                };
                if !package_dir.is_dir() {
                    return Err(format!(
                        "error[E0600]: dependency `{name}` source package does not exist: {}",
                        package_dir.display()
                    ));
                }
                self.load_package(&package_dir.join("Chill.toml"))?;
                return Ok(());
            }
        }

        self.external.push(
            grant_file
                .to_interface()
                .map_err(|error| format!("error[E0600]: {}: {error}", path.display()))?,
        );
        Ok(())
    }
}

fn discover_sources(package_dir: &Path, entry: &Path) -> Result<Vec<PathBuf>, String> {
    let entry_path = package_dir.join(entry);
    let Some(directory) = entry_path.parent() else {
        return Ok(Vec::new());
    };
    if !directory.is_dir() {
        return Ok(Vec::new());
    }

    let mut sources = Vec::new();
    for entry in std::fs::read_dir(directory)
        .map_err(|error| format!("error[E0300]: cannot read {}: {error}", directory.display()))?
    {
        let path = entry
            .map_err(|error| format!("error[E0300]: cannot read {}: {error}", directory.display()))?
            .path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("ch") {
            continue;
        }
        if path.file_name() == entry_path.file_name() && path.parent() == entry_path.parent() {
            continue;
        }
        sources.push(path);
    }
    sources.sort();
    Ok(sources)
}

fn assign_package(module: &mut crate::ast::Module, package: usize, package_name: &str) {
    module.package = package;
    module.package_name = Some(package_name.to_owned());
    assign_action_packages(&mut module.actions, package, package_name);
}

fn assign_action_packages(actions: &mut [crate::ast::Action], package: usize, package_name: &str) {
    for action in actions {
        match action {
            crate::ast::Action::Module(module) => {
                assign_package(module, package, package_name);
            }
            crate::ast::Action::If {
                branches,
                else_actions,
                ..
            } => {
                for branch in branches {
                    assign_action_packages(&mut branch.body, package, package_name);
                }
                if let Some(actions) = else_actions {
                    assign_action_packages(actions, package, package_name);
                }
            }
            crate::ast::Action::Do { body, .. } => {
                assign_action_packages(body, package, package_name);
            }
            crate::ast::Action::Case {
                alternatives,
                else_actions,
                ..
            } => {
                for alternative in alternatives {
                    assign_action_packages(&mut alternative.body, package, package_name);
                }
                if let Some(actions) = else_actions {
                    assign_action_packages(actions, package, package_name);
                }
            }
            crate::ast::Action::Labelled { action, .. } => {
                assign_action_packages(std::slice::from_mut(action), package, package_name);
            }
            crate::ast::Action::Begin { actions, .. } => {
                assign_action_packages(actions, package, package_name);
            }
            crate::ast::Action::Handled { action, .. } => {
                assign_action_packages(
                    std::slice::from_mut(action.as_mut()),
                    package,
                    package_name,
                );
            }
            _ => {}
        }
    }
}

fn resolve_manifest(target: &Path) -> Result<PathBuf, String> {
    if target.is_dir() {
        return Ok(target.join("Chill.toml"));
    }

    if target.is_file() {
        if target.file_name() == Some(OsStr::new("Chill.toml")) {
            return Ok(target.to_path_buf());
        }
        if let Some(candidate) = target.parent().map(|parent| parent.join("Chill.toml")) {
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        return Err(format!(
            "error[E0300]: no Chill.toml found for {}",
            target.display()
        ));
    }

    Err(format!(
        "error[E0300]: path does not exist: {}",
        target.display()
    ))
}

fn print_help() {
    println!(
        "z200c {VERSION}
Usage:
  z200c check [path]    Parse and mode-check a package
  z200c run [path]      Execute a package
  z200c compile [path]  Compile a supported subset to a native executable
  z200c grant [path]    Parse, check, and write the package grant file
  z200c build [path]    Alias for `grant`
  z200c --version
  z200c --help

`path` may be a package directory, a Chill.toml file, or a single `.ch` script
(default: .). A script is checked and run without a manifest."
    );
}
