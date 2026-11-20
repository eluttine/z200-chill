use std::fs;
use std::path::{Path, PathBuf};

use z200c::{interp, lexer, manifest::WidthProfile, parser, sema};

fn fixtures(directory: &str) -> Vec<PathBuf> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(directory);
    let mut files: Vec<PathBuf> = fs::read_dir(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("ch"))
        .collect();
    files.sort();
    files
}

fn expected_error(path: &Path) -> Option<String> {
    fs::read_to_string(path.with_extension("error"))
        .ok()
        .map(|text| text.trim().to_owned())
}

fn parse_source(source: &str) -> Result<z200c::ast::Program, String> {
    let tokens = lexer::tokenize(source).map_err(|errors| {
        errors
            .first()
            .map(|error| error.message.clone())
            .unwrap_or_default()
    })?;
    parser::parse(&tokens).map_err(|error| error.message)
}

#[test]
fn parse_ok_fixtures() {
    for file in fixtures("parse_ok") {
        let source = fs::read_to_string(&file).unwrap();
        parse_source(&source).unwrap_or_else(|error| panic!("{}: {error}", file.display()));
    }
}

#[test]
fn parse_fail_fixtures() {
    for file in fixtures("parse_fail") {
        let source = fs::read_to_string(&file).unwrap();
        let error = parse_source(&source)
            .err()
            .unwrap_or_else(|| panic!("{}: expected a parse error", file.display()));
        if let Some(expected) = expected_error(&file) {
            assert!(
                error.contains(&expected),
                "{}: expected `{expected}`, got `{error}`",
                file.display()
            );
        }
    }
}

#[test]
fn sema_fail_fixtures() {
    for file in fixtures("sema_fail") {
        let source = fs::read_to_string(&file).unwrap();
        let program =
            parse_source(&source).unwrap_or_else(|error| panic!("{}: {error}", file.display()));
        let error = sema::check(&program, WidthProfile::default())
            .err()
            .unwrap_or_else(|| panic!("{}: expected a semantic error", file.display()));
        if let Some(expected) = expected_error(&file) {
            assert!(
                error.message.contains(&expected),
                "{}: expected `{expected}`, got `{}`",
                file.display(),
                error.message
            );
        }
    }
}

#[test]
fn run_ok_fixtures() {
    for file in fixtures("run_ok") {
        let source = fs::read_to_string(&file).unwrap();
        let program =
            parse_source(&source).unwrap_or_else(|error| panic!("{}: {error}", file.display()));
        sema::check(&program, WidthProfile::default())
            .unwrap_or_else(|error| panic!("{}: {}", file.display(), error.message));
        interp::run(&program, WidthProfile::default())
            .unwrap_or_else(|error| panic!("{}: {}", file.display(), error.message));
    }
}
