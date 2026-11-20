use std::process::Command;

fn z200c() -> Command {
    Command::new(env!("CARGO_BIN_EXE_z200c"))
}

fn example(name: &str) -> String {
    format!("{}/examples/{}", env!("CARGO_MANIFEST_DIR"), name)
}

#[test]
fn check_hello_succeeds() {
    let output = z200c().arg("check").arg(example("hello")).output().unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("checked hello"));
}

#[test]
fn run_hello_succeeds_without_output() {
    let output = z200c().arg("run").arg(example("hello")).output().unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn run_modules_example_succeeds() {
    let output = z200c().arg("run").arg(example("modules")).output().unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn run_spec_example_succeeds() {
    let output = z200c().arg("run").arg(example("spec")).output().unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn run_control_example_succeeds() {
    let output = z200c().arg("run").arg(example("control")).output().unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn run_multifile_example_succeeds() {
    let output = z200c()
        .arg("run")
        .arg(example("multifile"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn run_directory_dependency_succeeds() {
    let output = z200c().arg("run").arg(example("app")).output().unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn grant_writes_grant_file() {
    let output = z200c()
        .arg("grant")
        .arg(example("modules"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let path = format!(
        "{}/examples/modules/target/z200c/modules.grt.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains("name = \"Bump\""));
    assert!(text.contains("name = \"value\""));
    assert!(text.contains("mode = \"INT\""));
}

#[test]
fn checks_against_dependency_grant_file() {
    let dir = std::env::temp_dir().join(format!("z200c-dep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();

    std::fs::write(
        dir.join("lib.grt.toml"),
        r#"
schema = 1
package = "lib"
edition = "z200-11/99"
int_widths = "16-32-64"

[[exports]]
name = "x"
kind = "variable"
mode = "INT"

[[exports]]
name = "F"
kind = "procedure"
result = "INT"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "client"
version = "0.1.0"
edition = "z200-11/99"

[build]
entry = "src/main.ch"
int_widths = "16-32-64"

[dependencies]
lib = "lib.grt.toml"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Client: MODULE
  SEIZE x, F;
  ASSERT x = 1;
  ASSERT F() = 2;
END Client;
"#,
    )
    .unwrap();

    let output = z200c().arg("check").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let run = z200c().arg("run").arg(&dir).output().unwrap();
    assert_eq!(run.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&run.stderr).contains("not linked"));

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn run_allows_unused_dependency() {
    let dir = std::env::temp_dir().join(format!("z200c-unused-dep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();

    std::fs::write(
        dir.join("lib.grt.toml"),
        r#"
schema = 1
package = "lib"
edition = "z200-11/99"
int_widths = "16-32-64"

[[exports]]
name = "unused"
kind = "variable"
mode = "INT"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "client"
version = "0.1.0"
edition = "z200-11/99"

[build]
entry = "src/main.ch"
int_widths = "16-32-64"

[dependencies]
lib = "lib.grt.toml"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Client: MODULE
  ASSERT TRUE;
END Client;
"#,
    )
    .unwrap();

    let output = z200c().arg("run").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn check_reports_multiple_semantic_errors() {
    let dir = std::env::temp_dir().join(format!("z200c-multi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();

    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "multi"
version = "0.1.0"
edition = "z200-11/99"

[build]
entry = "src/main.ch"
int_widths = "16-32-64"
"#,
    )
    .unwrap();

    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a SHORT_INT;
  DCL b LONG_INT;
  a := b;
  a := b;
END M;
"#,
    )
    .unwrap();

    let output = z200c().arg("check").arg(&dir).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.matches("error[E0404]").count(), 2);

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn run_links_grant_file_with_source_metadata() {
    let grant = z200c()
        .arg("grant")
        .arg(example("liblocal"))
        .output()
        .unwrap();
    assert!(
        grant.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&grant.stdout),
        String::from_utf8_lossy(&grant.stderr)
    );

    let grant_path = format!(
        "{}/examples/liblocal/target/z200c/liblocal.grt.toml",
        env!("CARGO_MANIFEST_DIR")
    );

    let dir = std::env::temp_dir().join(format!("z200c-grant-run-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();

    std::fs::write(
        dir.join("Chill.toml"),
        format!(
            r#"
[package]
name = "client"
version = "0.1.0"
edition = "z200-11/99"

[build]
entry = "src/main.ch"
int_widths = "16-32-64"

[dependencies]
liblocal = "{grant_path}"
"#
        ),
    )
    .unwrap();

    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Client: MODULE
  SEIZE value;
  ASSERT value = 10;
END Client;
"#,
    )
    .unwrap();

    let output = z200c().arg("run").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn check_resolves_qualified_grant_exports() {
    let grant = z200c()
        .arg("grant")
        .arg(example("modules"))
        .output()
        .unwrap();
    assert!(
        grant.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&grant.stdout),
        String::from_utf8_lossy(&grant.stderr)
    );

    let grant_path = format!(
        "{}/examples/modules/target/z200c/modules.grt.toml",
        env!("CARGO_MANIFEST_DIR")
    );

    let dir = std::env::temp_dir().join(format!("z200c-grant-qualified-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();

    std::fs::write(
        dir.join("Chill.toml"),
        format!(
            r#"
[package]
name = "client"
version = "0.1.0"
edition = "z200-11/99"

[build]
entry = "src/main.ch"
int_widths = "16-32-64"

[dependencies]
modules = "{grant_path}"
"#
        ),
    )
    .unwrap();

    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Client: MODULE
  SEIZE Counter!value;
  ASSERT value = 40;
END Client;
"#,
    )
    .unwrap();

    let output = z200c().arg("check").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn duplicate_module_names_across_packages_are_allowed() {
    let base = std::env::temp_dir().join(format!("z200c-packages-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    for (name, variable, value) in [("one", "a", 1), ("two", "b", 2)] {
        let dir = base.join(name);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Chill.toml"),
            format!(
                r#"
[package]
name = "{name}"
version = "0.1.0"
edition = "z200-11/99"

[build]
entry = "src/main.ch"
int_widths = "16-32-64"
"#
            ),
        )
        .unwrap();
        std::fs::write(
            dir.join("src/main.ch"),
            format!(
                r#"
Library: MODULE
  GRANT {variable};
  DCL {variable} INT INIT := {value};
END Library;
"#
            ),
        )
        .unwrap();
    }

    let client = base.join("client");
    std::fs::create_dir_all(client.join("src")).unwrap();
    std::fs::write(
        client.join("Chill.toml"),
        r#"
[package]
name = "client"
version = "0.1.0"
edition = "z200-11/99"

[build]
entry = "src/main.ch"
int_widths = "16-32-64"

[dependencies]
one = "../one"
two = "../two"
"#,
    )
    .unwrap();
    std::fs::write(
        client.join("src/main.ch"),
        r#"
Client: MODULE
  SEIZE a, b;
  ASSERT a = 1;
  ASSERT b = 2;
END Client;
"#,
    )
    .unwrap();

    let output = z200c().arg("run").arg(&client).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn version_prints_package_version() {
    let output = z200c().arg("--version").output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn unknown_command_fails() {
    let output = z200c().arg("frobnicate").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}
