use crate::ast::Program;
use crate::interp;
use crate::lexer::{self, Span};
use crate::manifest::Manifest;
use crate::parser;
use crate::sema;
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
        Some("build") => {
            eprintln!("error: `build` is not implemented yet; try `z200c run <path>`");
            2
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

fn check_impl(target: &Path) -> Result<String, String> {
    let package = load_package(target)?;

    sema::check(&package.program, package.manifest.build.int_widths).map_err(|error| {
        format_span_error(
            &package.entry_path,
            &package.source,
            error.span,
            &error.message,
        )
    })?;

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

    sema::check(&package.program, package.manifest.build.int_widths).map_err(|error| {
        format_span_error(
            &package.entry_path,
            &package.source,
            error.span,
            &error.message,
        )
    })?;

    interp::run(&package.program, package.manifest.build.int_widths).map_err(|error| {
        format_span_error(
            &package.entry_path,
            &package.source,
            error.span,
            &error.message,
        )
    })
}

struct LoadedPackage {
    manifest: Manifest,
    entry_path: PathBuf,
    source: String,
    token_count: usize,
    program: Program,
}

fn load_package(target: &Path) -> Result<LoadedPackage, String> {
    let manifest_path = resolve_manifest(target)?;
    let manifest = Manifest::load(&manifest_path).map_err(|error| format!("error: {error}"))?;

    let package_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let entry_path = package_dir.join(&manifest.build.entry);

    let source = std::fs::read_to_string(&entry_path)
        .map_err(|error| format!("error: cannot read {}: {error}", entry_path.display()))?;

    let tokens = lexer::tokenize(&source)
        .map_err(|errors| format_lex_errors(&entry_path, &source, &errors))?;
    let token_count = tokens.len();

    let program = parser::parse(&tokens)
        .map_err(|error| format_span_error(&entry_path, &source, error.span, &error.message))?;

    Ok(LoadedPackage {
        manifest,
        entry_path,
        source,
        token_count,
        program,
    })
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
            "error: no Chill.toml found for {}",
            target.display()
        ));
    }

    Err(format!("error: path does not exist: {}", target.display()))
}

fn format_lex_errors(path: &Path, source: &str, errors: &[lexer::Error]) -> String {
    let mut lines = Vec::with_capacity(errors.len());
    for error in errors {
        let (line, column) = lexer::line_col(source, error.offset);
        lines.push(format!(
            "{}:{line}:{column}: error: {}",
            path.display(),
            error.message
        ));
    }
    lines.join("\n")
}

fn format_span_error(path: &Path, source: &str, span: Span, message: &str) -> String {
    let (line, column) = lexer::line_col(source, span.start);
    format!("{}:{line}:{column}: error: {message}", path.display())
}

fn print_help() {
    println!(
        "z200c {VERSION}
Usage:
  z200c check [path]    Parse and mode-check a package
  z200c run [path]      Execute a package
  z200c build [path]    Build a package (not implemented yet)
  z200c --version
  z200c --help

`path` may be a package directory or a Chill.toml file (default: .)."
    );
}
