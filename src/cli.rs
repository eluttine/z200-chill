use crate::ast::{Declaration, Program, VisibilityKind};
use crate::diagnostics;
use crate::grant;
use crate::interp;
use crate::lexer;
use crate::manifest::{Manifest, WidthProfile};
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
    let package = load_package(target)?;

    let diagnostics = sema::check_all_with_externals(
        &package.program,
        package.manifest.build.int_widths,
        &package.external,
    );
    if !diagnostics.is_empty() {
        return Err(diagnostics::render(&package.source_map, &diagnostics));
    }

    let grant_file =
        grant::GrantFile::from_package(&package.manifest, &package.program).map_err(|error| {
            package
                .source_map
                .format_diagnostic(error.code, error.span, &error.message)
        })?;
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
    let package = load_package(target)?;

    let diagnostics = sema::check_all_with_externals(
        &package.program,
        package.manifest.build.int_widths,
        &package.external,
    );
    if !diagnostics.is_empty() {
        return Err(diagnostics::render(&package.source_map, &diagnostics));
    }

    Ok(format!(
        "checked {} (edition {}, widths {})\n  {}: {} tokens, {} module(s)",
        package.manifest.package.name,
        package.manifest.package.edition,
        package.manifest.build.int_widths,
        package.entry_path.display(),
        package.token_count,
        package.program.modules.len()
    ))
}

fn run_impl(target: &Path) -> Result<(), String> {
    let package = load_package(target)?;

    let check = sema::analyze_with_externals(
        &package.program,
        package.manifest.build.int_widths,
        &package.external,
    );
    if !check.diagnostics.is_empty() {
        return Err(diagnostics::render(&package.source_map, &check.diagnostics));
    }

    if let Some(name) = external_seize(&package.program, &package.external) {
        return Err(format!(
            "error[E0500]: cannot run `{name}`: external dependencies are not linked yet"
        ));
    }

    interp::run_checked(&package.program, package.manifest.build.int_widths, &check).map_err(
        |error| {
            package
                .source_map
                .format_diagnostic(error.code, error.span, &error.message)
        },
    )
}

fn external_seize(program: &Program, external: &[program::ExportInfo]) -> Option<String> {
    let names: HashSet<&str> = external.iter().map(|info| info.name.as_str()).collect();

    for module in &program.modules {
        for declaration in &module.declarations {
            let Declaration::Visibility(visibility) = declaration else {
                continue;
            };
            if visibility.kind != VisibilityKind::Seize {
                continue;
            }
            for name in &visibility.names {
                if names.contains(name.name.as_str()) {
                    return Some(name.name.clone());
                }
            }
        }
    }

    None
}

struct LoadedPackage {
    manifest: Manifest,
    package_dir: PathBuf,
    entry_path: PathBuf,
    source_map: SourceMap,
    token_count: usize,
    program: Program,
    external: Vec<program::ExportInfo>,
}

struct Loader {
    source_map: SourceMap,
    modules: Vec<crate::ast::Module>,
    token_count: usize,
    external: Vec<program::ExportInfo>,
    visiting: HashSet<PathBuf>,
    loaded: HashSet<PathBuf>,
    root_edition: crate::manifest::Edition,
    root_widths: WidthProfile,
}

fn load_package(target: &Path) -> Result<LoadedPackage, String> {
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
        token_count: 0,
        external: Vec::new(),
        visiting: HashSet::new(),
        loaded: HashSet::new(),
        root_edition: manifest.package.edition,
        root_widths: manifest.build.int_widths,
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

        if manifest.package.edition != self.root_edition {
            return Err(format!(
                "error[E0600]: {} uses edition {}, expected {}",
                manifest_path.display(),
                manifest.package.edition,
                self.root_edition
            ));
        }
        if manifest.build.int_widths != self.root_widths {
            return Err(format!(
                "error[E0600]: {} uses int_widths {}, expected {}",
                manifest_path.display(),
                manifest.build.int_widths,
                self.root_widths
            ));
        }

        let package_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));

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

        for source in &manifest.build.sources {
            self.load_source(&package_dir.join(source))?;
        }
        self.load_source(&package_dir.join(&manifest.build.entry))
    }

    fn load_source(&mut self, path: &Path) -> Result<(), String> {
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
        self.modules.extend(program.modules);
        Ok(())
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

        if grant_file.edition != self.root_edition.as_str() {
            return Err(format!(
                "error[E0600]: dependency `{name}` uses edition {}, expected {}",
                grant_file.edition, self.root_edition
            ));
        }
        let dependency_widths = WidthProfile::parse(&grant_file.int_widths)
            .map_err(|error| format!("error[E0600]: dependency `{name}`: {error}"))?;
        if dependency_widths != self.root_widths {
            return Err(format!(
                "error[E0600]: dependency `{name}` uses int_widths {}, expected {}",
                dependency_widths, self.root_widths
            ));
        }

        self.external.extend(
            grant_file
                .to_export_infos()
                .map_err(|error| format!("error[E0600]: {}: {error}", path.display()))?,
        );
        Ok(())
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
  z200c grant [path]    Parse, check, and write the package grant file
  z200c build [path]    Alias for `grant`
  z200c --version
  z200c --help

`path` may be a package directory or a Chill.toml file (default: .)."
    );
}
