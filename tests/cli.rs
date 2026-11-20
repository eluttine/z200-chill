use std::process::Command;

fn z200c() -> Command {
    Command::new(env!("CARGO_BIN_EXE_z200c"))
}

#[test]
fn check_hello_succeeds() {
    let package = format!("{}/examples/hello", env!("CARGO_MANIFEST_DIR"));
    let output = z200c().arg("check").arg(&package).output().unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("checked hello"));
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
