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
fn build_writes_grant_file() {
    let output = z200c()
        .arg("build")
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
