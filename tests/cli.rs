use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

fn example_grant_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn z200c() -> Command {
    Command::new(env!("CARGO_BIN_EXE_z200c"))
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("src")).unwrap();
        Self { path }
    }

    fn join(&self, path: impl AsRef<Path>) -> PathBuf {
        self.path.join(path)
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<std::ffi::OsStr> for TempDir {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.path.as_os_str()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
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
fn run_hello_prints_greeting() {
    let output = z200c().arg("run").arg(example("hello")).output().unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"Hello, world!\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn run_text_output_matches_golden() {
    let directory = TempDir::new("z200c-golden");
    std::fs::write(
        directory.join("Chill.toml"),
        r#"
[package]
name = "golden"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        directory.join("src/main.ch"),
        r#"
M: MODULE
  WRITE("value=");
  WRITELINE("ok");
  WRITELINE();
END M;
"#,
    )
    .unwrap();

    let output = z200c().arg("run").arg(&directory).output().unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"value=ok\n\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn run_procedures_example_succeeds_without_output() {
    let output = z200c()
        .arg("run")
        .arg(example("procedures"))
        .output()
        .unwrap();

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
fn run_pipeline_example_succeeds() {
    let output = z200c()
        .arg("run")
        .arg(example("pipeline"))
        .output()
        .unwrap();

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
fn pipeline_is_stable_under_yield_variations() {
    for interval in ["1", "2", "3", "5", "8", "13"] {
        let output = z200c()
            .env("Z200C_YIELD_INTERVAL", interval)
            .arg("run")
            .arg(example("pipeline"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "yield interval {interval}: stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
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
fn run_showcase_example_succeeds() {
    let output = z200c()
        .arg("run")
        .arg(example("showcase"))
        .output()
        .unwrap();
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
fn compiles_native_hello_world() {
    let dir = TempDir::new("z200c-native-hello");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativehello"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Hello: MODULE
  WRITELINE("Hello, native!");
END Hello;
"#,
    )
    .unwrap();

    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativehello");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "Hello, native!\n");
}

#[test]
fn compiles_native_integer_program() {
    let dir = TempDir::new("z200c-native-int");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativeint"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL x INT INIT := 2;
  DCL y INT INIT := 3;
  DCL z INT;
  z := x + y * 2;
  ASSERT z = 8;
  IF z = 8 THEN
    WRITELINE("native ok");
  ELSE
    WRITELINE("bad");
  FI;
END M;
"#,
    )
    .unwrap();

    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativeint");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "native ok\n");
}

#[test]
fn compiles_native_procedures_example() {
    let output = z200c()
        .arg("compile")
        .arg(example("procedures"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = format!(
        "{}/examples/procedures/target/z200c/procedures",
        env!("CARGO_MANIFEST_DIR")
    );
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert!(run.stdout.is_empty());
}

#[test]
fn compiles_native_array_program() {
    let dir = TempDir::new("z200c-native-array");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativearray"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a ARRAY(1:3) INT;
  a(1) := 4;
  a(2) := 5;
  a(3) := a(1) + a(2);
  ASSERT a(3) = 9;
  WRITELINE("array ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativearray");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "array ok\n");
}

#[test]
fn compiles_native_loop_program() {
    let dir = TempDir::new("z200c-native-loop");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativeloop"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a ARRAY(1:3) INT;
  DCL sum INT INIT := 0;
  DO FOR i := 1 TO 3;
    a(i) := i * 2;
    sum := sum + a(i);
  OD;
  ASSERT sum = 12;
  DO WHILE sum > 10;
    sum := sum - 1;
  OD;
  ASSERT sum = 10;
  WRITELINE("loop ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativeloop");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "loop ok\n");
}

#[test]
fn compiles_native_array_literals() {
    let dir = TempDir::new("z200c-native-arraylit");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativearraylit"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a ARRAY(1:3) INT INIT := [1, 2, 3];
  DCL b ARRAY(1:3) INT;
  b := [4, 5, 6];
  ASSERT a(2) = 2;
  ASSERT b(3) = 6;
  WRITELINE("lit ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativearraylit");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "lit ok\n");
}

#[test]
fn compiles_native_array_copy() {
    let dir = TempDir::new("z200c-native-arraycopy");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativearraycopy"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL a ARRAY(1:3) INT INIT := [1, 2, 3];
  DCL b ARRAY(1:3) INT;
  b := a;
  ASSERT b(1) = 1;
  ASSERT b(3) = 3;
  WRITELINE("copy ok");
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let binary = dir.join("target/z200c/nativearraycopy");
    let run = std::process::Command::new(binary).output().unwrap();
    assert!(run.status.success());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "copy ok\n");
}

#[test]
fn native_backend_rejects_unsupported_arrays() {
    let dir = TempDir::new("z200c-native-array");
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nativearray"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL values ARRAY(1:2, 1:2) INT;
END M;
"#,
    )
    .unwrap();
    let output = z200c().arg("compile").arg(&dir).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not supported"), "stderr: {stderr}");
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
    let _guard = example_grant_lock().lock().unwrap();
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
    let dir = TempDir::new("z200c-dep");
    std::fs::create_dir_all(dir.join("src")).unwrap();

    std::fs::write(
        dir.join("lib.grt.toml"),
        r#"
schema = 4
package = "lib"

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

[build]
entry = "src/main.ch"

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
}

#[test]
fn run_allows_unused_dependency() {
    let dir = TempDir::new("z200c-unused-dep");
    std::fs::create_dir_all(dir.join("src")).unwrap();

    std::fs::write(
        dir.join("lib.grt.toml"),
        r#"
schema = 4
package = "lib"

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

[build]
entry = "src/main.ch"

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
}

#[test]
fn check_reports_multiple_semantic_errors() {
    let dir = TempDir::new("z200c-multi");
    std::fs::create_dir_all(dir.join("src")).unwrap();

    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "multi"

[build]
entry = "src/main.ch"
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

    let dir = TempDir::new("z200c-grant-run");
    std::fs::create_dir_all(dir.join("src")).unwrap();

    std::fs::write(
        dir.join("Chill.toml"),
        format!(
            r#"
[package]
name = "client"

[build]
entry = "src/main.ch"

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
}

#[test]
fn check_resolves_qualified_grant_exports() {
    let _guard = example_grant_lock().lock().unwrap();
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

    let dir = TempDir::new("z200c-grant-qualified");
    std::fs::create_dir_all(dir.join("src")).unwrap();

    std::fs::write(
        dir.join("Chill.toml"),
        format!(
            r#"
[package]
name = "client"

[build]
entry = "src/main.ch"

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
}

#[test]
fn duplicate_module_names_across_packages_are_allowed() {
    let base = TempDir::new("z200c-packages");

    for (name, variable, value) in [("one", "a", 1), ("two", "b", 2)] {
        let dir = base.join(name);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Chill.toml"),
            format!(
                r#"
[package]
name = "{name}"

[build]
entry = "src/main.ch"
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

[build]
entry = "src/main.ch"

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
}

#[test]
fn runs_with_structure_mode_from_grant_file() {
    let base = TempDir::new("z200c-struct-grant");
    let lib = base.join("libstruct");
    std::fs::create_dir_all(lib.join("src")).unwrap();

    std::fs::write(
        lib.join("Chill.toml"),
        r#"
[package]
name = "structlib"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        lib.join("src/main.ch"),
        r#"
A: MODULE
  NEWMODE S = STRUCT(x INT, hidden BOOL);
  GRANT S FORBID (hidden);
END A;
"#,
    )
    .unwrap();

    let grant = z200c().arg("grant").arg(&lib).output().unwrap();
    assert!(
        grant.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&grant.stdout),
        String::from_utf8_lossy(&grant.stderr)
    );
    let grant_path = lib.join("target/z200c/structlib.grt.toml");
    let text = std::fs::read_to_string(&grant_path).unwrap();
    assert!(text.contains("schema = 4"));
    assert!(text.contains("[[modes]]"));
    assert!(text.contains("mode = \"structlib!A!S\""));

    let client = base.join("client");
    std::fs::create_dir_all(client.join("src")).unwrap();
    std::fs::write(
        client.join("Chill.toml"),
        format!(
            r#"
[package]
name = "client"

[build]
entry = "src/main.ch"

[dependencies]
structlib = "{}"
"#,
            grant_path.display()
        ),
    )
    .unwrap();
    std::fs::write(
        client.join("src/main.ch"),
        r#"
Client: MODULE
  SEIZE S;
  DCL s S;
  s.x := 7;
  ASSERT s.x = 7;
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

    std::fs::write(
        client.join("src/main.ch"),
        r#"
Client: MODULE
  SEIZE S;
  DCL s S;
  s.hidden := TRUE;
END Client;
"#,
    )
    .unwrap();
    let output = z200c().arg("check").arg(&client).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("FORBIDden"), "stderr: {stderr}");
}

#[test]
fn cross_package_set_reference_and_moreta_components() {
    let base = TempDir::new("z200c-grant-modes");
    let lib = base.join("libmodes");
    std::fs::create_dir_all(lib.join("src")).unwrap();

    std::fs::write(
        lib.join("Chill.toml"),
        r#"
[package]
name = "libmodes"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        lib.join("src/main.ch"),
        r#"
Lib: MODULE
  GRANT S, P, shared;

  NEWMODE S = SET(low, high);

  NEWMODE P = MODULE SPEC
    GRANT value, Set;
    DCL value INT;
    Set: PROC (n INT) END;
    Reset: PROC () END;
  END P;
  NEWMODE P = MODULE BODY
    DCL value INT INIT := 2;
    Set: PROC (n INT);
      value := n;
    END Set;
    Reset: PROC ();
      value := 0;
    END Reset;
  END P;

  DCL shared S INIT := high;
END Lib;
"#,
    )
    .unwrap();

    let grant = z200c().arg("grant").arg(&lib).output().unwrap();
    assert!(
        grant.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&grant.stdout),
        String::from_utf8_lossy(&grant.stderr)
    );

    let client = base.join("client");
    std::fs::create_dir_all(client.join("src")).unwrap();
    std::fs::write(
        client.join("Chill.toml"),
        format!(
            r#"
[package]
name = "client"

[build]
entry = "src/main.ch"

[dependencies]
libmodes = "{}"
"#,
            lib.display()
        ),
    )
    .unwrap();
    std::fs::write(
        client.join("src/main.ch"),
        r#"
Client: MODULE
  SEIZE S, P, shared;
  DCL p P;
  DCL s S;
  DCL r REF P;

  p.Set(7);
  ASSERT p.value = 7;
  s := shared;
  r := NULL;
  ASSERT r = NULL;
  ASSERT s = shared;
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

    std::fs::write(
        client.join("src/main.ch"),
        r#"
Client: MODULE
  SEIZE P;
  DCL p P;
  p.Reset();
END Client;
"#,
    )
    .unwrap();
    let output = z200c().arg("check").arg(&client).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not accessible") || stderr.contains("not public"),
        "stderr: {stderr}"
    );
}

#[test]
fn runs_single_file_script_without_manifest() {
    let dir = TempDir::new("z200c-script");
    let script = dir.join("hello.ch");
    std::fs::write(
        &script,
        r#"
Hello: MODULE
  DCL x INT INIT := 3;
  ASSERT x = 3;
END Hello;
"#,
    )
    .unwrap();
    // A sibling source must not be loaded in script mode.
    std::fs::write(dir.join("broken.ch"), "this is not CHILL").unwrap();

    let check = z200c().arg("check").arg(&script).output().unwrap();
    assert!(
        check.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(String::from_utf8_lossy(&check.stdout).contains("checked hello"));

    let run = z200c().arg("run").arg(&script).output().unwrap();
    assert!(
        run.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn write_and_writeline_text_io() {
    let dir = TempDir::new("z200-text-out");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("Chill.toml"), "").unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  WRITE("a");
  WRITE("b");
  WRITELINE("c");
  WRITELINE();
END M;
"#,
    )
    .unwrap();

    let output = z200c().arg("run").arg(&dir).output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "abc\n\n");
}

#[test]
fn readline_text_io() {
    let dir = TempDir::new("z200-text-in");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("Chill.toml"), "").unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
M: MODULE
  DCL line CHARS;
  line := READLINE();
  ASSERT line = "hello";
END M;
"#,
    )
    .unwrap();

    let mut child = z200c()
        .arg("run")
        .arg(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(b"hello\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn minimal_manifest_defaults_name_entry_and_sources() {
    let dir = TempDir::new("z200-minimal");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("Chill.toml"), "").unwrap();
    std::fs::write(
        dir.join("src/lib.ch"),
        "Lib: MODULE\n  GRANT x;\n  DCL x INT INIT := 1;\nEND Lib;\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        "Main: MODULE\n  SEIZE x;\n  ASSERT x = 1;\nEND Main;\n",
    )
    .unwrap();

    let output = z200c().arg("run").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn remote_piece_must_exist() {
    let dir = TempDir::new("z200-piece-missing");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "remote"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(dir.join("src/main.ch"), "REMOTE \"pieces/missing.ch\";\n").unwrap();

    let output = z200c().arg("check").arg(&dir).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E0700"), "stderr: {stderr}");
    assert!(stderr.contains("piece file not found"), "stderr: {stderr}");
}

#[test]
fn remote_piece_is_expanded() {
    let dir = TempDir::new("z200-piece-present");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("pieces")).unwrap();
    std::fs::write(dir.join("pieces/counter.ch"), "M: MODULE END M;\n").unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "remote"

[build]
entry = "src/main.ch"

[pieces]
counter = "pieces/counter.ch"
"#,
    )
    .unwrap();
    std::fs::write(dir.join("src/main.ch"), "REMOTE counter;\n").unwrap();

    let output = z200c().arg("check").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn named_remote_module_expands_and_runs() {
    let dir = TempDir::new("z200-piece-run");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("pieces")).unwrap();
    std::fs::write(
        dir.join("pieces/helper.ch"),
        r#"
Helper: MODULE
  ASSERT 1 = 1;
END Helper;
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "remote"

[build]
entry = "src/main.ch"

[pieces]
helper = "pieces/helper.ch"
"#,
    )
    .unwrap();
    std::fs::write(dir.join("src/main.ch"), "Helper: REMOTE helper;\n").unwrap();

    let output = z200c().arg("run").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn remote_spec_module_expands() {
    let dir = TempDir::new("z200-piece-spec");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("pieces")).unwrap();
    std::fs::write(
        dir.join("pieces/spec.ch"),
        r#"
Counter: SPEC MODULE
  GRANT value;
  DCL value INT;
END Counter;
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "remote"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Counter: SPEC REMOTE "pieces/spec.ch";

Counter: MODULE
  DCL value INT INIT := 1;
END Counter;
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
}

#[test]
fn nested_spec_grants_become_virtual_grants() {
    let dir = TempDir::new("z200-nested-spec");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nested"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Outer: SPEC MODULE
  Inner: SPEC MODULE
    DCL x INT;
    GRANT x;
  END Inner;
END Outer;

Outer: MODULE
  Inner: MODULE
    DCL x INT INIT := 1;
  END Inner;
  ASSERT x = 1;
END Outer;
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
}

#[test]
fn spec_region_pairs_with_a_region_body() {
    let dir = TempDir::new("z200-region");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "region"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
R: SPEC REGION
  GRANT x;
  DCL x INT;
END R;

R: REGION
  DCL x INT INIT := 1;
END R;

Client: MODULE
  SEIZE x;
  ASSERT x = 1;
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
}

#[test]
fn remote_region_expands() {
    let dir = TempDir::new("z200-region-remote");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("pieces")).unwrap();
    std::fs::write(
        dir.join("pieces/region.ch"),
        r#"
R: REGION
  DCL x INT INIT := 1;
END R;
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "region"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(dir.join("src/main.ch"), "REMOTE \"pieces/region.ch\";\n").unwrap();

    let output = z200c().arg("run").arg(&dir).output().unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn nested_spec_in_a_context_pairs_with_the_body_module() {
    let dir = TempDir::new("z200-ctx-spec");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nested"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
CONTEXT
  Inner: SPEC MODULE
    DCL x INT;
    GRANT x;
  END Inner;
FOR
Outer: MODULE
  Inner: MODULE
    DCL x INT INIT := 1;
  END Inner;
  ASSERT x = 1;
END Outer;
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
}

#[test]
fn nested_specs_are_checked_recursively() {
    let dir = TempDir::new("z200-deep-spec");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nested"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Outer: SPEC MODULE
  Inner: SPEC MODULE
    GRANT x;
    Deep: SPEC MODULE
      DCL x INT;
      GRANT x;
    END Deep;
  END Inner;
END Outer;

Outer: MODULE
  Inner: MODULE
    Deep: MODULE
      DCL x INT INIT := 1;
    END Deep;
  END Inner;
  ASSERT x = 1;
END Outer;
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
}

#[test]
fn nested_spec_interface_must_match_its_body() {
    let dir = TempDir::new("z200-nested-spec-bad");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "nested"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
Outer: SPEC MODULE
  Inner: SPEC MODULE
    DCL x BOOL;
  END Inner;
END Outer;

Outer: MODULE
  Inner: MODULE
    DCL x INT;
  END Inner;
END Outer;
"#,
    )
    .unwrap();

    let output = z200c().arg("check").arg(&dir).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("mode"), "stderr: {stderr}");
}

#[test]
fn remote_context_is_merged_after_the_last_context_body() {
    let dir = TempDir::new("z200-piece-ctx");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("pieces")).unwrap();
    std::fs::write(
        dir.join("pieces/ctx.ch"),
        r#"
CONTEXT
  DCL x INT;
FOR
CONTEXT
FOR
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "remote"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
A: MODULE
  GRANT x;
  DCL x INT INIT := 1;
END A;

CONTEXT REMOTE "pieces/ctx.ch"
  SEIZE x;
FOR
M: MODULE
  SEIZE x;
  ASSERT x = 1;
END M;
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
}

#[test]
fn context_module_remote_is_desugared() {
    let dir = TempDir::new("z200-piece-ctxmod");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("pieces")).unwrap();
    std::fs::write(
        dir.join("pieces/ctx.ch"),
        r#"
CONTEXT
  DCL x INT;
FOR
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "remote"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
A: MODULE
  GRANT x;
  DCL x INT INIT := 1;
END A;

CONTEXT MODULE REMOTE "pieces/ctx.ch";
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
}

#[test]
fn remote_context_target_must_be_a_context_list() {
    let dir = TempDir::new("z200-piece-ctxkind");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("pieces")).unwrap();
    std::fs::write(dir.join("pieces/module.ch"), "M: MODULE END M;\n").unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "remote"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        r#"
CONTEXT REMOTE "pieces/module.ch" FOR
M: MODULE
END M;
"#,
    )
    .unwrap();

    let output = z200c().arg("check").arg(&dir).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E0700"), "stderr: {stderr}");
}

#[test]
fn remote_piece_kind_mismatch_is_reported() {
    let dir = TempDir::new("z200-piece-kind");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("pieces")).unwrap();
    std::fs::write(
        dir.join("pieces/spec.ch"),
        "Counter: SPEC MODULE END Counter;\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("Chill.toml"),
        r#"
[package]
name = "remote"

[build]
entry = "src/main.ch"
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.ch"),
        "Counter: REMOTE \"pieces/spec.ch\";\n",
    )
    .unwrap();

    let output = z200c().arg("check").arg(&dir).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("E0700"), "stderr: {stderr}");
    assert!(stderr.contains("not a module"), "stderr: {stderr}");
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
